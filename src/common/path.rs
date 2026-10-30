/// Formats a path for output. On Windows, separators become `/` so results can be
/// pasted back into Bash (Git Bash, MSYS2) without backslash escaping.
pub fn display_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn displays_relative_paths() {
        assert_eq!(
            super::display_path(&std::path::Path::new("src").join("bin")),
            "src/bin"
        );
    }
}
