//! `link` and `unlink`: the bare two-argument and one-argument forms of `ln` and `rm`.
//! Neither takes options beyond `--help` and `--version`.

use std::fs;
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

use super::helpers::{os_text, print_help};

const LINK_HELP: &str = r#"Usage: link FILE1 FILE2
  or:  link OPTION
Call the link function to create a link named FILE2 to an existing FILE1.

      --help     display this help and exit
      --version  output version information and exit
"#;

const UNLINK_HELP: &str = r#"Usage: unlink FILE
  or:  unlink OPTION
Call the unlink function to remove the specified FILE.

      --help     display this help and exit
      --version  output version information and exit
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Parses the arguments shared by both commands. `Err` carries the exit status for `--help`,
/// `--version` or a usage error; `Ok` carries the operands.
fn operands(cmd: &str, help: &str, args: &[String]) -> Result<Vec<String>, i32> {
    let items = Parser::new(cmd, SPECS)
        .parse(args)
        .map_err(|error| report_usage(cmd, &error, EXIT_FAIL))?;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id: "help", .. } => return Err(print_help(cmd, help)),
            Item::Opt { id: "version", .. } => return Err(print_version(cmd)),
            Item::Opt { .. } => {}
        }
    }
    Ok(operands)
}

fn usage(cmd: &str, message: String) -> i32 {
    report_usage(cmd, &UsageError { message }, EXIT_FAIL)
}

/// Entry point for `link FILE1 FILE2`.
pub fn run_link(args: &[String]) -> i32 {
    let operands = match operands("link", LINK_HELP, args) {
        Ok(operands) => operands,
        Err(code) => return code,
    };
    match operands.as_slice() {
        [] => usage("link", "missing operand".to_string()),
        [only] => usage("link", format!("missing operand after '{only}'")),
        [target, link_name] => match fs::hard_link(Path::new(target), Path::new(link_name)) {
            Ok(()) => EXIT_OK,
            Err(error) => {
                eprintln!(
                    "link: cannot create link '{link_name}' to '{target}': {}",
                    os_text(&error)
                );
                EXIT_FAIL
            }
        },
        [_, _, extra, ..] => usage("link", format!("extra operand '{extra}'")),
    }
}

/// Entry point for `unlink FILE`.
pub fn run_unlink(args: &[String]) -> i32 {
    let operands = match operands("unlink", UNLINK_HELP, args) {
        Ok(operands) => operands,
        Err(code) => return code,
    };
    match operands.as_slice() {
        [] => usage("unlink", "missing operand".to_string()),
        [file] => match fs::remove_file(Path::new(file)) {
            Ok(()) => EXIT_OK,
            Err(error) => {
                eprintln!("unlink: cannot unlink '{file}': {}", os_text(&error));
                EXIT_FAIL
            }
        },
        [_, extra, ..] => usage("unlink", format!("extra operand '{extra}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operands_pass_through_plain_arguments() {
        let args = vec!["a".to_string(), "b".to_string()];
        assert_eq!(operands("link", LINK_HELP, &args), Ok(args.clone()));
    }

    #[test]
    fn unknown_options_are_usage_errors() {
        let args = vec!["-x".to_string()];
        assert_eq!(operands("link", LINK_HELP, &args), Err(EXIT_FAIL));
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;

    #[test]
    fn link_then_unlink_round_trip() {
        let t = TempDir::new("link-round");
        let a = t.write("a", "x");
        let b = t.at("b");
        let args = vec![
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned(),
        ];
        assert_eq!(run_link(&args), EXIT_OK);
        assert!(b.exists());
        let args = vec![b.to_string_lossy().into_owned()];
        assert_eq!(run_unlink(&args), EXIT_OK);
        assert!(!b.exists());
        assert!(a.exists());
    }

    #[test]
    fn unlink_of_a_directory_fails() {
        let t = TempDir::new("unlink-dir");
        fs::create_dir(t.at("d")).unwrap();
        let args = vec![t.at("d").to_string_lossy().into_owned()];
        assert_eq!(run_unlink(&args), EXIT_FAIL);
        assert!(t.at("d").is_dir());
    }
}
