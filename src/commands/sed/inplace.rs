//! In-place editing (`-i`). Output goes to a temporary file in the target's directory,
//! which replaces the original only after it is complete, so a failure leaves the
//! original intact.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::output::Output;
use crate::common::path::strip_verbatim;

/// An open temporary file standing in for one input file.
pub struct InPlace {
    pub output: Output,
    temp: PathBuf,
    target: PathBuf,
}

/// Starts editing `name`. Without `follow`, a symbolic link is replaced by a regular
/// file (GNU behaviour); with it, the link's target is edited.
pub fn start(name: &str, follow: bool) -> io::Result<InPlace> {
    let target = if follow {
        // Windows canonical paths carry a `\\?\` prefix that is not shown to users.
        strip_verbatim(&fs::canonicalize(name)?)
    } else {
        PathBuf::from(name)
    };
    let dir = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let (file, temp) = create_temp(&dir)?;
    Ok(InPlace {
        output: Output::new(Box::new(BufWriter::new(file))),
        temp,
        target,
    })
}

/// Finishes an edit: copies the mode, optionally keeps a backup named by `suffix`,
/// and renames the temporary file over the target.
pub fn commit(edit: InPlace, suffix: Option<&str>) -> io::Result<()> {
    let InPlace {
        mut output,
        temp,
        target,
    } = edit;
    let result = output
        .flush()
        .and_then(|()| drop_output(output))
        .and_then(|()| fs::set_permissions(&temp, fs::metadata(&target)?.permissions()))
        .and_then(|()| match suffix {
            Some(suffix) if !suffix.is_empty() => keep_backup(&target, suffix),
            _ => Ok(()),
        })
        .and_then(|()| fs::rename(&temp, &target));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn drop_output(output: Output) -> io::Result<()> {
    drop(output);
    Ok(())
}

fn keep_backup(target: &Path, suffix: &str) -> io::Result<()> {
    let backup = backup_path(target, suffix);
    #[cfg(windows)]
    let _ = fs::remove_file(&backup);
    fs::rename(target, backup)
}

/// The backup name for `target`: each `*` in `suffix` becomes the file's base name,
/// otherwise the suffix is appended. A bare name goes next to the target.
pub fn backup_path(target: &Path, suffix: &str) -> PathBuf {
    let base = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = if suffix.contains('*') {
        suffix.replace('*', &base)
    } else {
        format!("{base}{suffix}")
    };
    let path = PathBuf::from(&name);
    if name.contains('/') || path.is_absolute() {
        return path;
    }
    match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => path,
    }
}

/// Creates a new file in `dir` with a name nobody else is using.
fn create_temp(dir: &Path) -> io::Result<(File, PathBuf)> {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0);
    for attempt in 0..100u32 {
        let path = dir.join(format!("sed{pid:x}{nanos:x}{attempt:x}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free temporary file name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentcmd-sed-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn backup_names_follow_the_suffix_rules() {
        let target = Path::new("/data/notes.txt");
        assert_eq!(
            backup_path(target, ".bak"),
            PathBuf::from("/data/notes.txt.bak")
        );
        assert_eq!(
            backup_path(target, "old_*"),
            PathBuf::from("/data/old_notes.txt")
        );
        assert_eq!(
            backup_path(target, "/tmp/*.orig"),
            PathBuf::from("/tmp/notes.txt.orig")
        );
        assert_eq!(
            backup_path(Path::new("plain"), "~"),
            PathBuf::from("plain~")
        );
    }

    #[test]
    fn commit_replaces_the_file_and_keeps_a_backup() {
        let dir = sandbox("inplace-commit");
        let target = dir.join("data.txt");
        fs::write(&target, b"old\n").unwrap();
        let mut edit = start(&target.to_string_lossy(), false).unwrap();
        edit.output.text(b"new\n").unwrap();
        commit(edit, Some(".bak")).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new\n");
        assert_eq!(fs::read(dir.join("data.txt.bak")).unwrap(), b"old\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn commit_without_suffix_leaves_no_backup() {
        let dir = sandbox("inplace-nobackup");
        let target = dir.join("data.txt");
        fs::write(&target, b"x\n").unwrap();
        let edit = start(&target.to_string_lossy(), false).unwrap();
        commit(edit, None).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dropped_edit_keeps_the_original() {
        let dir = sandbox("inplace-drop");
        let target = dir.join("data.txt");
        fs::write(&target, b"keep\n").unwrap();
        {
            let mut edit = start(&target.to_string_lossy(), false).unwrap();
            edit.output.text(b"discard\n").unwrap();
        }
        assert_eq!(fs::read(&target).unwrap(), b"keep\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn commit_keeps_the_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = sandbox("inplace-mode");
        let target = dir.join("script.sh");
        fs::write(&target, b"echo\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o750)).unwrap();
        let edit = start(&target.to_string_lossy(), false).unwrap();
        commit(edit, None).unwrap();
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o750);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn follow_edits_the_link_target() {
        let dir = sandbox("inplace-follow");
        let target = dir.join("real.txt");
        let link = dir.join("link.txt");
        fs::write(&target, b"a\n").unwrap();
        if std::os::unix::fs::symlink(&target, &link).is_err() {
            fs::remove_dir_all(&dir).unwrap();
            return;
        }
        let mut edit = start(&link.to_string_lossy(), true).unwrap();
        edit.output.text(b"b\n").unwrap();
        commit(edit, None).unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&target).unwrap(), b"b\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
