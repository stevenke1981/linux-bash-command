//! `tar -c`: writes the operands, and the directories below them, as a tar archive.

use std::fs::{self, File, Metadata};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tar::{Builder, EntryType, Header};

use super::archive_io::{self, Counting};
use super::names;
use super::options::{Operand, Options, CMD};
use crate::common::error::{io_error_text, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::is_broken_pipe;
use crate::common::path::display_path;

type Out = Counting<archive_io::Sink>;

/// Mutable state for one create run.
struct Creator<'a> {
    opts: &'a Options,
    builder: Builder<Out>,
    failed: bool,
    /// Where `-v` names go: stdout, unless the archive itself is on stdout.
    verbose_to_stderr: bool,
    warned_root: bool,
}

/// Runs `-c`. Returns the process exit status.
pub fn run(opts: &Options, operands: &[Operand]) -> i32 {
    let gzip = opts.gzip || (opts.auto_compress && archive_io::auto_gzip(opts.archive.as_deref()));
    let sink = match archive_io::open_sink(opts.archive.as_deref(), gzip) {
        Ok(sink) => sink,
        Err(error) => {
            let name = opts.archive.as_deref().unwrap_or("-");
            eprintln!("{CMD}: {name}: Cannot open: {}", io_error_text(&error));
            return EXIT_TROUBLE;
        }
    };
    let archive_on_stdout = matches!(opts.archive.as_deref(), None | Some("-"));
    let mut creator = Creator {
        opts,
        builder: Builder::new(Counting::new(sink)),
        failed: false,
        verbose_to_stderr: archive_on_stdout,
        warned_root: false,
    };
    for operand in operands {
        creator.add_operand(operand);
    }
    creator.finish()
}

impl Creator<'_> {
    fn add_operand(&mut self, operand: &Operand) {
        let stored = self.stored_name(&operand.name);
        let fs_path = match &operand.chdir {
            Some(dir) => Path::new(dir).join(&operand.name),
            None => PathBuf::from(&operand.name),
        };
        self.add_path(&stored, &fs_path);
    }

    /// The name recorded in the archive: leading slashes are removed unless `-P`.
    fn stored_name(&mut self, name: &str) -> String {
        if self.opts.absolute_names {
            return name.to_string();
        }
        let trimmed = name.trim_start_matches('/');
        if trimmed.len() != name.len() && !self.warned_root {
            eprintln!("{CMD}: Removing leading '/' from member names");
            self.warned_root = true;
        }
        trimmed.to_string()
    }

    fn add_path(&mut self, stored: &str, fs_path: &Path) {
        if names::excluded(stored.as_bytes(), &self.opts.excludes) {
            return;
        }
        let meta = if self.opts.dereference {
            fs::metadata(fs_path)
        } else {
            fs::symlink_metadata(fs_path)
        };
        let meta = match meta {
            Ok(meta) => meta,
            Err(error) => return self.fail(stored, "Cannot stat", &error),
        };
        if meta.is_dir() {
            self.add_directory(stored, fs_path, &meta);
        } else if meta.file_type().is_symlink() {
            self.add_symlink(stored, fs_path);
        } else if meta.is_file() {
            self.add_file(stored, fs_path, &meta);
        } else {
            eprintln!(
                "{CMD}: {stored}: file is neither a regular file, directory nor symlink; ignored"
            );
            self.failed = true;
        }
    }

    fn add_directory(&mut self, stored: &str, fs_path: &Path, meta: &Metadata) {
        let base = stored.trim_end_matches('/');
        let shown = if base.is_empty() {
            "/".to_string()
        } else {
            format!("{base}/")
        };
        let mut header = Header::new_gnu();
        fill_header(&mut header, meta, EntryType::Directory, 0);
        if let Err(error) = self.builder.append_data(&mut header, &shown, io::empty()) {
            return self.fail(&shown, "Cannot add", &error);
        }
        self.verbose(&shown);
        // Directory order, as GNU tar uses; the archive is therefore not sorted.
        let children: Vec<_> = match fs::read_dir(fs_path) {
            Ok(entries) => entries.filter_map(Result::ok).collect(),
            Err(error) => return self.fail(&shown, "Cannot open", &error),
        };
        for child in children {
            let child_name = child.file_name().to_string_lossy().into_owned();
            let child_stored = if base.is_empty() {
                child_name
            } else {
                format!("{base}/{child_name}")
            };
            self.add_path(&child_stored, &fs_path.join(child.file_name()));
        }
    }

    fn add_file(&mut self, stored: &str, fs_path: &Path, meta: &Metadata) {
        let file = match File::open(fs_path) {
            Ok(file) => file,
            Err(error) => return self.fail(stored, "Cannot open", &error),
        };
        let mut header = Header::new_gnu();
        fill_header(&mut header, meta, EntryType::Regular, meta.len());
        match self.builder.append_data(&mut header, stored, file) {
            Ok(()) => self.verbose(stored),
            Err(error) => self.fail(stored, "Cannot add", &error),
        }
    }

    fn add_symlink(&mut self, stored: &str, fs_path: &Path) {
        let target = match fs::read_link(fs_path) {
            Ok(target) => target,
            Err(error) => return self.fail(stored, "Cannot read link", &error),
        };
        let mut header = Header::new_gnu();
        let meta = match fs::symlink_metadata(fs_path) {
            Ok(meta) => meta,
            Err(error) => return self.fail(stored, "Cannot stat", &error),
        };
        fill_header(&mut header, &meta, EntryType::Symlink, 0);
        match self.builder.append_link(&mut header, stored, &target) {
            Ok(()) => self.verbose(stored),
            Err(error) => self.fail(stored, "Cannot add", &error),
        }
    }

    fn fail(&mut self, name: &str, what: &str, error: &io::Error) {
        eprintln!("{CMD}: {name}: {what}: {}", io_error_text(error));
        self.failed = true;
    }

    fn verbose(&mut self, name: &str) {
        if !self.opts.verbose {
            return;
        }
        let shown = display_path(Path::new(name));
        if self.verbose_to_stderr {
            eprintln!("{shown}");
        } else {
            println!("{shown}");
        }
    }

    /// Writes the end-of-archive blocks, pads to a full record and flushes.
    fn finish(self) -> i32 {
        let failed = self.failed;
        let mut counted = match self.builder.into_inner() {
            Ok(counted) => counted,
            Err(error) => return report_write_error(&error),
        };
        let padding = archive_io::record_padding(counted.written);
        if padding > 0 {
            if let Err(error) = counted.write_all(&vec![0u8; padding as usize]) {
                return report_write_error(&error);
            }
        }
        if let Err(error) = counted.into_inner().finish() {
            return report_write_error(&error);
        }
        if failed {
            eprintln!("{CMD}: Exiting with failure status due to previous errors");
            EXIT_TROUBLE
        } else {
            EXIT_OK
        }
    }
}

fn report_write_error(error: &io::Error) -> i32 {
    if is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{CMD}: write error: {}", io_error_text(error));
    EXIT_TROUBLE
}

/// Copies the attributes the archive records from `meta` into `header`.
fn fill_header(header: &mut Header, meta: &Metadata, kind: EntryType, size: u64) {
    header.set_entry_type(kind);
    header.set_size(size);
    let (mode, uid, gid, mtime) = attributes(meta, kind);
    header.set_mode(mode);
    header.set_uid(uid);
    header.set_gid(gid);
    header.set_mtime(mtime);
    let (user, group) = owner_names(meta);
    if let Some(user) = user {
        let _ = header.set_username(&user);
    }
    if let Some(group) = group {
        let _ = header.set_groupname(&group);
    }
}

/// User and group names for the header, looked up from the account database on Unix.
#[cfg(unix)]
fn owner_names(meta: &Metadata) -> (Option<String>, Option<String>) {
    use std::os::unix::fs::MetadataExt;
    (lookup_user(meta.uid()), lookup_group(meta.gid()))
}

#[cfg(not(unix))]
fn owner_names(_meta: &Metadata) -> (Option<String>, Option<String>) {
    (None, None)
}

#[cfg(unix)]
fn lookup_user(uid: u32) -> Option<String> {
    // SAFETY: getpwuid returns a pointer into static storage; the name is copied out at once.
    unsafe {
        let entry = libc::getpwuid(uid as libc::uid_t);
        if entry.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*entry).pw_name)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(unix)]
fn lookup_group(gid: u32) -> Option<String> {
    // SAFETY: getgrgid returns a pointer into static storage; the name is copied out at once.
    unsafe {
        let entry = libc::getgrgid(gid as libc::gid_t);
        if entry.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*entry).gr_name)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(unix)]
fn attributes(meta: &Metadata, _kind: EntryType) -> (u32, u64, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let mode = meta.mode() & 0o7777;
    let mtime = u64::try_from(meta.mtime()).unwrap_or(0);
    (mode, u64::from(meta.uid()), u64::from(meta.gid()), mtime)
}

#[cfg(not(unix))]
fn attributes(meta: &Metadata, kind: EntryType) -> (u32, u64, u64, u64) {
    let mode = match kind {
        EntryType::Directory => 0o755,
        _ if meta.permissions().readonly() => 0o444,
        _ => 0o644,
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (mode, 0, 0, mtime)
}
