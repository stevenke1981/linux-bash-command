use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, io,
};
use std::{
    fs,
    io::{BufRead, IsTerminal, Write},
    path::Path,
};
const HELP: &str = "Usage: rm [OPTION]... [FILE]...\nRemove files or directories.\n  -f, --force            ignore nonexistent files\n  -i                     prompt before every removal\n  -I                     prompt once before removing recursively or more than three files\n      --interactive[=WHEN]  prompt never, once or always\n  -r, -R, --recursive    remove directories recursively\n  -d, --dir              remove empty directories\n  -v, --verbose          explain removals\n      --one-file-system  stay on the starting filesystem\n      --preserve-root[=all]  protect filesystem roots\n      --no-preserve-root accepted; filesystem roots remain protected\n      --help             display this help\n      --version          output version information\n";
#[derive(Clone, Copy, PartialEq)]
enum Interactive {
    Never,
    Once,
    Always,
}
struct Options {
    force: bool,
    interactive: Interactive,
    recursive: bool,
    dir: bool,
    verbose: bool,
    one_fs: bool,
}

/// Pure root and terminal dot checks: never touches the named path.
pub fn refuse_reason(path: &str) -> Option<&'static str> {
    let path = path.replace('\\', "/");
    let trimmed = path.trim_end_matches('/');
    if trimmed == "." || trimmed == ".." || trimmed.ends_with("/.") || trimmed.ends_with("/..") {
        return Some("dot");
    }
    if !path.is_empty() && trimmed.is_empty() {
        return Some("root");
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && path.ends_with('/')
    {
        return Some("root");
    }
    if path.to_ascii_lowercase().starts_with("//?/unc/") {
        let parts: Vec<_> = trimmed
            .get(8..)
            .unwrap_or("")
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() == 2 {
            return Some("root");
        }
    }
    if path.starts_with("//") {
        let parts: Vec<_> = trimmed
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() == 2 {
            return Some("root");
        }
    }
    None
}

fn inspection_path(name: &str) -> (&str, bool) {
    #[cfg(windows)]
    let trimmed = name.trim_end_matches(['/', '\\']);
    #[cfg(not(windows))]
    let trimmed = name.trim_end_matches('/');
    if trimmed.is_empty() {
        (name, false)
    } else {
        (trimmed, trimmed.len() != name.len())
    }
}

fn ask(question: &str) -> bool {
    eprint!("{question}");
    let _ = std::io::stderr().flush();
    let mut response = String::new();
    let _ = std::io::stdin().lock().read_line(&mut response);
    response.starts_with(['y', 'Y'])
}
#[cfg(unix)]
fn device(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}
#[cfg(not(unix))]
fn device(_: &fs::Metadata) -> u64 {
    0
}

fn remove_entry(
    path: &Path,
    options: &Options,
    root_device: u64,
    out: &mut dyn Write,
) -> std::io::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if options.force && e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => {
            eprintln!(
                "rm: cannot remove '{}': {}",
                super::display(path),
                error::io_text(&e)
            );
            return Ok(false);
        }
    };
    let is_dir = metadata.is_dir() && !metadata.file_type().is_symlink();
    if is_dir && !options.recursive && !options.dir {
        eprintln!(
            "rm: cannot remove '{}': Is a directory",
            super::display(path)
        );
        return Ok(false);
    }
    if is_dir {
        match fs::canonicalize(path) {
            Ok(resolved) if refuse_reason(&resolved.to_string_lossy()) == Some("root") => {
                eprintln!(
                    "rm: it is dangerous to operate recursively on '{}'",
                    super::display(path)
                );
                return Ok(false);
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!(
                    "rm: cannot resolve '{}': {}",
                    super::display(path),
                    error::io_text(&error)
                );
                return Ok(false);
            }
        }
    }
    if is_dir && options.recursive && options.one_fs && device(&metadata) != root_device {
        eprintln!(
            "rm: skipping '{}' since it's on a different device",
            super::display(path)
        );
        return Ok(false);
    }
    let prompt_readonly = !options.force
        && options.interactive != Interactive::Always
        && metadata.permissions().readonly()
        && std::io::stdin().is_terminal();
    if is_dir
        && options.recursive
        && options.interactive == Interactive::Always
        && !ask(&format!(
            "rm: descend into directory '{}'? ",
            super::display(path)
        ))
    {
        return Ok(true);
    }
    let mut succeeded = true;
    if is_dir && options.recursive {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            succeeded &= remove_entry(&entry.path(), options, root_device, out)?;
                        }
                        Err(e) => {
                            eprintln!(
                                "rm: cannot read '{}': {}",
                                super::display(path),
                                error::io_text(&e)
                            );
                            succeeded = false;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "rm: cannot remove '{}': {}",
                    super::display(path),
                    error::io_text(&e)
                );
                return Ok(false);
            }
        }
        if !succeeded {
            return Ok(false);
        }
    }
    if (options.interactive == Interactive::Always || prompt_readonly)
        && !ask(&format!(
            "rm: remove {}'{}'? ",
            if is_dir { "directory " } else { "" },
            super::display(path)
        ))
    {
        return Ok(true);
    }
    #[cfg(windows)]
    #[allow(clippy::permissions_set_readonly_false)]
    // Windows exposes only the read-only attribute.
    if !is_dir && metadata.permissions().readonly() && !metadata.file_type().is_symlink() {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        if let Err(e) = fs::set_permissions(path, permissions) {
            eprintln!(
                "rm: cannot remove '{}': {}",
                super::display(path),
                error::io_text(&e)
            );
            return Ok(false);
        }
    }
    #[cfg(windows)]
    let dir_link = metadata.file_type().is_symlink() && path.is_dir();
    #[cfg(not(windows))]
    let dir_link = false;
    let result = if is_dir || dir_link {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    match result {
        Ok(()) => {
            if options.verbose {
                writeln!(
                    out,
                    "removed {}'{}'",
                    if is_dir { "directory " } else { "" },
                    super::display(path)
                )?;
            }
            Ok(true)
        }
        Err(e) => {
            eprintln!(
                "rm: cannot remove '{}': {}",
                super::display(path),
                error::io_text(&e)
            );
            Ok(false)
        }
    }
}

pub fn run(argv: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("force", 'f'),
        OptSpec {
            id: "interactive-always",
            short: 'i',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec {
            id: "interactive-once",
            short: 'I',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec::optional("interactive", '\0'),
        OptSpec::flag("recursive", 'r'),
        OptSpec {
            id: "recursive",
            short: 'R',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec::flag("dir", 'd'),
        OptSpec::flag("verbose", 'v'),
        OptSpec::flag("one-file-system", '\0'),
        OptSpec::optional("preserve-root", '\0'),
        OptSpec::flag("no-preserve-root", '\0'),
    ];
    let parsed = match args::parse_command("rm", argv, HELP, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut opts = Options {
        force: false,
        interactive: Interactive::Never,
        recursive: false,
        dir: false,
        verbose: false,
        one_fs: false,
    };
    let mut files = Vec::new();
    for item in parsed {
        match item {
            Item::Operand(p) => files.push(p),
            Item::Opt("force", _) => {
                opts.force = true;
                opts.interactive = Interactive::Never;
            }
            Item::Opt("interactive-always", _) => {
                opts.force = false;
                opts.interactive = Interactive::Always;
            }
            Item::Opt("interactive-once", _) => {
                opts.force = false;
                opts.interactive = Interactive::Once;
            }
            Item::Opt("interactive", value) => {
                opts.force = false;
                opts.interactive = match value.as_deref().unwrap_or("always") {
                    "never" => Interactive::Never,
                    "once" => Interactive::Once,
                    "always" => Interactive::Always,
                    value => {
                        return error::usage(
                            "rm",
                            format!("invalid argument '{value}' for '--interactive'"),
                            1,
                        )
                    }
                };
            }
            Item::Opt("recursive", _) => opts.recursive = true,
            Item::Opt("dir", _) => opts.dir = true,
            Item::Opt("verbose", _) => opts.verbose = true,
            Item::Opt("one-file-system", _) => opts.one_fs = true,
            Item::Opt("preserve-root", Some(v)) if v != "all" => {
                return error::usage(
                    "rm",
                    format!("invalid argument '{v}' for '--preserve-root'"),
                    1,
                )
            }
            Item::Opt("preserve-root" | "no-preserve-root", _) => {}
            _ => unreachable!(),
        }
    }
    if files.is_empty() {
        return if opts.force {
            0
        } else {
            error::usage("rm", "missing operand", 1)
        };
    }
    if opts.interactive == Interactive::Once
        && (files.len() > 3 || opts.recursive)
        && !ask("rm: remove arguments? ")
    {
        return 0;
    }
    let mut failed = false;
    let status = io::output("rm", |out| {
        for name in &files {
            if let Some(reason) = refuse_reason(name) {
                if reason == "dot" {
                    eprintln!("rm: refusing to remove '.' or '..' directory: skipping '{name}'");
                } else {
                    eprintln!("rm: it is dangerous to operate recursively on '{name}'");
                }
                failed = true;
                continue;
            }
            let (inspected, trailing) = inspection_path(name);
            let path = Path::new(inspected);
            let metadata = fs::symlink_metadata(path);
            if trailing
                && metadata
                    .as_ref()
                    .is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink())
            {
                eprintln!("rm: cannot remove '{}': Not a directory", name);
                failed = true;
                continue;
            }
            let root_device = metadata.map(|m| device(&m)).unwrap_or(0);
            failed |= !remove_entry(path, &opts, root_device, out)?;
        }
        Ok(())
    });
    status.max(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::refuse_reason;
    #[test]
    fn root() {
        assert_eq!(refuse_reason("/"), Some("root"));
    }
    #[test]
    fn drives() {
        assert_eq!(refuse_reason("C:/"), Some("root"));
        assert_eq!(refuse_reason("C:\\"), Some("root"));
    }
    #[test]
    fn unc() {
        assert_eq!(refuse_reason("//server/share"), Some("root"));
        assert_eq!(refuse_reason(r"\\?\UNC\server\share"), Some("root"));
        assert_eq!(refuse_reason(r"\\?\C:\"), Some("root"));
        assert_eq!(refuse_reason(r"\\?\UNC\server\share\child"), None);
        assert_eq!(refuse_reason("//?/UNC/"), Some("root"));
    }
    #[test]
    fn dot() {
        assert_eq!(refuse_reason("."), Some("dot"));
        assert_eq!(refuse_reason("a/.."), Some("dot"));
    }
    #[test]
    fn safe() {
        assert_eq!(refuse_reason("sandbox/a"), None);
    }
}
