use std::io;

pub fn io_text(error: &io::Error) -> String {
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return "Permission denied".into();
    }
    match error.kind() {
        io::ErrorKind::NotFound => "No such file or directory".into(),
        io::ErrorKind::PermissionDenied => "Permission denied".into(),
        io::ErrorKind::AlreadyExists => "File exists".into(),
        io::ErrorKind::IsADirectory => "Is a directory".into(),
        io::ErrorKind::NotADirectory => "Not a directory".into(),
        io::ErrorKind::DirectoryNotEmpty => "Directory not empty".into(),
        io::ErrorKind::CrossesDevices => "Invalid cross-device link".into(),
        io::ErrorKind::ReadOnlyFilesystem => "Read-only file system".into(),
        io::ErrorKind::InvalidInput => "Invalid argument".into(),
        _ => error
            .to_string()
            .split(" (os error")
            .next()
            .unwrap_or("")
            .into(),
    }
}

pub fn usage(command: &str, message: impl std::fmt::Display, code: i32) -> i32 {
    eprintln!("{command}: {message}\nTry '{command} --help' for more information.");
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing() {
        assert_eq!(
            io_text(&io::Error::from(io::ErrorKind::NotFound)),
            "No such file or directory"
        );
    }
    #[test]
    fn denied() {
        assert_eq!(
            io_text(&io::Error::from(io::ErrorKind::PermissionDenied)),
            "Permission denied"
        );
    }
    #[test]
    fn exists() {
        assert_eq!(
            io_text(&io::Error::from(io::ErrorKind::AlreadyExists)),
            "File exists"
        );
    }
    #[test]
    fn directory() {
        assert_eq!(
            io_text(&io::Error::from(io::ErrorKind::IsADirectory)),
            "Is a directory"
        );
    }
    #[test]
    fn custom() {
        assert_eq!(io_text(&io::Error::other("custom")), "custom");
    }
}
