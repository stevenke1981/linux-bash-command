//! Extraction safety: a member may only be written where its path stays inside the
//! destination, even when earlier members created symbolic links on disk.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Why a member path was refused.
#[derive(Debug)]
pub enum Escape {
    /// A component on disk is a symbolic link resolving outside the destination, or a
    /// dangling link that could direct the write elsewhere.
    Link(PathBuf),
    /// The check itself failed.
    Io(io::Error),
}

/// Checks the components of `rel` below `dest`. Existing symbolic links are followed only
/// to confirm that they stay inside `dest`. The final component is checked only when
/// `include_last` is set (directory members); other final components are replaced, not
/// followed, by the caller.
pub fn check_inside(dest: &Path, rel: &Path, include_last: bool) -> Result<(), Escape> {
    let root = fs::canonicalize(dest).map_err(Escape::Io)?;
    let parts: Vec<&std::ffi::OsStr> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    let checked = if include_last || parts.is_empty() {
        parts.len()
    } else {
        parts.len() - 1
    };
    let mut current = dest.to_path_buf();
    for part in &parts[..checked] {
        current.push(part);
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(Escape::Io(error)),
        };
        if !meta.file_type().is_symlink() {
            continue;
        }
        match fs::canonicalize(&current) {
            Ok(real) if real.starts_with(&root) => {}
            _ => return Err(Escape::Link(current)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp dir, removed by the caller.
    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("agentcmd-tar-safety-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("dest")).unwrap();
        fs::create_dir_all(dir.join("outside")).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_outside_blocks_nested_members() {
        let base = scratch("outside");
        let dest = base.join("dest");
        std::os::unix::fs::symlink(base.join("outside"), dest.join("lnk")).unwrap();
        let result = check_inside(&dest, Path::new("lnk/file.txt"), false);
        assert!(matches!(result, Err(Escape::Link(_))));
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_inside_destination_is_allowed() {
        let base = scratch("inside");
        let dest = base.join("dest");
        fs::create_dir_all(dest.join("real")).unwrap();
        std::os::unix::fs::symlink(dest.join("real"), dest.join("alias")).unwrap();
        assert!(check_inside(&dest, Path::new("alias/file.txt"), false).is_ok());
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_refused() {
        let base = scratch("dangling");
        let dest = base.join("dest");
        std::os::unix::fs::symlink(base.join("missing"), dest.join("dang")).unwrap();
        assert!(matches!(
            check_inside(&dest, Path::new("dang/x"), false),
            Err(Escape::Link(_))
        ));
        fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn final_component_is_checked_only_for_directories() {
        let base = scratch("last");
        let dest = base.join("dest");
        std::os::unix::fs::symlink(base.join("outside"), dest.join("lnk")).unwrap();
        assert!(check_inside(&dest, Path::new("lnk"), false).is_ok());
        assert!(matches!(
            check_inside(&dest, Path::new("lnk"), true),
            Err(Escape::Link(_))
        ));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn plain_paths_pass() {
        let base = scratch("plain");
        let dest = base.join("dest");
        fs::create_dir_all(dest.join("a/b")).unwrap();
        assert!(check_inside(&dest, Path::new("a/b/c.txt"), false).is_ok());
        assert!(check_inside(&dest, Path::new("new/dir/file"), true).is_ok());
        fs::remove_dir_all(&base).unwrap();
    }
}
