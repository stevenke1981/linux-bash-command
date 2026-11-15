use crate::common::{
    args::{self, Item, OptSpec},
    error, escape,
};
use std::path::{Path, PathBuf};

const HELP: &str = "Usage: pathchk [OPTION]... NAME...\nDiagnose invalid or unportable file names.\n  -p                  check for all POSIX systems\n  -P                  check for empty names and leading '-'\n      --portability   equivalent to -p -P\n      --help          display this help\n      --version       output version information\n";

#[derive(Clone, Copy, Debug, Default)]
struct Checks {
    posix: bool,
    leading_dash: bool,
}

fn quoted(name: &str) -> String {
    String::from_utf8_lossy(&escape::quote_name(name.as_bytes())).into_owned()
}

fn filesystem_limits(name: &str) -> (usize, usize) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let input = Path::new(name);
        let mut prefix = if input.is_absolute() {
            Path::new("/").to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
        };
        for component in input.components() {
            match component {
                std::path::Component::RootDir => prefix = Path::new("/").to_path_buf(),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => prefix.push(".."),
                std::path::Component::Normal(part) => prefix.push(part),
                _ => {}
            }
            if !prefix.exists() {
                prefix.pop();
                break;
            }
        }
        let bytes = prefix.as_os_str().as_bytes();
        let prefix_c = std::ffi::CString::new(bytes).unwrap_or_default();
        let name_max = unsafe { libc::pathconf(prefix_c.as_ptr(), libc::_PC_NAME_MAX) };
        let path_max = unsafe { libc::pathconf(prefix_c.as_ptr(), libc::_PC_PATH_MAX) };
        let name_max = if name_max > 0 { name_max as usize } else { 255 };
        let path_max = if path_max > 0 {
            path_max as usize
        } else {
            4096
        };
        (name_max, path_max)
    }
    #[cfg(windows)]
    {
        let _ = name;
        (255, 32_767)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = name;
        (255, 4096)
    }
}

fn components(name: &str) -> impl Iterator<Item = &[u8]> {
    name.as_bytes()
        .split(|byte| *byte == b'/')
        .filter(|part| !part.is_empty())
}

fn filesystem_components(name: &str) -> impl Iterator<Item = &[u8]> {
    #[cfg(windows)]
    let separator = |byte: &u8| matches!(*byte, b'/' | b'\\');
    #[cfg(not(windows))]
    let separator = |byte: &u8| *byte == b'/';
    name.as_bytes()
        .split(separator)
        .filter(|part| !part.is_empty())
}

fn non_directory_prefix(name: &str) -> Option<String> {
    let parts = Path::new(name).components().collect::<Vec<_>>();
    let mut prefix = PathBuf::new();
    for (index, part) in parts.iter().enumerate() {
        prefix.push(part.as_os_str());
        if index + 1 == parts.len() {
            continue;
        }
        match std::fs::metadata(&prefix) {
            Ok(metadata) if !metadata.is_dir() => {
                return Some(format!("{}: Not a directory", quoted(name)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {
                return Some(format!("{}: Not a directory", quoted(name)));
            }
            _ => {}
        }
    }
    None
}

fn violation(name: &str, checks: Checks) -> Option<String> {
    let bytes = name.as_bytes();
    if name.is_empty() {
        return Some(if checks.leading_dash || checks.posix {
            "empty file name".into()
        } else {
            format!("{}: No such file or directory", quoted(name))
        });
    }
    if checks.leading_dash {
        for component in components(name) {
            if component.first() == Some(&b'-') {
                return Some(format!(
                    "leading '-' in a component of file name {}",
                    quoted(name)
                ));
            }
        }
    }
    if checks.posix {
        for component in components(name) {
            if let Some(byte) = component
                .iter()
                .copied()
                .find(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')))
            {
                let ch = if byte.is_ascii_graphic() {
                    (byte as char).to_string()
                } else {
                    format!("\\{:03o}", byte)
                };
                return Some(format!(
                    "nonportable character '{}' in file name {}",
                    ch,
                    quoted(name)
                ));
            }
            if component.len() > 14 {
                return Some(format!(
                    "limit 14 exceeded by length {} of file name component '{}'",
                    component.len(),
                    String::from_utf8_lossy(component)
                ));
            }
        }
        if bytes.len() >= 256 {
            return Some(format!(
                "limit 256 exceeded by length {} of file name",
                bytes.len()
            ));
        }
        return None;
    }
    let (name_max, path_max) = filesystem_limits(name);
    for component in filesystem_components(name) {
        if component.len() > name_max {
            return Some(format!("{}: file name too long", quoted(name)));
        }
    }
    if bytes.len() >= path_max {
        return Some(format!("{}: file name too long", quoted(name)));
    }
    if !checks.posix {
        return non_directory_prefix(name);
    }
    None
}

pub fn run(args: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("portability", '\0'),
        OptSpec {
            id: "-p",
            short: 'p',
            long: "",
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "-P",
            short: 'P',
            long: "",
            has_arg: args::HasArg::No,
        },
    ];
    let items = match args::parse_command("pathchk", args, HELP, &specs, true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let mut checks = Checks::default();
    let mut names = Vec::new();
    for item in items {
        match item {
            Item::Opt("portability", _) => {
                checks = Checks {
                    posix: true,
                    leading_dash: true,
                }
            }
            Item::Opt("-p", _) => checks.posix = true,
            Item::Opt("-P", _) => checks.leading_dash = true,
            Item::Operand(name) => names.push(name),
            Item::Opt(_, _) => unreachable!(),
        }
    }
    if names.is_empty() {
        return error::usage("pathchk", "missing operand", 1);
    }
    let mut failed = false;
    for name in names {
        if let Some(reason) = violation(&name, checks) {
            eprintln!("pathchk: {reason}");
            failed = true;
        }
    }
    if failed {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_portable_names_and_rejects_nonportable_bytes() {
        let checks = Checks {
            posix: true,
            leading_dash: false,
        };
        assert_eq!(violation("dir/file_1.txt", checks), None);
        assert!(violation("dir/file@1", checks)
            .unwrap()
            .contains("nonportable character"));
    }

    #[test]
    fn enforces_posix_component_and_path_limits() {
        let checks = Checks {
            posix: true,
            leading_dash: false,
        };
        assert!(violation("123456789012345", checks)
            .unwrap()
            .contains("limit 14"));
        let max_path = ("1234567890123/".repeat(18)) + "abc";
        let too_long_path = ("1234567890123/".repeat(18)) + "abcd";
        assert_eq!(max_path.len(), 255);
        assert_eq!(too_long_path.len(), 256);
        assert_eq!(violation(&max_path, checks), None);
        assert!(violation(&too_long_path, checks)
            .unwrap()
            .contains("limit 256"));
    }

    #[test]
    fn portability_mode_checks_empty_and_hyphen_components() {
        let checks = Checks {
            posix: false,
            leading_dash: true,
        };
        assert_eq!(violation("", checks).as_deref(), Some("empty file name"));
        assert!(violation("dir/-name", checks)
            .unwrap()
            .contains("leading '-'"));
        assert_eq!(violation("dir/name", checks), None);
    }

    #[test]
    fn default_checks_components_against_filesystem_limit() {
        let checks = Checks::default();
        assert_eq!(violation("a/b", checks), None);
        assert!(violation(&"x".repeat(300), checks).is_some());
        assert!(violation("", checks)
            .unwrap()
            .contains("No such file or directory"));
    }
}
