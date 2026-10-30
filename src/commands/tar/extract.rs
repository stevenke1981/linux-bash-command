//! `tar -x`: restores members under the destination directory.
//!
//! Safety rules (see `names` and `safety`): names with `..` are refused; a leading `/` is
//! removed unless `-P`; and no member is written through a symbolic link that leads outside
//! the destination. Existing final components are replaced rather than followed.

use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use filetime::FileTime;
use tar::{Archive, Entry, EntryType};

use super::archive_io;
use super::names::{self, NameError};
use super::options::{Operand, Options, CMD};
use super::safety::{self, Escape};
use crate::common::error::{io_error_text, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::stdout_writer;
use crate::common::path::display_path;

/// Mode bits removed from archived permissions when `-p` is not given.
const DEFAULT_UMASK: u32 = 0o022;

struct Extractor<'a> {
    opts: &'a Options,
    dest: PathBuf,
    failed: bool,
    warned_root: bool,
    stdout: std::io::BufWriter<std::io::Stdout>,
}

/// Runs `-x`. Returns the process exit status.
pub fn run(opts: &Options, operands: &[Operand]) -> i32 {
    let dest = PathBuf::from(opts.extract_dir.as_deref().unwrap_or("."));
    if !opts.to_stdout && !dest.is_dir() {
        eprintln!(
            "{CMD}: {}: Cannot chdir: {}",
            display_path(&dest),
            if dest.exists() {
                "Not a directory"
            } else {
                "No such file or directory"
            }
        );
        return EXIT_TROUBLE;
    }
    let reader = match archive_io::open_source(opts.archive.as_deref()) {
        Ok(reader) => reader,
        Err(error) => return fatal_open(opts, &error),
    };
    let mut archive = Archive::new(reader);
    let entries = match archive.entries() {
        Ok(entries) => entries,
        Err(error) => return fatal_read(&error, true),
    };
    let mut extractor = Extractor {
        opts,
        dest,
        failed: false,
        warned_root: false,
        stdout: stdout_writer(),
    };
    let wanted: Vec<String> = operands.iter().map(|o| o.name.clone()).collect();
    for (index, entry) in entries.enumerate() {
        match entry {
            Ok(entry) => extractor.handle(entry, &wanted),
            Err(error) => {
                let code = fatal_read(&error, index == 0);
                let _ = extractor.stdout_flush();
                return code;
            }
        }
    }
    let _ = extractor.stdout_flush();
    if extractor.failed {
        eprintln!("{CMD}: Exiting with failure status due to previous errors");
        EXIT_TROUBLE
    } else {
        EXIT_OK
    }
}

fn fatal_open(opts: &Options, error: &io::Error) -> i32 {
    let name = opts.archive.as_deref().unwrap_or("-");
    eprintln!("{CMD}: {name}: Cannot open: {}", io_error_text(error));
    eprintln!("{CMD}: Error is not recoverable: exiting now");
    EXIT_TROUBLE
}

/// Maps a read error to GNU's wording. A damaged first header means the input is not a
/// tar archive; a short read later on means the archive was truncated.
pub fn fatal_read(error: &io::Error, first_member: bool) -> i32 {
    let text = if first_member {
        "This does not look like a tar archive".to_string()
    } else if error.kind() == io::ErrorKind::UnexpectedEof {
        "Unexpected EOF in archive".to_string()
    } else {
        io_error_text(error)
    };
    eprintln!("{CMD}: {text}");
    eprintln!("{CMD}: Error is not recoverable: exiting now");
    EXIT_TROUBLE
}

impl Extractor<'_> {
    fn stdout_flush(&mut self) -> io::Result<()> {
        use std::io::Write;
        self.stdout.flush()
    }

    fn handle<R: Read>(&mut self, mut entry: Entry<'_, R>, wanted: &[String]) {
        let name = entry.path_bytes().into_owned();
        let shown = String::from_utf8_lossy(&name).into_owned();
        if !names::selected(&name, wanted) || names::excluded(&name, &self.opts.excludes) {
            return;
        }
        let sanitized = match names::sanitize(&name) {
            Ok(sanitized) => sanitized,
            Err(NameError::ParentDir) => {
                eprintln!("{CMD}: {shown}: Member name contains '..'");
                self.failed = true;
                return;
            }
        };
        if sanitized.had_root && !self.opts.absolute_names {
            self.warn_root();
        }
        let Some(rel) = names::strip_components(&sanitized.rel, self.opts.strip) else {
            return;
        };
        if rel.as_os_str().is_empty() {
            return;
        }
        let target = match (&sanitized.absolute, self.opts.absolute_names) {
            (Some(_), true) => Path::new("/").join(&rel),
            _ => self.dest.join(&rel),
        };
        let kind = entry.header().entry_type();
        if self.opts.to_stdout {
            if matches!(
                kind,
                EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse
            ) {
                self.verbose(&shown, true);
                self.write_to_stdout(&mut entry, &shown);
            }
            return;
        }
        self.verbose(&shown, false);
        let absolute = self.opts.absolute_names;
        match kind {
            EntryType::Directory => self.make_directory(&shown, &target, &rel, absolute, &entry),
            EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse => {
                self.write_file(&shown, &target, &rel, absolute, &mut entry)
            }
            EntryType::Symlink => self.make_symlink(&shown, &target, &rel, absolute, &entry),
            EntryType::Link => self.make_hardlink(&shown, &target, &rel, absolute, &entry),
            EntryType::Char | EntryType::Block | EntryType::Fifo => {
                self.fail(
                    &shown,
                    "Cannot mknod",
                    &io::Error::from(io::ErrorKind::Unsupported),
                );
            }
            _ => {}
        }
    }

    fn warn_root(&mut self) {
        if !self.warned_root {
            eprintln!("{CMD}: Removing leading '/' from member names");
            self.warned_root = true;
        }
    }

    fn verbose(&mut self, shown: &str, to_stdout_mode: bool) {
        if !self.opts.verbose {
            return;
        }
        // With -O the names go to stderr so they do not mix with the file data.
        if to_stdout_mode {
            eprintln!("{shown}");
        } else {
            use std::io::Write;
            let _ = writeln!(self.stdout, "{shown}");
        }
    }

    fn write_to_stdout<R: Read>(&mut self, entry: &mut Entry<'_, R>, shown: &str) {
        use std::io::Write;
        if let Err(error) = io::copy(entry, &mut self.stdout) {
            self.fail(shown, "Error writing to stdout", &error);
        }
        let _ = self.stdout.flush();
    }

    fn fail(&mut self, shown: &str, what: &str, error: &io::Error) {
        eprintln!("{CMD}: {shown}: {what}: {}", io_error_text(error));
        self.failed = true;
    }

    /// Refuses the member when a symlink on the way leads outside the destination.
    fn check_path(&mut self, shown: &str, rel: &Path, absolute: bool, include_last: bool) -> bool {
        if absolute {
            return true;
        }
        match safety::check_inside(&self.dest, rel, include_last) {
            Ok(()) => true,
            Err(Escape::Link(link)) => {
                eprintln!(
                    "{CMD}: {shown}: Cannot open: Symbolic link escapes the destination ({})",
                    display_path(&link)
                );
                self.failed = true;
                false
            }
            Err(Escape::Io(error)) => {
                self.fail(shown, "Cannot open", &error);
                false
            }
        }
    }

    fn make_parents(&mut self, shown: &str, target: &Path) -> bool {
        match target.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => match fs::create_dir_all(parent) {
                Ok(()) => true,
                Err(error) => {
                    self.fail(shown, "Cannot mkdir", &error);
                    false
                }
            },
            _ => true,
        }
    }

    fn make_directory<R: Read>(
        &mut self,
        shown: &str,
        target: &Path,
        rel: &Path,
        absolute: bool,
        entry: &Entry<'_, R>,
    ) {
        if !self.check_path(shown, rel, absolute, true) {
            return;
        }
        if let Err(error) = fs::create_dir_all(target) {
            return self.fail(shown, "Cannot mkdir", &error);
        }
        let mode = entry.header().mode().unwrap_or(0o755);
        self.apply_mode(target, mode);
    }

    fn write_file<R: Read>(
        &mut self,
        shown: &str,
        target: &Path,
        rel: &Path,
        absolute: bool,
        entry: &mut Entry<'_, R>,
    ) {
        if !self.check_path(shown, rel, absolute, false) || !self.make_parents(shown, target) {
            return;
        }
        if let Ok(meta) = fs::symlink_metadata(target) {
            if self.opts.keep_old {
                return self.fail(
                    shown,
                    "Cannot open",
                    &io::Error::from(io::ErrorKind::AlreadyExists),
                );
            }
            if meta.is_dir() {
                return self.fail(
                    shown,
                    "Cannot open",
                    &io::Error::from(io::ErrorKind::IsADirectory),
                );
            }
            // Removing a symlink replaces it; the link is never followed.
            if let Err(error) = fs::remove_file(target) {
                return self.fail(shown, "Cannot unlink", &error);
            }
        }
        let mut file = match OpenOptions::new().write(true).create_new(true).open(target) {
            Ok(file) => file,
            Err(error) => return self.fail(shown, "Cannot open", &error),
        };
        if let Err(error) = io::copy(entry, &mut file) {
            return self.fail(shown, "Cannot write", &error);
        }
        drop(file);
        let mode = entry.header().mode().unwrap_or(0o644);
        self.apply_mode(target, mode);
        self.apply_mtime(target, entry);
    }

    fn make_symlink<R: Read>(
        &mut self,
        shown: &str,
        target: &Path,
        rel: &Path,
        absolute: bool,
        entry: &Entry<'_, R>,
    ) {
        if !self.check_path(shown, rel, absolute, false) || !self.make_parents(shown, target) {
            return;
        }
        let link = match entry.link_name() {
            Ok(Some(link)) => link.into_owned(),
            _ => {
                return self.fail(
                    shown,
                    "Cannot create symlink",
                    &io::Error::from(io::ErrorKind::InvalidInput),
                )
            }
        };
        if fs::symlink_metadata(target).is_ok() {
            if self.opts.keep_old {
                return self.fail(
                    shown,
                    "Cannot create symlink",
                    &io::Error::from(io::ErrorKind::AlreadyExists),
                );
            }
            if let Err(error) = remove_any(target) {
                return self.fail(shown, "Cannot unlink", &error);
            }
        }
        if let Err(error) = create_symlink(&link, target) {
            self.fail(shown, "Cannot create symlink", &error);
        }
    }

    fn make_hardlink<R: Read>(
        &mut self,
        shown: &str,
        target: &Path,
        rel: &Path,
        absolute: bool,
        entry: &Entry<'_, R>,
    ) {
        if !self.check_path(shown, rel, absolute, false) || !self.make_parents(shown, target) {
            return;
        }
        let Some(link_bytes) = entry.link_name_bytes() else {
            return self.fail(
                shown,
                "Cannot hard link",
                &io::Error::from(io::ErrorKind::InvalidInput),
            );
        };
        let source = match names::sanitize(&link_bytes) {
            Ok(sanitized) => sanitized,
            Err(NameError::ParentDir) => {
                eprintln!("{CMD}: {shown}: Member name contains '..'");
                self.failed = true;
                return;
            }
        };
        let Some(source_rel) = names::strip_components(&source.rel, self.opts.strip) else {
            return;
        };
        let source_path = self.dest.join(&source_rel);
        if !self.check_path(shown, &source_rel, absolute, true) {
            return;
        }
        if fs::symlink_metadata(target).is_ok() {
            if self.opts.keep_old {
                return self.fail(
                    shown,
                    "Cannot hard link",
                    &io::Error::from(io::ErrorKind::AlreadyExists),
                );
            }
            if let Err(error) = remove_any(target) {
                return self.fail(shown, "Cannot unlink", &error);
            }
        }
        if let Err(error) = fs::hard_link(&source_path, target) {
            self.fail(shown, "Cannot hard link", &error);
        }
    }

    fn apply_mode(&mut self, target: &Path, mode: u32) {
        let bits = if self.opts.preserve_permissions {
            mode & 0o7777
        } else {
            mode & 0o777 & !DEFAULT_UMASK
        };
        set_mode(target, bits);
    }

    fn apply_mtime<R: Read>(&mut self, target: &Path, entry: &Entry<'_, R>) {
        if let Ok(mtime) = entry.header().mtime() {
            let time = FileTime::from_unix_time(i64::try_from(mtime).unwrap_or(0), 0);
            let _ = filetime::set_file_mtime(target, time);
        }
    }
}

/// Removes a file, symlink or empty directory at `path` without following links.
fn remove_any(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(unix)]
fn create_symlink(link: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(link, target)
}

#[cfg(windows)]
fn create_symlink(link: &Path, target: &Path) -> io::Result<()> {
    let base = target.parent().unwrap_or(Path::new("."));
    let result = if base.join(link).is_dir() {
        std::os::windows::fs::symlink_dir(link, target)
    } else {
        std::os::windows::fs::symlink_file(link, target)
    };
    result.map_err(|error| {
        if error.kind() == io::ErrorKind::PermissionDenied {
            io::Error::new(
                error.kind(),
                format!(
                    "{} (creating symbolic links on Windows requires Developer Mode or an elevated shell)",
                    io_error_text(&error)
                ),
            )
        } else {
            error
        }
    })
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(path: &Path, mode: u32) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_readonly(mode & 0o200 == 0);
        let _ = fs::set_permissions(path, perms);
    }
}
