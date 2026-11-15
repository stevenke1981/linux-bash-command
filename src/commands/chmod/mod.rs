mod mode;
use crate::common::{
    args::{self, Item, OptSpec},
    error, io, meta,
    path::display_path,
};
use std::{fs, io::Write, path::Path};

const HELP: &str = "Usage: chmod [OPTION]... MODE FILE...\n  or: chmod [OPTION]... --reference=RFILE FILE...\nChange file modes. Octal and symbolic modes are supported.\nOn Windows only owner write permission changes the read-only attribute.\n  -c, --changes            report changed files\n  -f, --silent, --quiet    suppress errors\n  -v, --verbose            report every file\n  -R, --recursive          change directories recursively\n      --reference=RFILE    use RFILE's mode\n      --preserve-root      refuse recursive filesystem roots\n      --no-preserve-root   allow recursive filesystem roots (default)\n      --help               display help\n      --version            output version\nExit status: 0 success, 1 failure.\n";
struct Options {
    mode: String,
    reference: Option<u32>,
    recursive: bool,
    quiet: bool,
    report: u8,
    preserve: bool,
    mask: u32,
}

pub fn run(arguments: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("changes", 'c'),
        OptSpec::flag("silent", 'f'),
        OptSpec {
            id: "silent",
            long: "quiet",
            short: '\0',
            has_arg: args::HasArg::No,
        },
        OptSpec::flag("verbose", 'v'),
        OptSpec::flag("recursive", 'R'),
        OptSpec::value("reference", '\0'),
        OptSpec::flag("preserve-root", '\0'),
        OptSpec::flag("no-preserve-root", '\0'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let replacements: std::collections::HashMap<_, _> = arguments
        .iter()
        .enumerate()
        .filter(|(_, s)| negative_mode(s))
        .map(|(i, s)| (format!("\0agentcmd-mode:{i}"), s.clone()))
        .collect();
    let rewritten: Vec<_> = arguments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if negative_mode(s) {
                format!("\0agentcmd-mode:{i}")
            } else {
                s.clone()
            }
        })
        .collect();
    let items = match args::parse(&rewritten, &specs, true) {
        Ok(v) => v,
        Err(e) => return error::usage("chmod", e, 1),
    };
    let mut options = Options {
        mode: String::new(),
        reference: None,
        recursive: false,
        quiet: false,
        report: 0,
        preserve: false,
        mask: creation_mask(),
    };
    let mut files = Vec::new();
    for item in items {
        match item {
            Item::Operand(s) => files.push(replacements.get(&s).cloned().unwrap_or(s)),
            Item::Opt(id, value) => match id {
                "help" | "version" => {
                    return args::help_version("chmod", &[format!("--{id}")], HELP, true).unwrap()
                }
                "changes" => options.report = 1,
                "verbose" => options.report = 2,
                "silent" => options.quiet = true,
                "recursive" => options.recursive = true,
                "preserve-root" => options.preserve = true,
                "no-preserve-root" => options.preserve = false,
                "reference" => {
                    let name = value.unwrap();
                    match fs::metadata(&name) {
                        Ok(m) => {
                            options.reference = Some(meta::permission_bits_at(Path::new(&name), &m))
                        }
                        Err(e) => {
                            eprintln!(
                                "chmod: failed to get attributes of '{}': {}",
                                name,
                                error::io_text(&e)
                            );
                            return 1;
                        }
                    }
                }
                _ => unreachable!(),
            },
        }
    }
    if options.reference.is_none() && !files.is_empty() {
        options.mode = files.remove(0);
        if let Err(e) = mode::parse(&options.mode, 0, false, options.mask) {
            return error::usage("chmod", e, 1);
        }
    }
    if files.is_empty() {
        return error::usage("chmod", "missing operand", 1);
    }
    let mut failed = false;
    io::output_result("chmod", 1, |out| {
        for file in files {
            failed |= !visit(Path::new(&file), true, &options, out)?;
        }
        Ok(i32::from(failed))
    })
}
fn negative_mode(s: &str) -> bool {
    s.starts_with('-')
        && !s.starts_with("--")
        && s.len() > 1
        && (s[1..].bytes().all(|b| b"rwxXstugoa,+-=".contains(&b))
            || s[1..].bytes().all(|b| (b'0'..=b'7').contains(&b)))
}
#[cfg(unix)]
fn creation_mask() -> u32 {
    // Called once at CLI startup, before filesystem work or worker threads.
    let value = unsafe { libc::umask(0) };
    unsafe { libc::umask(value) };
    value as u32
}
#[cfg(not(unix))]
fn creation_mask() -> u32 {
    0
}
fn protected_root(path: &Path) -> bool {
    if crate::commands::fsops::rm::refuse_reason(&path.to_string_lossy()) == Some("root") {
        return true;
    }
    fs::canonicalize(path)
        .ok()
        .is_some_and(|p| p.parent().is_none())
}
fn fail(options: &Options, path: &Path, error: &std::io::Error) -> bool {
    if !options.quiet {
        eprintln!(
            "chmod: changing permissions of '{}': {}",
            display_path(path),
            error::io_text(error)
        );
    }
    false
}
fn visit(
    path: &Path,
    operand: bool,
    options: &Options,
    out: &mut dyn Write,
) -> std::io::Result<bool> {
    let link = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return Ok(fail(options, path, &e)),
    };
    if link.file_type().is_symlink() && !operand {
        return Ok(true);
    }
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return Ok(fail(options, path, &e)),
    };
    if options.recursive && metadata.is_dir() && options.preserve && protected_root(path) {
        if !options.quiet {
            eprintln!(
                "chmod: it is dangerous to operate recursively on '{}'",
                display_path(path)
            );
        }
        return Ok(false);
    }
    let old = meta::permission_bits_at(path, &metadata) & 0o7777;
    let new = options.reference.unwrap_or_else(|| {
        mode::parse(&options.mode, old, metadata.is_dir(), options.mask).unwrap()
    });
    let mut success = match apply(path, &metadata, new) {
        Ok(()) => true,
        Err(e) => fail(options, path, &e),
    };
    if success && (options.report == 2 || options.report == 1 && old != new) {
        let verb = if old == new {
            "retained as"
        } else {
            "changed from"
        };
        if old == new {
            writeln!(
                out,
                "mode of '{}' {verb} {:04o} ({})",
                display_path(path),
                new,
                symbolic(new)
            )?;
        } else {
            writeln!(
                out,
                "mode of '{}' {verb} {:04o} ({}) to {:04o} ({})",
                display_path(path),
                old,
                symbolic(old),
                new,
                symbolic(new)
            )?;
        }
    }
    if options.recursive && metadata.is_dir() && !link.file_type().is_symlink() {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(e) => success &= visit(&e.path(), false, options, out)?,
                        Err(e) => {
                            success = fail(options, path, &e);
                        }
                    }
                }
            }
            Err(e) => success = fail(options, path, &e),
        }
    }
    Ok(success)
}
#[cfg(unix)]
fn apply(path: &Path, _metadata: &fs::Metadata, new: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(new))
}
#[cfg(not(unix))]
fn apply(path: &Path, metadata: &fs::Metadata, new: u32) -> std::io::Result<()> {
    let mut permissions = metadata.permissions();
    permissions.set_readonly(new & 0o200 == 0);
    fs::set_permissions(path, permissions)
}
fn symbolic(mode: u32) -> String {
    let mut s = String::new();
    for (shift, special, lower, upper) in [
        (6, 0o4000, 's', 'S'),
        (3, 0o2000, 's', 'S'),
        (0, 0o1000, 't', 'T'),
    ] {
        let n = mode >> shift;
        s.push(if n & 4 != 0 { 'r' } else { '-' });
        s.push(if n & 2 != 0 { 'w' } else { '-' });
        s.push(if mode & special != 0 {
            if n & 1 != 0 {
                lower
            } else {
                upper
            }
        } else if n & 1 != 0 {
            'x'
        } else {
            '-'
        });
    }
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn option_like_modes() {
        assert!(negative_mode("-wx"));
        assert!(!negative_mode("-R"));
        assert!(!negative_mode("--help"));
    }
    #[test]
    fn reports_special_permissions() {
        assert_eq!(symbolic(0o4755), "rwsr-xr-x");
        assert_eq!(symbolic(0o1644), "rw-r--r-T");
    }
    #[test]
    fn roots_are_checked_without_mutation() {
        for p in ["/", "C:/", "C:\\", "//server/share"] {
            assert_eq!(crate::commands::fsops::rm::refuse_reason(p), Some("root"));
        }
    }
}
