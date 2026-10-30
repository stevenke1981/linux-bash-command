//! File side effects of a patch run: `.rej` reject files, backups and atomic replacement
//! of the target file.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::parse::{FilePatch, Hunk, Kind};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Text of a reject file: the file's `---`/`+++` lines followed by the given hunks in
/// unified form.
pub fn reject_text(file: &FilePatch, hunks: &[&Hunk]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(file.old_line.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(file.new_line.as_bytes());
    out.push(b'\n');
    for hunk in hunks {
        out.extend_from_slice(hunk.header.as_bytes());
        out.push(b'\n');
        for line in &hunk.lines {
            out.push(match line.kind {
                Kind::Context => b' ',
                Kind::Delete => b'-',
                Kind::Add => b'+',
            });
            out.extend_from_slice(&line.text);
            out.push(b'\n');
            if !line.newline {
                out.extend_from_slice(b"\\ No newline at end of file\n");
            }
        }
    }
    out
}

/// Writes reject text to `path`, replacing the file unless `append` is set.
pub fn write_rejects(path: &Path, bytes: &[u8], append: bool) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(path)?;
    file.write_all(bytes)
}

/// Copies `target` to `target + suffix`, replacing an older backup.
pub fn backup(target: &Path, suffix: &str) -> io::Result<()> {
    let mut name = target.as_os_str().to_owned();
    name.push(suffix);
    fs::copy(target, name).map(|_| ())
}

/// Replaces `target` with `data`: the bytes go to a temporary file in the same directory,
/// which takes the original permissions and is then renamed over the target. A failed
/// write leaves the original untouched.
pub fn replace_file(target: &Path, data: &[u8]) -> io::Result<()> {
    let dir = match target.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => Path::new(".").to_path_buf(),
    };
    let base = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "patch".to_string());
    let temp = dir.join(format!(
        ".{base}.agentcmd-patch-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(data)?;
        drop(file);
        if let Ok(meta) = fs::metadata(target) {
            fs::set_permissions(&temp, meta.permissions())?;
        }
        fs::rename(&temp, target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::patch::parse::parse;

    fn sandbox(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentcmd-patch-reject-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reject_text_repeats_headers_and_hunks() {
        let text = b"--- t\n+++ t\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+B\n";
        let file = parse(text).unwrap().remove(0);
        let refs: Vec<&Hunk> = file.hunks.iter().collect();
        assert_eq!(reject_text(&file, &refs), text.to_vec());
    }

    #[test]
    fn write_rejects_truncates_or_appends() {
        let dir = sandbox("rej");
        let path = dir.join("x.rej");
        write_rejects(&path, b"one\n", false).unwrap();
        write_rejects(&path, b"two\n", true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"one\ntwo\n");
        write_rejects(&path, b"three\n", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"three\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn backup_copies_with_suffix() {
        let dir = sandbox("bak");
        let target = dir.join("t.txt");
        fs::write(&target, b"orig").unwrap();
        backup(&target, ".orig").unwrap();
        assert_eq!(fs::read(dir.join("t.txt.orig")).unwrap(), b"orig");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replace_file_swaps_contents_and_leaves_no_temp() {
        let dir = sandbox("replace");
        let target = dir.join("t.txt");
        fs::write(&target, b"old").unwrap();
        replace_file(&target, b"new\n").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new\n");
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["t.txt".to_string()]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replace_file_creates_new_targets() {
        let dir = sandbox("create");
        let target = dir.join("fresh.txt");
        replace_file(&target, b"hello\n").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"hello\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
