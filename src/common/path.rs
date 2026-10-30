use std::path::{Path, PathBuf};

/// Formats a path for output. On Windows, separators become `/` so results can be
/// pasted back into Bash (Git Bash, MSYS2) without backslash escaping.
pub fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

/// Removes the Windows verbatim prefix that `canonicalize` adds: `\\?\C:\x` becomes
/// `C:\x`, and `\\?\UNC\server\share\x` becomes `//server/share/x`. Other paths, and
/// paths that are not valid UTF-8 when a prefix must be removed, are returned unchanged.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!("//{}", rest.replace('\\', "/")));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_relative_paths() {
        assert_eq!(display_path(&Path::new("src").join("bin")), "src/bin");
    }

    #[test]
    fn strips_disk_verbatim_prefix() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\work\file.txt")),
            PathBuf::from(r"C:\work\file.txt")
        );
    }

    #[test]
    fn turns_unc_verbatim_prefix_into_double_slash() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\server\share\dir\f")),
            PathBuf::from("//server/share/dir/f")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\server\share")),
            PathBuf::from("//server/share")
        );
    }

    #[test]
    fn leaves_ordinary_paths_alone() {
        assert_eq!(
            strip_verbatim(Path::new(r"C:\plain\path")),
            PathBuf::from(r"C:\plain\path")
        );
        assert_eq!(
            strip_verbatim(Path::new("/usr/bin/env")),
            PathBuf::from("/usr/bin/env")
        );
        assert_eq!(strip_verbatim(Path::new("rel/x")), PathBuf::from("rel/x"));
    }
}
