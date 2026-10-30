//! Refusal rules for recursive removal: `.` and `..` names, and filesystem roots
//! (`/`, drive roots such as `C:/` and `C:\`, and UNC share roots such as `//server/share`).
//!
//! These are pure string checks, identical on every platform, so they can be unit-tested
//! everywhere without touching the filesystem.

use std::path::Path;

/// Both separators on every platform: these checks are string-only and must not depend on
/// the host (a `C:\` root is refused on Linux too).
fn is_any_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Why a path must not be removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The last component is `.` or `..`.
    DotOrDotDot,
    /// The path is a filesystem root (protected by `--preserve-root`).
    Root,
}

/// Returns the refusal that applies to `path`, if any. Dot names are checked first.
pub fn refuse_reason(path: &Path) -> Option<Refusal> {
    let text = path.to_string_lossy();
    if is_dot_name(&text) {
        Some(Refusal::DotOrDotDot)
    } else if is_root(&text) {
        Some(Refusal::Root)
    } else {
        None
    }
}

/// True when the last component of `text` is `.` or `..` (`a/..`, `./` and `.` all count).
pub fn is_dot_name(text: &str) -> bool {
    let trimmed = text.trim_end_matches(is_any_sep);
    matches!(trimmed.rsplit(is_any_sep).next(), Some("." | ".."))
}

/// True for a filesystem root in any of the forms listed in the module comment.
pub fn is_root(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    if text.chars().all(is_any_sep) {
        return true;
    }
    if is_drive_root(text) {
        return true;
    }
    is_unc_share_root(text)
}

/// `X:` followed only by separators, such as `C:`, `C:/` or `C:\`.
fn is_drive_root(text: &str) -> bool {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic() => chars.all(is_any_sep),
        _ => false,
    }
}

/// `//server/share` (or with `\`) followed only by separators.
fn is_unc_share_root(text: &str) -> bool {
    let mut chars = text.chars();
    let starts_unc = matches!((chars.next(), chars.next()), (Some(a), Some(b)) if is_any_sep(a) && is_any_sep(b));
    if !starts_unc {
        return false;
    }
    let rest = &text[2..];
    let parts: Vec<&str> = rest.split(is_any_sep).collect();
    parts.len() >= 2
        && !parts[0].is_empty()
        && !parts[1].is_empty()
        && parts[2..].iter().all(|part| part.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unix_root_is_refused() {
        assert_eq!(refuse_reason(Path::new("/")), Some(Refusal::Root));
        assert_eq!(refuse_reason(Path::new("//")), Some(Refusal::Root));
    }

    #[test]
    fn drive_roots_are_refused() {
        assert_eq!(refuse_reason(Path::new("C:/")), Some(Refusal::Root));
        assert_eq!(refuse_reason(Path::new("C:\\")), Some(Refusal::Root));
        assert_eq!(refuse_reason(Path::new("d:")), Some(Refusal::Root));
    }

    #[test]
    fn unc_share_roots_are_refused() {
        assert_eq!(
            refuse_reason(Path::new("//server/share")),
            Some(Refusal::Root)
        );
        assert_eq!(
            refuse_reason(Path::new("//server/share/")),
            Some(Refusal::Root)
        );
        assert_eq!(
            refuse_reason(Path::new("\\\\server\\share")),
            Some(Refusal::Root)
        );
    }

    #[test]
    fn dot_and_dot_dot_are_refused() {
        assert_eq!(refuse_reason(Path::new(".")), Some(Refusal::DotOrDotDot));
        assert_eq!(refuse_reason(Path::new("..")), Some(Refusal::DotOrDotDot));
        assert_eq!(refuse_reason(Path::new("a/..")), Some(Refusal::DotOrDotDot));
        assert_eq!(refuse_reason(Path::new("a/.")), Some(Refusal::DotOrDotDot));
    }

    #[test]
    fn ordinary_paths_are_allowed() {
        for text in [
            "tmp",
            "a/b",
            "C:/x",
            "//server/share/dir",
            "/tmp/.hidden",
            "..x",
        ] {
            assert_eq!(refuse_reason(&PathBuf::from(text)), None, "{text}");
        }
    }

    #[test]
    fn dot_check_applies_before_root_check() {
        assert!(is_dot_name("/."));
        assert!(!is_root("/."));
    }
}
