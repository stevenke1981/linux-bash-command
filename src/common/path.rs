/// Formats a path for output. On Windows, separators become `/` so results can be
/// pasted back into Bash (Git Bash, MSYS2) without backslash escaping.
pub fn display_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        let normalized = text.replace('\\', "/");
        if let Some(unc) = normalized.strip_prefix("//?/UNC/") {
            format!("//{unc}")
        } else {
            normalized
                .strip_prefix("//?/")
                .unwrap_or(&normalized)
                .to_owned()
        }
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn strips_extended_windows_prefixes() {
        assert_eq!(
            super::display_path(std::path::Path::new(r"\\?\C:\dir")),
            "C:/dir"
        );
        assert_eq!(
            super::display_path(std::path::Path::new(r"\\?\UNC\server\share")),
            "//server/share"
        );
    }

    #[test]
    fn displays_relative_paths() {
        assert_eq!(
            super::display_path(&std::path::Path::new("src").join("bin")),
            "src/bin"
        );
    }
}
