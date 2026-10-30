//! `chmod`: change file permission bits. On Windows only the read-only attribute changes:
//! a file is read-only when the owner write bit is clear.

use std::fs;
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};
use crate::common::meta;
use crate::common::path::display_path;

use super::helpers::{os_text, print_help, say, umask};
use super::mode::{self, ModeSpec};
use super::safety::{refuse_reason, Refusal};

const HELP: &str = r#"Usage: chmod [OPTION]... MODE[,MODE]... FILE...
  or:  chmod [OPTION]... OCTAL-MODE FILE...
  or:  chmod [OPTION]... --reference=RFILE FILE...
Change the mode of each FILE to MODE.

  -c, --changes          like verbose but report only when a change is made
  -f, --silent, --quiet  suppress most error messages
  -v, --verbose          output a diagnostic for every file processed
      --no-preserve-root  do not treat '/' specially (the default)
      --preserve-root    fail to operate recursively on '/'
      --reference=RFILE  use RFILE's mode instead of MODE values
  -R, --recursive        change files and directories recursively
      --help             display this help and exit
      --version          output version information and exit

Each MODE is of the form [ugoa]*([-+=]([rwxXst]*|[ugo]))+|[-+=]?[0-7]+.
On Windows only the write permission is applied (read-only attribute).
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("changes", Some('c'), Some("changes")),
    OptSpec::flag("silent", Some('f'), Some("silent")),
    OptSpec::flag("silent", None, Some("quiet")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("no-preserve-root", None, Some("no-preserve-root")),
    OptSpec::flag("preserve-root", None, Some("preserve-root")),
    OptSpec::req("reference", None, Some("reference")),
    OptSpec::flag("recursive", Some('R'), Some("recursive")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Options and the chosen mode.
struct Settings {
    spec: ModeSpec,
    changes: bool,
    silent: bool,
    verbose: bool,
    recursive: bool,
    preserve_root: bool,
    umask: u32,
}

/// Current permission bits of a file, as `chmod` sees them.
fn current_mode(path: &Path, meta: &fs::Metadata) -> u32 {
    meta::permission_bits(path, meta) & 0o7777
}

/// Writes new permission bits to a file.
fn write_mode(path: &Path, bits: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(bits))
    }
    #[cfg(not(unix))]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_readonly(bits & 0o200 == 0);
        fs::set_permissions(path, perms)
    }
}

/// Applies the mode to one path and, with `-R`, to everything below it.
fn process(path: &Path, shown: &str, settings: &Settings, top: bool) -> bool {
    let meta = if top {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    };
    let meta = match meta {
        Ok(meta) => meta,
        Err(error) => {
            if !settings.silent {
                eprintln!("chmod: cannot access '{shown}': {}", os_text(&error));
            }
            return false;
        }
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    let is_dir = meta.is_dir();
    let mut ok = change_one(path, shown, &meta, settings);
    if is_dir && settings.recursive {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let name = entry.file_name().to_string_lossy().into_owned();
                            let child_shown = format!("{shown}/{name}");
                            let child_ok = process(&entry.path(), &child_shown, settings, false);
                            ok &= child_ok;
                        }
                        Err(error) => {
                            if !settings.silent {
                                eprintln!("chmod: cannot access '{shown}': {}", os_text(&error));
                            }
                            ok = false;
                        }
                    }
                }
            }
            Err(error) => {
                if !settings.silent {
                    eprintln!(
                        "chmod: cannot read directory '{shown}': {}",
                        os_text(&error)
                    );
                }
                ok = false;
            }
        }
    }
    ok
}

/// Changes the mode of one already-stat'ed path and reports as requested.
fn change_one(path: &Path, shown: &str, meta: &fs::Metadata, settings: &Settings) -> bool {
    let old = current_mode(path, meta);
    let new = mode::apply(&settings.spec, old, meta.is_dir(), settings.umask);
    let changed = new != old;
    if changed {
        if let Err(error) = write_mode(path, new) {
            if !settings.silent {
                eprintln!(
                    "chmod: changing permissions of '{shown}': {}",
                    os_text(&error)
                );
            }
            return false;
        }
    }
    if settings.verbose || (settings.changes && changed) {
        if changed {
            say(&format!(
                "mode of '{shown}' changed from {} {} to {} {}",
                octal(old),
                mode::symbolic_text(old),
                octal(new),
                mode::symbolic_text(new)
            ));
        } else {
            say(&format!(
                "mode of '{shown}' retained as {} {}",
                octal(old),
                mode::symbolic_text(old)
            ));
        }
    }
    true
}

fn octal(bits: u32) -> String {
    format!("{:04o}", bits & 0o7777)
}

/// Reads the mode from `--reference` and returns it as an absolute mode.
fn reference_mode(path: &str) -> Result<ModeSpec, String> {
    let meta = fs::metadata(path)
        .map_err(|error| format!("failed to get attributes of '{path}': {}", os_text(&error)))?;
    Ok(ModeSpec::Absolute(current_mode(Path::new(path), &meta)))
}

/// Entry point for `chmod`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("chmod", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("chmod", &error, EXIT_FAIL),
    };
    let mut flags = Flags::default();
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => return print_help("chmod", HELP),
                "version" => return print_version("chmod"),
                "changes" => flags.changes = true,
                "silent" => flags.silent = true,
                "verbose" => flags.verbose = true,
                "no-preserve-root" => flags.preserve_root = false,
                "preserve-root" => flags.preserve_root = true,
                "recursive" => flags.recursive = true,
                "reference" => flags.reference = value,
                _ => {}
            },
        }
    }
    let (spec, files) = match select_mode(&mut operands, flags.reference.as_deref()) {
        Ok(choice) => choice,
        Err(code) => return code,
    };
    let settings = Settings {
        spec,
        changes: flags.changes,
        silent: flags.silent,
        verbose: flags.verbose,
        recursive: flags.recursive,
        preserve_root: flags.preserve_root,
        umask: umask(),
    };
    let mut status = EXIT_OK;
    for file in &files {
        if settings.recursive
            && settings.preserve_root
            && refuse_reason(Path::new(file)) == Some(Refusal::Root)
        {
            eprintln!("chmod: it is dangerous to operate recursively on '{file}'");
            eprintln!("chmod: use --no-preserve-root to override this failsafe");
            status = EXIT_FAIL;
            continue;
        }
        if !process(
            Path::new(file),
            &display_path(Path::new(file)),
            &settings,
            true,
        ) {
            status = EXIT_FAIL;
        }
    }
    status
}

#[derive(Default)]
struct Flags {
    changes: bool,
    silent: bool,
    verbose: bool,
    recursive: bool,
    preserve_root: bool,
    reference: Option<String>,
}

/// Splits operands into the mode and the files, or takes the mode from `--reference`.
fn select_mode(
    operands: &mut Vec<String>,
    reference: Option<&str>,
) -> Result<(ModeSpec, Vec<String>), i32> {
    if let Some(path) = reference {
        if operands.is_empty() {
            return Err(usage_error("missing operand".to_string()));
        }
        return match reference_mode(path) {
            Ok(spec) => Ok((spec, std::mem::take(operands))),
            Err(message) => {
                eprintln!("chmod: {message}");
                Err(EXIT_FAIL)
            }
        };
    }
    if operands.is_empty() {
        return Err(usage_error("missing operand".to_string()));
    }
    let text = operands.remove(0);
    let spec = mode::parse(&text).ok_or_else(|| usage_error(format!("invalid mode: '{text}'")))?;
    if operands.is_empty() {
        return Err(usage_error(format!("missing operand after '{text}'")));
    }
    Ok((spec, std::mem::take(operands)))
}

fn usage_error(message: String) -> i32 {
    report_usage("chmod", &UsageError { message }, EXIT_FAIL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_formats_four_digits() {
        assert_eq!(octal(0o644), "0644");
        assert_eq!(octal(0o4755), "4755");
    }

    #[test]
    fn mode_argument_is_split_from_files() {
        let mut ops = vec!["0644".to_string(), "a".to_string(), "b".to_string()];
        let (spec, files) = select_mode(&mut ops, None).unwrap();
        assert_eq!(spec, ModeSpec::Absolute(0o644));
        assert_eq!(files, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn missing_operands_are_usage_errors() {
        assert!(select_mode(&mut Vec::new(), None).is_err());
        assert!(select_mode(&mut vec!["0644".to_string()], None).is_err());
        assert!(select_mode(&mut vec!["u=q".to_string(), "f".to_string()], None).is_err());
    }
}

#[cfg(all(test, unix))]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;
    use std::os::unix::fs::PermissionsExt;

    fn settings(spec: ModeSpec, recursive: bool) -> Settings {
        Settings {
            spec,
            changes: false,
            silent: false,
            verbose: false,
            recursive,
            preserve_root: true,
            umask: 0o022,
        }
    }

    fn bits(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn absolute_mode_is_written() {
        let t = TempDir::new("chmod-abs");
        let file = t.write("f", "x");
        let s = settings(ModeSpec::Absolute(0o600), false);
        assert!(process(&file, "f", &s, true));
        assert_eq!(bits(&file), 0o600);
    }

    #[test]
    fn recursive_applies_to_directory_contents() {
        let t = TempDir::new("chmod-rec");
        let file = t.write("d/f", "x");
        // The directory keeps its search bit so non-root users can recurse into it.
        let s = settings(ModeSpec::Absolute(0o750), true);
        assert!(process(&t.at("d"), "d", &s, true));
        assert_eq!(bits(&file), 0o750);
    }

    #[test]
    fn missing_operand_reports_failure() {
        let t = TempDir::new("chmod-missing");
        let s = settings(ModeSpec::Absolute(0o600), false);
        assert!(!process(&t.at("nope"), "nope", &s, true));
    }
}
