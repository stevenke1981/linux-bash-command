//! `pathchk`: check whether file names are valid, and optionally portable.
//!
//! Without options, only the file system limits are checked (4096 bytes per name and 255 per
//! component), and an empty name is "No such file or directory". `-p` applies the POSIX
//! limits (256 bytes per name, 14 per component), requires the portable character set and
//! rejects empty names. `-P` rejects empty names and components that start with `-`.
//! `--portability` is `-p -P`. Nothing is looked up on disk.

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

const CMD: &str = "pathchk";

const SPECS: &[OptSpec] = &[
    OptSpec::flag("posix", Some('p'), None),
    OptSpec::flag("leading-dash", Some('P'), None),
    OptSpec::flag("portability", None, Some("portability")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: pathchk [OPTION]... NAME...\n\
Diagnose invalid or unportable file names.\n\n\
  -p                  check for most POSIX systems\n\
  -P                  check for empty names and leading \"-\"\n\
      --portability   check for all POSIX systems (equivalent to -p -P)\n\
      --help          display this help and exit\n\
      --version       output version information and exit\n\n\
Exit status: 0 if all file names passed, 1 if any failed.\n";

/// Limits of the host file system, used when no portability option is given.
const PATH_MAX: usize = 4096;
const NAME_MAX: usize = 255;
/// POSIX minimums used by `-p`.
const POSIX_PATH_MAX: usize = 256;
const POSIX_NAME_MAX: usize = 14;

/// Which checks a run applies.
#[derive(Clone, Copy, Default)]
struct Checks {
    posix_limits: bool,
    leading_dash: bool,
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    let mut checks = Checks::default();
    let mut names = Vec::new();
    for item in &items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(CMD),
            Item::Opt { id: "posix", .. } => checks.posix_limits = true,
            Item::Opt {
                id: "leading-dash", ..
            } => checks.leading_dash = true,
            Item::Opt {
                id: "portability", ..
            } => {
                checks.posix_limits = true;
                checks.leading_dash = true;
            }
            Item::Opt { .. } => {}
            Item::Operand(name) => names.push(name.clone()),
        }
    }
    if names.is_empty() {
        eprintln!("{CMD}: missing operand\nTry '{CMD} --help' for more information.");
        return EXIT_FAIL;
    }
    let mut status = EXIT_OK;
    for name in &names {
        if let Err(message) = check_name(name, checks) {
            eprintln!("{CMD}: {message}");
            status = EXIT_FAIL;
        }
    }
    status
}

/// The first problem found in `name`, as the message GNU prints (without the program name).
fn check_name(name: &str, checks: Checks) -> Result<(), String> {
    if name.is_empty() {
        return Err(if checks.posix_limits || checks.leading_dash {
            "empty file name".to_owned()
        } else {
            "'': No such file or directory".to_owned()
        });
    }
    if checks.posix_limits {
        check_posix(name)?;
    } else {
        check_host_limits(name)?;
    }
    if checks.leading_dash {
        check_leading_dash(name)?;
    }
    Ok(())
}

fn check_host_limits(name: &str) -> Result<(), String> {
    let too_long = name.len() > PATH_MAX || name.split('/').any(|part| part.len() > NAME_MAX);
    if too_long {
        return Err(format!("{name}: File name too long"));
    }
    Ok(())
}

fn check_posix(name: &str) -> Result<(), String> {
    if name.len() > POSIX_PATH_MAX {
        return Err(format!(
            "limit {POSIX_PATH_MAX} exceeded by length {} of file name '{name}'",
            name.len()
        ));
    }
    for component in name.split('/') {
        if component.len() > POSIX_NAME_MAX {
            return Err(format!(
                "limit {POSIX_NAME_MAX} exceeded by length {} of file name component '{component}'",
                component.len()
            ));
        }
    }
    match name.chars().find(|c| !is_portable(*c)) {
        Some(bad) => Err(format!(
            "non-portable character {} in file name '{name}'",
            quote_char(bad)
        )),
        None => Ok(()),
    }
}

fn check_leading_dash(name: &str) -> Result<(), String> {
    if name.split('/').any(|component| component.starts_with('-')) {
        return Err(format!("leading '-' in a component of file name '{name}'"));
    }
    Ok(())
}

/// The POSIX portable file name character set.
fn is_portable(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')
}

/// Renders a character for a message: as is when printable, otherwise as a 3-digit octal escape.
fn quote_char(c: char) -> String {
    if c.is_ascii_graphic() || c == ' ' {
        format!("'{c}'")
    } else {
        let mut bytes = [0u8; 4];
        let text = c.encode_utf8(&mut bytes);
        let octal: String = text.bytes().map(|b| format!("\\{b:03o}")).collect();
        format!("'{octal}'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: Checks = Checks {
        posix_limits: false,
        leading_dash: false,
    };
    const POSIX: Checks = Checks {
        posix_limits: true,
        leading_dash: false,
    };
    const LEADING: Checks = Checks {
        posix_limits: false,
        leading_dash: true,
    };

    #[test]
    fn empty_names_depend_on_the_mode() {
        assert_eq!(
            check_name("", DEFAULT).unwrap_err(),
            "'': No such file or directory"
        );
        assert_eq!(check_name("", POSIX).unwrap_err(), "empty file name");
        assert_eq!(check_name("", LEADING).unwrap_err(), "empty file name");
    }

    #[test]
    fn posix_limits_apply_to_names_and_components() {
        assert!(check_name("a-b_c.d/x", POSIX).is_ok());
        let long_component = "a".repeat(15);
        assert!(check_name(&long_component, POSIX)
            .unwrap_err()
            .contains("limit 14 exceeded"));
        let long_name = format!("{}/x", "a".repeat(14)).repeat(20);
        assert!(check_name(&long_name, POSIX)
            .unwrap_err()
            .contains("limit 256 exceeded"));
        assert!(check_name(&long_name, DEFAULT).is_ok());
    }

    #[test]
    fn portable_set_is_enforced_by_posix_mode_only() {
        assert!(check_name("a b", POSIX)
            .unwrap_err()
            .contains("non-portable character ' '"));
        assert!(check_name("a b", DEFAULT).is_ok());
    }

    #[test]
    fn leading_dash_is_checked_per_component() {
        assert!(check_name("dir/-abc", LEADING)
            .unwrap_err()
            .contains("leading '-'"));
        assert!(check_name("-abc", DEFAULT).is_ok());
        assert!(check_name("a-b/c", LEADING).is_ok());
    }

    #[test]
    fn host_limits_reject_overlong_components() {
        let component = "x".repeat(256);
        assert!(check_name(&component, DEFAULT)
            .unwrap_err()
            .contains("File name too long"));
        assert!(check_name(&"x".repeat(255), DEFAULT).is_ok());
    }

    #[test]
    fn non_printable_characters_are_escaped() {
        assert_eq!(quote_char('\u{1}'), "'\\001'");
        assert_eq!(quote_char('é'), "'\\303\\251'");
    }
}
