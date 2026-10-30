//! Shared error reporting: GNU-style messages, exit status constants and OS error text.

use std::io;

/// Success.
pub const EXIT_OK: i32 = 0;
/// A general failure (missing input, partial failure, usage error for most commands).
pub const EXIT_FAIL: i32 = 1;
/// Serious trouble (usage error for `ls`, `grep`-family and `diff`-family commands).
pub const EXIT_TROUBLE: i32 = 2;

/// Maps an I/O error to GNU `strerror` wording, without Rust's " (os error N)" suffix.
pub fn io_error_text(error: &io::Error) -> String {
    if cfg!(windows) {
        if let Some(text) = error.raw_os_error().and_then(windows_code_text) {
            return text.to_string();
        }
    }
    if error.get_ref().is_some() {
        return error.to_string();
    }
    if cfg!(unix) && error.raw_os_error().is_some() {
        // The C library's strerror text is exactly what GNU prints.
        return strip_os_suffix(&error.to_string());
    }
    match kind_text(error.kind()) {
        Some(text) => text.to_string(),
        None => strip_os_suffix(&error.to_string()),
    }
}

/// Prints `cmd: operand: reason` to stderr.
pub fn report(cmd: &str, operand: &str, error: &io::Error) {
    eprintln!("{cmd}: {operand}: {}", io_error_text(error));
}

/// Prints `cmd (agentcmd) VERSION` to stdout and returns success.
pub fn print_version(cmd: &str) -> i32 {
    println!("{cmd} (agentcmd) {}", env!("CARGO_PKG_VERSION"));
    EXIT_OK
}

/// Reports a GNU option this project does not implement and returns the usage status 1.
pub fn unsupported(cmd: &str, opt: &str) -> i32 {
    eprintln!(
        "{cmd}: option '{opt}' is not supported by agentcmd\nTry '{cmd} --help' for more information."
    );
    EXIT_FAIL
}

fn kind_text(kind: io::ErrorKind) -> Option<&'static str> {
    Some(match kind {
        io::ErrorKind::NotFound => "No such file or directory",
        io::ErrorKind::PermissionDenied => "Permission denied",
        io::ErrorKind::AlreadyExists => "File exists",
        io::ErrorKind::IsADirectory => "Is a directory",
        io::ErrorKind::NotADirectory => "Not a directory",
        io::ErrorKind::DirectoryNotEmpty => "Directory not empty",
        io::ErrorKind::CrossesDevices => "Invalid cross-device link",
        io::ErrorKind::ReadOnlyFilesystem => "Read-only file system",
        io::ErrorKind::InvalidInput => "Invalid argument",
        _ => return None,
    })
}

/// Windows system error codes with a GNU equivalent. Only consulted on Windows, where the
/// same numbers mean different things on Unix.
fn windows_code_text(code: i32) -> Option<&'static str> {
    Some(match code {
        2 | 3 => "No such file or directory",
        5 | 32 => "Permission denied",
        80 | 183 => "File exists",
        145 => "Directory not empty",
        _ => return None,
    })
}

fn strip_os_suffix(text: &str) -> String {
    match text.find(" (os error") {
        Some(index) => text[..index].to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_messages_and_eperm_keep_their_text() {
        let custom = io::Error::new(io::ErrorKind::NotFound, "custom text");
        assert_eq!(io_error_text(&custom), "custom text");
        #[cfg(unix)]
        assert_eq!(
            io_error_text(&io::Error::from_raw_os_error(1)),
            "Operation not permitted"
        );
    }

    #[test]
    fn maps_kinds_to_gnu_wording() {
        let cases = [
            (io::ErrorKind::NotFound, "No such file or directory"),
            (io::ErrorKind::PermissionDenied, "Permission denied"),
            (io::ErrorKind::AlreadyExists, "File exists"),
            (io::ErrorKind::IsADirectory, "Is a directory"),
            (io::ErrorKind::NotADirectory, "Not a directory"),
            (io::ErrorKind::DirectoryNotEmpty, "Directory not empty"),
            (io::ErrorKind::CrossesDevices, "Invalid cross-device link"),
            (io::ErrorKind::ReadOnlyFilesystem, "Read-only file system"),
        ];
        for (kind, text) in cases {
            assert_eq!(io_error_text(&io::Error::from(kind)), text, "{kind:?}");
        }
    }

    #[test]
    fn strips_os_suffix_from_other_errors() {
        let error = io::Error::other("custom failure");
        assert_eq!(io_error_text(&error), "custom failure");
        assert_eq!(strip_os_suffix("boom (os error 99)"), "boom");
        assert_eq!(strip_os_suffix("plain"), "plain");
    }

    #[test]
    fn windows_codes_map_to_gnu_wording() {
        assert_eq!(windows_code_text(2), Some("No such file or directory"));
        assert_eq!(windows_code_text(3), Some("No such file or directory"));
        assert_eq!(windows_code_text(5), Some("Permission denied"));
        assert_eq!(windows_code_text(32), Some("Permission denied"));
        assert_eq!(windows_code_text(183), Some("File exists"));
        assert_eq!(windows_code_text(80), Some("File exists"));
        assert_eq!(windows_code_text(145), Some("Directory not empty"));
        assert_eq!(windows_code_text(9999), None);
    }

    #[test]
    fn exit_constants_match_gnu_convention() {
        assert_eq!((EXIT_OK, EXIT_FAIL, EXIT_TROUBLE), (0, 1, 2));
    }

    #[test]
    fn version_and_unsupported_return_status() {
        assert_eq!(print_version("demo"), 0);
        assert_eq!(unsupported("demo", "--weird"), 1);
    }
}
