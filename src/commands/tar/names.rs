//! Member-name rules for `tar`: sanitising names read from an archive, stripping leading
//! components, selecting members named on the command line, and `--exclude` matching.
//!
//! Everything here is a pure function of bytes so it can be tested with crafted names.

use std::path::PathBuf;

/// Why a member name was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum NameError {
    /// The name has a `..` component that could leave the extraction directory.
    ParentDir,
}

/// A member name reduced to the components that may be used under the destination.
#[derive(Debug, PartialEq, Eq)]
pub struct Sanitized {
    /// Relative path made of normal components only.
    pub rel: PathBuf,
    /// The name had a leading root (or, on Windows, a drive prefix).
    pub had_root: bool,
    /// The name as an absolute path, used only with `-P`.
    pub absolute: Option<PathBuf>,
}

/// Converts raw name bytes into a path. On Unix the bytes are used as they are; on
/// Windows invalid UTF-8 is replaced.
pub fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(bytes.to_vec()))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn is_separator(byte: u8) -> bool {
    byte == b'/' || (cfg!(windows) && byte == b'\\')
}

/// Splits a member name into components. `..` is an error. A root or (on Windows) a
/// drive prefix is reported through `had_root` and never kept in `rel`.
pub fn sanitize(name: &[u8]) -> Result<Sanitized, NameError> {
    let mut rel = PathBuf::new();
    let mut had_root = false;
    let mut absolute_parts: Vec<&[u8]> = Vec::new();
    let leading = name.first().is_some_and(|&b| is_separator(b));
    if leading {
        had_root = true;
    }
    for (index, part) in name.split(|&b| is_separator(b)).enumerate() {
        if part.is_empty() || part == b"." {
            continue;
        }
        if part == b".." {
            return Err(NameError::ParentDir);
        }
        if cfg!(windows) && index == 0 && part.contains(&b':') {
            had_root = true;
            continue;
        }
        absolute_parts.push(part);
        rel.push(bytes_to_path(part));
    }
    let absolute = leading.then(|| {
        let mut full = PathBuf::from("/");
        for part in &absolute_parts {
            full.push(bytes_to_path(part));
        }
        full
    });
    Ok(Sanitized {
        rel,
        had_root,
        absolute,
    })
}

/// Drops the first `count` components. Returns `None` when fewer remain, in which case the
/// member is skipped, as GNU tar does.
pub fn strip_components(rel: &std::path::Path, count: usize) -> Option<PathBuf> {
    if count == 0 {
        return Some(rel.to_path_buf());
    }
    let parts: Vec<_> = rel.components().collect();
    if parts.len() <= count {
        return None;
    }
    Some(parts[count..].iter().collect())
}

/// Removes trailing slashes, so `dir/` and `dir` compare equal.
pub fn trim_slashes(name: &[u8]) -> &[u8] {
    let mut end = name.len();
    while end > 0 && name[end - 1] == b'/' {
        end -= 1;
    }
    &name[..end]
}

/// True when `name` is one of the named operands or lies below one of them.
pub fn selected(name: &[u8], operands: &[String]) -> bool {
    if operands.is_empty() {
        return true;
    }
    let name = trim_slashes(name);
    operands.iter().any(|operand| {
        let wanted = trim_slashes(operand.as_bytes());
        name == wanted
            || (name.len() > wanted.len() && name.starts_with(wanted) && name[wanted.len()] == b'/')
    })
}

/// True when any pattern matches the whole name or any trailing run of components,
/// matching GNU's `--exclude` behaviour.
pub fn excluded(name: &[u8], patterns: &[String]) -> bool {
    let name = trim_slashes(name);
    patterns.iter().any(|pattern| {
        let pattern = pattern.as_bytes();
        if glob_match(pattern, name) {
            return true;
        }
        name.iter()
            .enumerate()
            .any(|(index, &byte)| byte == b'/' && glob_match(pattern, &name[index + 1..]))
    })
}

/// Shell-style matching with `*`, `?`, bracket classes and backslash escapes.
pub fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() {
            match pattern[p] {
                b'*' => {
                    star = Some((p, t));
                    p += 1;
                    continue;
                }
                b'?' => {
                    p += 1;
                    t += 1;
                    continue;
                }
                b'[' => {
                    if let Some((matched, next)) = bracket_match(pattern, p, text[t]) {
                        if matched {
                            p = next;
                            t += 1;
                            continue;
                        }
                    } else if text[t] == b'[' {
                        p += 1;
                        t += 1;
                        continue;
                    }
                }
                b'\\' if p + 1 < pattern.len() => {
                    if pattern[p + 1] == text[t] {
                        p += 2;
                        t += 1;
                        continue;
                    }
                }
                literal if literal == text[t] => {
                    p += 1;
                    t += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((star_p, star_t)) => {
                p = star_p + 1;
                t = star_t + 1;
                star = Some((star_p, star_t + 1));
            }
            None => return false,
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// Matches one bracket expression starting at `pattern[start] == '['`. Returns whether
/// `byte` is in the class and the index after the class, or `None` when the class is not
/// closed (then `[` is an ordinary character).
fn bracket_match(pattern: &[u8], start: usize, byte: u8) -> Option<(bool, usize)> {
    let mut index = start + 1;
    let negate = matches!(pattern.get(index), Some(b'!' | b'^'));
    if negate {
        index += 1;
    }
    let mut matched = false;
    let mut first = true;
    while index < pattern.len() {
        if pattern[index] == b']' && !first {
            return Some((matched != negate, index + 1));
        }
        first = false;
        let low = pattern[index];
        if index + 2 < pattern.len() && pattern[index + 1] == b'-' && pattern[index + 2] != b']' {
            let high = pattern[index + 2];
            if (low..=high).contains(&byte) {
                matched = true;
            }
            index += 3;
        } else {
            if low == byte {
                matched = true;
            }
            index += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_names_lose_their_root_and_keep_the_rest() {
        let got = sanitize(b"/etc/passwd").unwrap();
        assert!(got.had_root);
        assert_eq!(got.rel, PathBuf::from("etc/passwd"));
        assert_eq!(got.absolute, Some(PathBuf::from("/etc/passwd")));
    }

    #[test]
    fn parent_directory_components_are_refused() {
        assert_eq!(sanitize(b"../escape"), Err(NameError::ParentDir));
        assert_eq!(sanitize(b"ok/../../escape"), Err(NameError::ParentDir));
        assert_eq!(
            sanitize(b"ok/..x/file").unwrap().rel,
            PathBuf::from("ok/..x/file")
        );
    }

    #[test]
    fn dot_and_empty_components_are_dropped() {
        let got = sanitize(b"./a//./b/").unwrap();
        assert!(!got.had_root);
        assert_eq!(got.rel, PathBuf::from("a/b"));
        assert_eq!(got.absolute, None);
    }

    #[test]
    fn strip_components_counts_normal_parts() {
        assert_eq!(
            strip_components(std::path::Path::new("a/b/c"), 1),
            Some(PathBuf::from("b/c"))
        );
        assert_eq!(strip_components(std::path::Path::new("a/b"), 2), None);
        assert_eq!(
            strip_components(std::path::Path::new("a"), 0),
            Some(PathBuf::from("a"))
        );
    }

    #[test]
    fn named_operands_select_themselves_and_children() {
        let wanted = vec!["src".to_string()];
        assert!(selected(b"src", &wanted));
        assert!(selected(b"src/a.txt", &wanted));
        assert!(selected(b"src/", &wanted));
        assert!(!selected(b"srcs/a.txt", &wanted));
        assert!(selected(b"anything", &[]));
    }

    #[test]
    fn glob_handles_star_question_and_classes() {
        assert!(glob_match(b"*.txt", b"notes.txt"));
        assert!(!glob_match(b"*.txt", b"notes.md"));
        assert!(glob_match(b"a?c", b"abc"));
        assert!(glob_match(b"[a-c]x", b"bx"));
        assert!(!glob_match(b"[!a-c]x", b"bx"));
        assert!(glob_match(b"\\*", b"*"));
        assert!(glob_match(b"a*b*c", b"aXXbYYc"));
    }

    #[test]
    fn exclude_matches_any_trailing_component_run() {
        let patterns = vec!["*.o".to_string()];
        assert!(excluded(b"build/main.o", &patterns));
        assert!(excluded(b"main.o", &patterns));
        assert!(!excluded(b"build/main.c", &patterns));
        let dirs = vec!["build".to_string()];
        assert!(excluded(b"src/build", &dirs));
        assert!(excluded(b"build/", &dirs));
    }
}
