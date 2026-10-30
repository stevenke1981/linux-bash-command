//! `mkdir`: make directories. `-m` sets the mode of the final directory exactly (not subject
//! to the umask); `-p` creates missing parents with the umask applied.

use std::fs;
use std::io;
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::path::display_path;

use super::helpers::{os_text, print_help, say, umask};
use super::mode::{self, ModeSpec};

const HELP: &str = r#"Usage: mkdir [OPTION]... DIRECTORY...
Create the DIRECTORY(ies), if they do not already exist.

Mandatory arguments to long options are mandatory for short options too.
  -m, --mode=MODE   set file mode (as in chmod), not a=rwx - umask
                    (on Windows only the write permission is applied)
  -p, --parents     no error if existing, make parent directories as needed
  -v, --verbose     print a message for each created directory
      --help        display this help and exit
      --version     output version information and exit
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::req("mode", Some('m'), Some("mode")),
    OptSpec::flag("parents", Some('p'), Some("parents")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("context-short", Some('Z'), None),
    OptSpec::opt("context", None, Some("context")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Options after parsing.
struct Options {
    mode: Option<ModeSpec>,
    parents: bool,
    verbose: bool,
}

/// Creates one directory with the default mode, subject to the umask.
fn create_one(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o777).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

/// Sets the exact permission bits of a directory created by this run.
fn set_mode(path: &Path, bits: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(bits & 0o7777))
    }
    #[cfg(not(unix))]
    {
        let readonly = bits & 0o200 == 0;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_readonly(readonly);
        fs::set_permissions(path, perms)
    }
}

/// Creates `operand` (and its parents with `-p`). Returns the error text on failure.
fn make_dir(operand: &str, opts: &Options) -> Result<(), String> {
    let path = Path::new(operand);
    let mode_bits = opts
        .mode
        .as_ref()
        .map(|spec| mode::apply(spec, 0o777, true, umask()));
    if !opts.parents {
        return create_one(path)
            .map(|()| {
                if opts.verbose {
                    say(&format!("mkdir: created directory '{operand}'"));
                }
                if let Some(bits) = mode_bits {
                    let _ = set_mode(path, bits);
                }
            })
            .map_err(|error| format!("cannot create directory '{operand}': {}", os_text(&error)));
    }
    if let Ok(meta) = fs::metadata(path) {
        return if meta.is_dir() {
            Ok(())
        } else {
            Err(format!("cannot create directory '{operand}': File exists"))
        };
    }
    let ancestors: Vec<&Path> = path
        .ancestors()
        .skip(1)
        .filter(|p| !p.as_os_str().is_empty())
        .collect();
    for ancestor in ancestors.iter().rev() {
        match fs::metadata(ancestor) {
            Ok(meta) if meta.is_dir() => continue,
            Ok(_) => {
                return Err(format!(
                    "cannot create directory '{operand}': Not a directory"
                ));
            }
            Err(_) => {}
        }
        create_missing(ancestor, operand, opts.verbose)?;
    }
    create_missing(path, operand, opts.verbose)?;
    if let Some(bits) = mode_bits {
        let _ = set_mode(path, bits);
    }
    Ok(())
}

/// Creates one missing directory of a `-p` chain, reporting it under `-v`.
fn create_missing(path: &Path, operand: &str, verbose: bool) -> Result<(), String> {
    create_one(path)
        .map_err(|error| format!("cannot create directory '{operand}': {}", os_text(&error)))?;
    if verbose {
        say(&format!(
            "mkdir: created directory '{}'",
            display_path(path)
        ));
    }
    Ok(())
}

/// Entry point for `mkdir`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("mkdir", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("mkdir", &error, EXIT_FAIL),
    };
    let mut opts = Options {
        mode: None,
        parents: false,
        verbose: false,
    };
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => return print_help("mkdir", HELP),
                "version" => return print_version("mkdir"),
                "mode" => {
                    let text = value.unwrap_or_default();
                    match mode::parse(&text) {
                        Some(spec) => opts.mode = Some(spec),
                        None => {
                            eprintln!("mkdir: invalid mode '{text}'");
                            return EXIT_FAIL;
                        }
                    }
                }
                "parents" => opts.parents = true,
                "verbose" => opts.verbose = true,
                "context-short" => return unsupported("mkdir", "-Z"),
                "context" => return unsupported("mkdir", "--context"),
                _ => {}
            },
        }
    }
    if operands.is_empty() {
        return report_usage(
            "mkdir",
            &UsageError {
                message: "missing operand".to_string(),
            },
            EXIT_FAIL,
        );
    }
    let mut status = EXIT_OK;
    for operand in &operands {
        if let Err(message) = make_dir(operand, &opts) {
            eprintln!("mkdir: {message}");
            status = EXIT_FAIL;
        }
    }
    status
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;

    fn opts(parents: bool) -> Options {
        Options {
            mode: None,
            parents,
            verbose: false,
        }
    }

    #[test]
    fn creates_a_single_directory() {
        let t = TempDir::new("mkdir-single");
        let target = t.at("d");
        make_dir(target.to_str().unwrap(), &opts(false)).unwrap();
        assert!(target.is_dir());
    }

    #[test]
    fn existing_directory_is_an_error_without_parents() {
        let t = TempDir::new("mkdir-exists");
        fs::create_dir(t.at("d")).unwrap();
        let error = make_dir(t.at("d").to_str().unwrap(), &opts(false)).unwrap_err();
        assert!(error.contains("File exists"), "{error}");
    }

    #[test]
    fn parents_creates_the_chain_and_accepts_existing_directories() {
        let t = TempDir::new("mkdir-chain");
        let target = t.at("a/b/c");
        let operand = target.to_str().unwrap();
        make_dir(operand, &opts(true)).unwrap();
        assert!(target.is_dir());
        make_dir(operand, &opts(true)).unwrap();
    }

    #[test]
    fn parents_through_a_regular_file_fails() {
        let t = TempDir::new("mkdir-through-file");
        t.write("f", "x");
        let operand = t.at("f/g");
        let error = make_dir(operand.to_str().unwrap(), &opts(true)).unwrap_err();
        assert!(error.contains("Not a directory"), "{error}");
    }

    #[test]
    fn mode_is_applied_to_the_final_directory() {
        let t = TempDir::new("mkdir-mode");
        let target = t.at("m");
        let options = Options {
            mode: Some(ModeSpec::Absolute(0o700)),
            parents: false,
            verbose: false,
        };
        make_dir(target.to_str().unwrap(), &options).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let bits = fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(bits & 0o777, 0o700);
        }
    }
}
