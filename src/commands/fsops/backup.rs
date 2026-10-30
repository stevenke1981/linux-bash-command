//! Backup naming for `cp`, `mv` and `ln` (`-b`, `--backup=CONTROL`, `-S SUFFIX`).
//!
//! Simple backups append a suffix (default `~`). Numbered backups use `name.~N~`, with N one
//! more than the highest existing number. `existing` picks numbered when a numbered backup
//! already exists and simple otherwise.

use std::fs;
use std::path::{Path, PathBuf};

use super::helpers::{argmatch, ArgGroup};

/// The backup method selected by `--backup` or `VERSION_CONTROL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    None,
    Numbered,
    Existing,
    Simple,
}

/// Spellings accepted by `--backup=CONTROL`, in GNU's listing order.
pub const CONTROL_GROUPS: &[ArgGroup<'_, Control>] = &[
    (&["none", "off"], Control::None),
    (&["numbered", "t"], Control::Numbered),
    (&["existing", "nil"], Control::Existing),
    (&["simple", "never"], Control::Simple),
];

/// Parses a `--backup` argument.
pub fn parse_control(text: &str) -> Result<Control, String> {
    argmatch("--backup", text, CONTROL_GROUPS)
}

/// The method from the `VERSION_CONTROL` environment variable, or `None` when unset.
pub fn control_from_env() -> Control {
    match std::env::var("VERSION_CONTROL") {
        Ok(text) if !text.is_empty() => parse_control(&text).unwrap_or(Control::Existing),
        _ => Control::None,
    }
}

/// The suffix for simple backups: `-S`, else `SIMPLE_BACKUP_SUFFIX`, else `~`.
pub fn default_suffix() -> String {
    std::env::var("SIMPLE_BACKUP_SUFFIX").unwrap_or_else(|_| "~".to_string())
}

/// Name a backup of `path` takes under `control`. `None` returns `path` unchanged.
pub fn backup_name(path: &Path, control: Control, suffix: &str) -> PathBuf {
    let numbered = match control {
        Control::None => return path.to_path_buf(),
        Control::Numbered => true,
        Control::Existing => numbered_exists(path),
        Control::Simple => false,
    };
    if numbered {
        next_numbered(path)
    } else {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    }
}

fn numbered_name(path: &Path, number: u32) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".~{number}~"));
    PathBuf::from(name)
}

fn numbered_exists(path: &Path) -> bool {
    numbered_name(path, 1).symlink_metadata().is_ok()
}

/// Highest existing `name.~N~` plus one, scanning the parent directory.
fn next_numbered(path: &Path) -> PathBuf {
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let prefix = format!("{base}.~");
    let mut highest = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(number) = name
                .strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix('~'))
                .and_then(|digits| digits.parse::<u32>().ok())
            {
                highest = highest.max(number);
            }
        }
    }
    numbered_name(path, highest + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_control_spellings_and_prefixes() {
        assert_eq!(parse_control("none"), Ok(Control::None));
        assert_eq!(parse_control("off"), Ok(Control::None));
        assert_eq!(parse_control("t"), Ok(Control::Numbered));
        assert_eq!(parse_control("nev"), Ok(Control::Simple));
        assert!(parse_control("bogus").is_err());
    }

    #[test]
    fn simple_backup_appends_suffix() {
        let path = Path::new("dir/file.txt");
        assert_eq!(
            backup_name(path, Control::Simple, "~"),
            PathBuf::from("dir/file.txt~")
        );
        assert_eq!(
            backup_name(path, Control::Simple, ".bak"),
            PathBuf::from("dir/file.txt.bak")
        );
    }

    #[test]
    fn none_control_keeps_the_path() {
        let path = Path::new("x");
        assert_eq!(backup_name(path, Control::None, "~"), PathBuf::from("x"));
    }

    #[test]
    fn numbered_backups_count_up_from_existing_ones() {
        let dir = std::env::temp_dir().join(format!("agentcmd-backup-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("f");
        fs::write(&target, b"v").unwrap();
        assert_eq!(
            backup_name(&target, Control::Numbered, "~"),
            dir.join("f.~1~")
        );
        fs::write(dir.join("f.~1~"), b"old").unwrap();
        fs::write(dir.join("f.~7~"), b"older").unwrap();
        assert_eq!(
            backup_name(&target, Control::Numbered, "~"),
            dir.join("f.~8~")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_control_uses_simple_until_a_numbered_backup_exists() {
        let dir = std::env::temp_dir().join(format!("agentcmd-existing-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("g");
        fs::write(&target, b"v").unwrap();
        assert_eq!(backup_name(&target, Control::Existing, "~"), dir.join("g~"));
        fs::write(dir.join("g.~1~"), b"old").unwrap();
        assert_eq!(
            backup_name(&target, Control::Existing, "~"),
            dir.join("g.~2~")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
