use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};
const HELP: &str = "Usage: mktemp [OPTION]... [TEMPLATE]\nCreate a temporary file or directory.\n  -d, --directory       create a directory\n  -u, --dry-run         do not create anything\n  -q, --quiet           suppress errors\n      --suffix=SUFF     append SUFF to TEMPLATE\n  -p DIR, --tmpdir[=DIR]  use DIR or the temporary directory\n  -t                    interpret TEMPLATE relative to temporary directory\n      --help            display this help\n      --version         output version information\n";

fn split_template(
    template: &str,
    suffix: Option<&str>,
) -> Result<(String, usize, String), &'static str> {
    let (stem, suffix) = if let Some(suffix) = suffix {
        (template, suffix)
    } else {
        let Some(last) = template.rfind('X') else {
            return Err("too few X's in template");
        };
        (&template[..=last], &template[last + 1..])
    };
    if suffix.contains(['/', '\\']) {
        return Err("invalid suffix");
    }
    let count = stem.bytes().rev().take_while(|b| *b == b'X').count();
    if count < 3 {
        return Err("too few X's in template");
    }
    Ok((
        stem[..stem.len() - count].to_owned(),
        count,
        suffix.to_owned(),
    ))
}

#[cfg(unix)]
fn create(path: &Path, directory: bool) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    if directory {
        fs::DirBuilder::new().mode(0o700).create(path)
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map(|_| ())
    }
}
#[cfg(not(unix))]
fn create(path: &Path, directory: bool) -> std::io::Result<()> {
    if directory {
        fs::create_dir(path)
    } else {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(|_| ())
    }
}
fn candidate(prefix: &str, count: usize, suffix: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut name = String::with_capacity(prefix.len() + count + suffix.len());
    name.push_str(prefix);
    for _ in 0..count {
        name.push(char::from(ALPHABET[rand::random_range(0..ALPHABET.len())]));
    }
    name.push_str(suffix);
    name
}

pub fn run(argv: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("directory", 'd'),
        OptSpec::flag("dry-run", 'u'),
        OptSpec::flag("quiet", 'q'),
        OptSpec::value("suffix", '\0'),
        OptSpec {
            id: "tmpdir-value",
            short: 'p',
            long: "",
            has_arg: args::HasArg::Required,
        },
        OptSpec::optional("tmpdir", '\0'),
        OptSpec::flag("temp", 't'),
    ];
    let items = match args::parse_command("mktemp", argv, HELP, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let (mut directory, mut dry, mut quiet, mut suffix, mut tmpdir, mut temp, mut templates) =
        (false, false, false, None, None, false, Vec::new());
    let mut legacy_t = false;
    for item in items {
        match item {
            Item::Opt("directory", _) => directory = true,
            Item::Opt("dry-run", _) => dry = true,
            Item::Opt("quiet", _) => quiet = true,
            Item::Opt("suffix", v) => suffix = v,
            Item::Opt("tmpdir-value", v) => {
                temp = true;
                tmpdir = v.map(PathBuf::from);
            }
            Item::Opt("tmpdir", v) => {
                temp = true;
                tmpdir = v.map(PathBuf::from);
            }
            Item::Opt("temp", _) => {
                temp = true;
                legacy_t = true;
            }
            Item::Operand(v) => templates.push(v),
            _ => unreachable!(),
        }
    }
    if templates.len() > 1 {
        return error::usage(
            "mktemp",
            format!("too many templates: '{}'", templates[1]),
            1,
        );
    }
    let template = templates.first().map_or("tmp.XXXXXXXXXX", String::as_str);
    if templates.is_empty() {
        temp = true;
    }
    if temp && (Path::new(template).is_absolute() || legacy_t && template.contains(['/', '\\'])) {
        if !quiet {
            eprintln!(
                "mktemp: invalid template, '{}' contains directory separator",
                template
            );
        }
        return 1;
    }
    let (prefix, count, suffix) = match split_template(template, suffix.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            if !quiet {
                eprintln!("mktemp: {e} '{template}'");
            }
            return 1;
        }
    };
    let base = if temp {
        tmpdir.unwrap_or_else(std::env::temp_dir)
    } else {
        PathBuf::new()
    };
    for _ in 0..128 {
        let path = base.join(candidate(&prefix, count, &suffix));
        let result = if dry {
            if path.exists() {
                Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
            } else {
                Ok(())
            }
        } else {
            create(&path, directory)
        };
        match result {
            Ok(()) => {
                return io::output("mktemp", |out| {
                    writeln!(out, "{}", crate::common::path::display_path(&path))
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                if !quiet {
                    eprintln!(
                        "mktemp: failed to create {} via template '{}': {}",
                        if directory { "directory" } else { "file" },
                        template,
                        error::io_text(&e)
                    );
                }
                return 1;
            }
        }
    }
    if !quiet {
        eprintln!("mktemp: failed to create file via template '{template}': File exists");
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic() {
        assert_eq!(
            split_template("fooXXX", None).unwrap(),
            ("foo".into(), 3, "".into())
        );
    }
    #[test]
    fn implicit_suffix() {
        assert_eq!(
            split_template("fooXXXX.txt", None).unwrap(),
            ("foo".into(), 4, ".txt".into())
        );
    }
    #[test]
    fn explicit_suffix() {
        assert_eq!(
            split_template("fooXXX", Some(".txt")).unwrap(),
            ("foo".into(), 3, ".txt".into())
        );
    }
    #[test]
    fn short() {
        assert!(split_template("fooXX", None).is_err());
    }
    #[test]
    fn suffix_slash() {
        assert!(split_template("fooXXX", Some("/x")).is_err());
    }
}
