//! Copy engine behind `cp` and the cross-device fallback of `mv`: regular files, symbolic
//! links, directory trees, backups, hard-link preservation and attribute copying.
//!
//! Errors are printed as they happen, prefixed with the command name, and the walk goes on
//! to the next entry (GNU behaviour). Each call returns whether everything succeeded.

use std::collections::HashMap;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::common::path::display_path;

use super::backup::{self, Control};
use super::helpers::{self, ask, os_text, say};

/// Which symbolic links are followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deref {
    /// `-L`: follow every link.
    Always,
    /// `-H`: follow links named on the command line only.
    CommandLine,
    /// `-P`: never follow; copy links as links.
    Never,
}

/// What `--update` does when the destination exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    All,
    None,
    NoneFail,
    Older,
}

/// Attributes copied from the source (`--preserve`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Preserve {
    pub mode: bool,
    pub ownership: bool,
    pub timestamps: bool,
    pub links: bool,
}

/// Settings for one copy run.
pub struct Options {
    pub cmd: &'static str,
    pub recursive: bool,
    pub deref: Deref,
    pub preserve: Preserve,
    pub hard_link: bool,
    pub symbolic: bool,
    pub force: bool,
    pub remove_dest: bool,
    pub no_clobber: bool,
    pub interactive: bool,
    pub update: Update,
    pub backup: Control,
    pub suffix: String,
    pub verbose: bool,
}

/// Copies sources to destinations under one set of [`Options`].
pub struct Copier {
    pub opts: Options,
    /// First destination written for each source inode, for `--preserve=links`.
    seen: HashMap<(u64, u64), PathBuf>,
}

/// Outcome of looking at an existing destination.
enum Existing {
    /// Overwrite it.
    Proceed,
    /// Leave it alone and report success.
    Skip,
    /// Leave it alone and report failure (already printed, or declined).
    Refuse,
}

impl Copier {
    pub fn new(opts: Options) -> Self {
        Self {
            opts,
            seen: HashMap::new(),
        }
    }

    fn fail(&self, text: &str) {
        eprintln!("{}: {text}", self.opts.cmd);
    }

    /// Copies one operand to `dst`, following the `-R`/`-L`/`-H`/`-P` policy.
    pub fn copy_top(&mut self, src: &Path, dst: &Path) -> bool {
        self.copy_path(src, dst, true)
    }

    fn follows(&self, top: bool) -> bool {
        match self.opts.deref {
            Deref::Always => true,
            Deref::CommandLine => top,
            Deref::Never => false,
        }
    }

    fn copy_path(&mut self, src: &Path, dst: &Path, top: bool) -> bool {
        let follow = self.follows(top);
        let meta = match stat(src, follow) {
            Ok(meta) => meta,
            Err(error) => {
                self.fail(&format!(
                    "cannot stat '{}': {}",
                    display_path(src),
                    os_text(&error)
                ));
                return false;
            }
        };
        if meta.is_dir() {
            self.copy_dir(src, dst, &meta)
        } else {
            self.copy_nondir(src, dst, &meta)
        }
    }

    fn copy_dir(&mut self, src: &Path, dst: &Path, meta: &Metadata) -> bool {
        let (src_text, dst_text) = (display_path(src), display_path(dst));
        if !self.opts.recursive {
            self.fail(&format!(
                "-r not specified; omitting directory '{src_text}'"
            ));
            return false;
        }
        if into_itself(src, dst) {
            // GNU has already created the destination when it finds the loop.
            if fs::symlink_metadata(dst).is_err() {
                let _ = create_dir(dst, meta);
            }
            self.fail(&format!(
                "cannot copy a directory, '{src_text}', into itself, '{dst_text}'"
            ));
            return false;
        }
        match fs::metadata(dst) {
            Ok(existing) if existing.is_dir() => {}
            Ok(_) => {
                self.fail(&format!(
                    "cannot overwrite non-directory '{dst_text}' with directory '{src_text}'"
                ));
                return false;
            }
            Err(_) => {
                if let Err(error) = create_dir(dst, meta) {
                    self.fail(&format!(
                        "cannot create directory '{dst_text}': {}",
                        os_text(&error)
                    ));
                    return false;
                }
                if self.opts.verbose {
                    say(&format!("'{src_text}' -> '{dst_text}'"));
                }
            }
        }
        let mut ok = true;
        match fs::read_dir(src) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let child_dst = dst.join(entry.file_name());
                            let copied = self.copy_path(&entry.path(), &child_dst, false);
                            ok &= copied;
                        }
                        Err(error) => {
                            self.fail(&format!(
                                "cannot read directory '{src_text}': {}",
                                os_text(&error)
                            ));
                            ok = false;
                        }
                    }
                }
            }
            Err(error) => {
                self.fail(&format!(
                    "cannot open '{src_text}' for reading: {}",
                    os_text(&error)
                ));
                return false;
            }
        }
        self.apply_attrs(meta, dst);
        ok
    }

    fn copy_nondir(&mut self, src: &Path, dst: &Path, meta: &Metadata) -> bool {
        let (src_text, dst_text) = (display_path(src), display_path(dst));
        let existing = fs::metadata(dst).ok();
        let dst_exists = existing.is_some() || fs::symlink_metadata(dst).is_ok();
        if let Some(found) = &existing {
            if found.is_dir() {
                self.fail(&format!(
                    "cannot overwrite directory '{dst_text}' with non-directory"
                ));
                return false;
            }
        }
        if dst_exists {
            if !self.opts.remove_dest && helpers::same_file(src, dst) {
                self.fail(&format!("'{src_text}' and '{dst_text}' are the same file"));
                return false;
            }
            match self.existing_policy(dst, meta, existing.as_ref()) {
                Existing::Proceed => {}
                Existing::Skip => return true,
                Existing::Refuse => return false,
            }
            if let Some(code) = self.clear_destination(dst) {
                self.fail(&code);
                return false;
            }
        }
        let result = if self.opts.hard_link {
            make_hard_link(src, dst)
        } else if self.opts.symbolic {
            make_symbolic(src, dst)
        } else if meta.file_type().is_symlink() {
            recreate_symlink(src, dst)
        } else {
            self.copy_regular(src, dst, meta)
        };
        match result {
            Ok(()) => {
                if self.opts.verbose {
                    say(&format!("'{src_text}' -> '{dst_text}'"));
                }
                true
            }
            Err(message) => {
                self.fail(&message);
                false
            }
        }
    }

    /// Decides what to do with an existing destination.
    fn existing_policy(
        &self,
        dst: &Path,
        meta: &Metadata,
        existing: Option<&Metadata>,
    ) -> Existing {
        let dst_text = display_path(dst);
        if self.opts.no_clobber {
            return Existing::Skip;
        }
        let newer_src = match (meta.modified(), existing.map(Metadata::modified)) {
            (Ok(src_time), Some(Ok(dst_time))) => src_time > dst_time,
            _ => true,
        };
        match self.opts.update {
            Update::None => return Existing::Skip,
            Update::NoneFail => {
                self.fail(&format!("not replacing '{dst_text}'"));
                return Existing::Refuse;
            }
            Update::Older if !newer_src => return Existing::Skip,
            Update::Older | Update::All => {}
        }
        if self.opts.interactive && !ask(&format!("{}: overwrite '{dst_text}'? ", self.opts.cmd)) {
            return Existing::Refuse;
        }
        Existing::Proceed
    }

    /// Removes or backs up an existing destination when `-b`, `--remove-destination` or
    /// `-f` asks for it. Returns an error message on failure.
    fn clear_destination(&self, dst: &Path) -> Option<String> {
        let dst_text = display_path(dst);
        if self.opts.backup != Control::None {
            let suffix = self.opts.suffix.as_str();
            let target = backup::backup_name(dst, self.opts.backup, suffix);
            return fs::rename(dst, &target)
                .err()
                .map(|error| format!("cannot backup '{dst_text}': {}", os_text(&error)));
        }
        if self.opts.remove_dest || self.opts.force {
            if let Err(error) = fs::remove_file(dst) {
                return Some(format!("cannot remove '{dst_text}': {}", os_text(&error)));
            }
        }
        None
    }

    /// Copies a regular file's contents, keeping hard links when `--preserve=links` is set.
    fn copy_regular(&mut self, src: &Path, dst: &Path, meta: &Metadata) -> Result<(), String> {
        let key =
            helpers::file_key(meta).filter(|_| self.opts.preserve.links && link_count(meta) > 1);
        if let Some(key) = key {
            if let Some(first) = self.seen.get(&key).cloned() {
                let _ = fs::remove_file(dst);
                return fs::hard_link(&first, dst).map_err(|error| {
                    format!(
                        "cannot create hard link '{}' to '{}': {}",
                        display_path(dst),
                        display_path(&first),
                        os_text(&error)
                    )
                });
            }
            self.seen.insert(key, dst.to_path_buf());
        }
        write_contents(src, dst, meta, self.opts.force)?;
        self.apply_attrs(meta, dst);
        Ok(())
    }

    /// Copies the attributes selected by `--preserve` from `meta` onto `dst`. Failures are
    /// ignored, as GNU does for ownership the caller may not change.
    fn apply_attrs(&self, meta: &Metadata, dst: &Path) {
        let preserve = self.opts.preserve;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if preserve.ownership {
                let _ = std::os::unix::fs::chown(dst, Some(meta.uid()), Some(meta.gid()));
            }
            if preserve.mode {
                let _ = fs::set_permissions(dst, fs::Permissions::from_mode(meta.mode() & 0o7777));
            }
        }
        #[cfg(not(unix))]
        {
            if preserve.mode {
                if let Ok(current) = fs::metadata(dst) {
                    let mut perms = current.permissions();
                    perms.set_readonly(meta.permissions().readonly());
                    let _ = fs::set_permissions(dst, perms);
                }
            }
        }
        if preserve.timestamps {
            let atime = filetime::FileTime::from_last_access_time(meta);
            let mtime = filetime::FileTime::from_last_modification_time(meta);
            let _ = filetime::set_file_times(dst, atime, mtime);
        }
    }
}

fn stat(path: &Path, follow: bool) -> io::Result<Metadata> {
    if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
}

/// Number of names the file has (1 where the platform does not say).
#[cfg(unix)]
fn link_count(meta: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.nlink()
}

#[cfg(not(unix))]
fn link_count(_meta: &Metadata) -> u64 {
    1
}

/// True when `dst` lies inside `src`, which would copy a tree into itself.
pub fn into_itself(src: &Path, dst: &Path) -> bool {
    let Ok(src_abs) = fs::canonicalize(src) else {
        return false;
    };
    let parent = match dst.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let (Ok(parent_abs), Some(name)) = (fs::canonicalize(parent), dst.file_name()) else {
        return false;
    };
    parent_abs.join(name).starts_with(&src_abs)
}

/// Creates a directory with the source's permission bits (plus owner rwx), subject to the
/// umask. Attributes are fixed up after the children are copied.
fn create_dir(dst: &Path, meta: &Metadata) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let mode = (meta.permissions().mode() & 0o777) | 0o700;
        fs::DirBuilder::new().mode(mode).create(dst)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        fs::create_dir(dst)
    }
}

/// Writes a regular file. The new file takes the source mode subject to the umask; with
/// `force` an unwritable destination is removed and recreated.
fn write_contents(src: &Path, dst: &Path, meta: &Metadata, force: bool) -> Result<(), String> {
    let src_text = display_path(src);
    let dst_text = display_path(dst);
    let mut input = File::open(src)
        .map_err(|error| format!("cannot open '{src_text}' for reading: {}", os_text(&error)))?;
    let mut output = match open_dest(dst, meta) {
        Ok(file) => file,
        Err(_) if force => {
            let _ = fs::remove_file(dst);
            open_dest(dst, meta).map_err(|error| {
                format!(
                    "cannot create regular file '{dst_text}': {}",
                    os_text(&error)
                )
            })?
        }
        Err(error) => {
            return Err(format!(
                "cannot create regular file '{dst_text}': {}",
                os_text(&error)
            ))
        }
    };
    io::copy(&mut input, &mut output)
        .map(|_| ())
        .map_err(|error| format!("error writing '{dst_text}': {}", os_text(&error)))
}

fn open_dest(dst: &Path, meta: &Metadata) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(meta.permissions().mode() & 0o7777);
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
    }
    options.open(dst)
}

fn make_hard_link(src: &Path, dst: &Path) -> Result<(), String> {
    fs::hard_link(src, dst).map_err(|error| {
        format!(
            "cannot create hard link '{}' to '{}': {}",
            display_path(dst),
            display_path(src),
            os_text(&error)
        )
    })
}

fn make_symbolic(src: &Path, dst: &Path) -> Result<(), String> {
    let target = std::path::absolute(src).unwrap_or_else(|_| src.to_path_buf());
    let is_dir = fs::metadata(src).is_ok_and(|meta| meta.is_dir());
    helpers::make_symlink(&target, dst, is_dir).map_err(|error| {
        format!(
            "cannot create symbolic link '{}': {}",
            display_path(dst),
            helpers::symlink_error_text(&error)
        )
    })
}

fn recreate_symlink(src: &Path, dst: &Path) -> Result<(), String> {
    let target = fs::read_link(src).map_err(|error| {
        format!(
            "cannot read symbolic link '{}': {}",
            display_path(src),
            os_text(&error)
        )
    })?;
    let is_dir = fs::metadata(src).is_ok_and(|meta| meta.is_dir());
    helpers::make_symlink(&target, dst, is_dir).map_err(|error| {
        format!(
            "cannot create symbolic link '{}': {}",
            display_path(dst),
            helpers::symlink_error_text(&error)
        )
    })
}
