/// Formats a path for output. On Windows, separators become `/` so results can be
/// pasted back into Bash (Git Bash, MSYS2) without backslash escaping.
pub fn display_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    #[cfg(windows)]
    {
        normalize_windows(&text)
    }
    #[cfg(not(windows))]
    {
        text
    }
}

#[cfg(any(windows, test))]
fn normalize_windows(text: &str) -> String {
    let normalized = text.replace('\\', "/");
    if let Some(unc) = normalized.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else {
        normalized
            .strip_prefix("//?/")
            .unwrap_or(&normalized)
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn strips_extended_windows_prefixes() {
        assert_eq!(super::normalize_windows(r"\\?\C:\dir"), "C:/dir");
        assert_eq!(
            super::normalize_windows(r"\\?\UNC\server\share"),
            "//server/share"
        );
        assert_eq!(super::normalize_windows(r"C:\dir\file"), "C:/dir/file");
        assert_eq!(super::normalize_windows("relative/file"), "relative/file");
    }

    #[test]
    fn displays_relative_paths() {
        assert_eq!(
            super::display_path(&std::path::Path::new("src").join("bin")),
            "src/bin"
        );
    }
}
