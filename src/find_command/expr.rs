//! Expression tree for find, plus evaluation of the tests against one entry.

pub use crate::common::meta::{kind_char, permission_bits_at};

use super::glob::Glob;
use super::printf::Piece;
use crate::display_path;
use regex::Regex;
use std::fs::{self, File, Metadata};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const NS_PER_SEC: i128 = 1_000_000_000;
pub const DAY_NS: i128 = 86_400 * NS_PER_SEC;
pub const MINUTE_NS: i128 = 60 * NS_PER_SEC;

/// A parsed find expression. Operators bind as in GNU find: `!` > `-a` > `-o` > `,`.
pub enum Expr {
    Const(bool),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Comma(Box<Expr>, Box<Expr>),
    Test(Test),
    Action(Action),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Less,
    Equal,
    Greater,
}

impl Cmp {
    pub fn holds(self, actual: i128, target: i128) -> bool {
        match self {
            Cmp::Less => actual < target,
            Cmp::Equal => actual == target,
            Cmp::Greater => actual > target,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    Modified,
    Accessed,
    Changed,
    Birth,
}

#[derive(Clone, Copy)]
pub enum PermCheck {
    Exact(u32),
    AllBits(u32),
    AnyBit(u32),
}

#[derive(Clone, Copy)]
pub enum Access {
    Read,
    Write,
    Execute,
}

pub enum Test {
    Name(Glob),
    Path(Glob),
    Regex(Regex),
    Type(Vec<char>),
    Empty,
    /// `-size`: compares the size rounded up to whole `unit_bytes` units.
    Size {
        cmp: Cmp,
        units: i64,
        unit_bytes: u64,
    },
    /// `-mtime` and friends: whole periods of `unit_ns` elapsed since the clock time.
    Age {
        clock: Clock,
        cmp: Cmp,
        count: i64,
        unit_ns: i128,
    },
    /// `-newer` and friends: the clock time is later than the reference time.
    Newer {
        clock: Clock,
        reference_ns: i128,
    },
    Perm(PermCheck),
    Access(Access),
    Owner {
        group: bool,
        id: u32,
    },
    UnknownOwner {
        group: bool,
    },
    Number {
        field: NumberField,
        cmp: Cmp,
        value: u64,
    },
    SameFile(Option<crate::common::meta::FileKey>),
    LinkName(Glob),
    XType(Vec<char>),
}

pub enum NumberField {
    Uid,
    Gid,
    Links,
    Inode,
}

pub enum Action {
    Print,
    Print0,
    Printf(Vec<Piece>),
    Ls,
    Prune,
    Quit,
    Delete,
    /// `-exec CMD {} ;` or `-execdir CMD {} ;`, run once per entry.
    Exec {
        argv: Vec<String>,
        in_dir: bool,
        prompt: bool,
    },
    /// `-exec CMD {} +`, queued into batch number `index` of the plan.
    Batch(usize),
    File {
        index: usize,
        action: Box<Action>,
    },
}

/// One file system entry as seen by the tests and actions.
pub struct Entry<'a> {
    pub path: &'a Path,
    /// The starting point this entry was reached from (used by `%P`).
    pub root: &'a Path,
    pub depth: usize,
    pub meta: &'a Metadata,
    pub now_ns: i128,
}

pub fn now_ns() -> i128 {
    system_ns(SystemTime::now())
}

pub fn system_ns(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    }
}

/// Returns the clock value in nanoseconds since the Unix epoch, when the platform has it.
pub fn time_ns(meta: &Metadata, clock: Clock) -> Option<i128> {
    match clock {
        Clock::Modified => meta.modified().ok().map(system_ns),
        Clock::Accessed => meta.accessed().ok().map(system_ns),
        Clock::Changed => changed_ns(meta),
        Clock::Birth => meta.created().ok().map(system_ns),
    }
}

pub fn time_ns_at(path: &Path, meta: &Metadata, clock: Clock) -> Option<i128> {
    if clock == Clock::Changed {
        crate::common::meta::change_time(path, meta).map(system_ns)
    } else {
        time_ns(meta, clock)
    }
}

#[cfg(unix)]
fn changed_ns(meta: &Metadata) -> Option<i128> {
    use std::os::unix::fs::MetadataExt;
    Some(i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec()))
}

#[cfg(not(unix))]
fn changed_ns(_meta: &Metadata) -> Option<i128> {
    None
}

/// Name of the entry as used by `-name` and `%f`: the last component, or the text for `.`.
pub fn file_name_text(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

pub fn test_matches(test: &Test, entry: &Entry) -> bool {
    match test {
        Test::Name(glob) => glob.is_match(&file_name_text(entry.path)),
        Test::Path(glob) => glob.is_match(&display_path(entry.path)),
        Test::Regex(regex) => regex.is_match(&display_path(entry.path)),
        Test::Type(kinds) => kinds.contains(&kind_char(entry.meta)),
        Test::Empty => is_empty(entry),
        Test::Size {
            cmp,
            units,
            unit_bytes,
        } => {
            let needed = entry.meta.len().div_ceil(*unit_bytes);
            cmp.holds(i128::from(needed), i128::from(*units))
        }
        Test::Age {
            clock,
            cmp,
            count,
            unit_ns,
        } => time_ns_at(entry.path, entry.meta, *clock).is_some_and(|time| {
            let periods = (entry.now_ns - time).div_euclid(*unit_ns);
            cmp.holds(periods, i128::from(*count))
        }),
        Test::Newer {
            clock,
            reference_ns,
        } => time_ns_at(entry.path, entry.meta, *clock).is_some_and(|time| time > *reference_ns),
        Test::Perm(check) => perm_matches(entry.path, entry.meta, *check),
        Test::Access(access) => access_ok(entry, *access),
        Test::Owner { group, id } => {
            let ids = crate::common::meta::owner_ids(entry.meta);
            *id == if *group { ids.1 } else { ids.0 }
        }
        Test::UnknownOwner { group } => {
            let (uid, gid) = crate::common::meta::owner_ids(entry.meta);
            if *group {
                crate::common::meta::group_name(gid).is_none()
            } else {
                crate::common::meta::user_name(uid).is_none()
            }
        }
        Test::Number { field, cmp, value } => {
            let identity = crate::common::meta::file_identity(entry.path, entry.meta);
            let (uid, gid) = crate::common::meta::owner_ids(entry.meta);
            let actual = match field {
                NumberField::Uid => u64::from(uid),
                NumberField::Gid => u64::from(gid),
                NumberField::Links => identity.nlink,
                NumberField::Inode => identity.inode,
            };
            cmp.holds(i128::from(actual), i128::from(*value))
        }
        Test::SameFile(reference) => reference.as_ref().is_some_and(|reference| {
            crate::common::meta::file_key(entry.path, entry.meta).is_ok_and(|key| key == *reference)
        }),
        Test::LinkName(glob) => {
            entry.meta.file_type().is_symlink()
                && fs::read_link(entry.path)
                    .is_ok_and(|target| glob.is_match(&display_path(&target)))
        }
        Test::XType(kinds) => {
            let metadata = if entry.meta.file_type().is_symlink() {
                fs::metadata(entry.path).or_else(|_| fs::symlink_metadata(entry.path))
            } else {
                fs::symlink_metadata(entry.path)
            };
            metadata.is_ok_and(|metadata| kinds.contains(&kind_char(&metadata)))
        }
    }
}

/// `-perm` is rejected at parse time on Windows, so the approximated bits only matter
/// on Unix in practice.
fn perm_matches(path: &Path, meta: &Metadata, check: PermCheck) -> bool {
    let mode = permission_bits_at(path, meta);
    match check {
        PermCheck::Exact(bits) => mode == bits,
        PermCheck::AllBits(bits) => mode & bits == bits,
        PermCheck::AnyBit(bits) => mode & bits != 0,
    }
}

fn is_empty(entry: &Entry) -> bool {
    if entry.meta.is_dir() {
        fs::read_dir(entry.path).is_ok_and(|mut entries| entries.next().is_none())
    } else {
        entry.meta.is_file() && entry.meta.len() == 0
    }
}

fn access_ok(entry: &Entry, access: Access) -> bool {
    let meta = entry.meta;
    match access {
        Access::Read => {
            if meta.is_file() {
                File::open(entry.path).is_ok()
            } else if meta.is_dir() {
                fs::read_dir(entry.path).is_ok()
            } else {
                true
            }
        }
        Access::Write => !meta.permissions().readonly(),
        Access::Execute => is_executable(entry),
    }
}

#[cfg(unix)]
fn is_executable(entry: &Entry) -> bool {
    // Checks the mode bits only, not the caller's identity.
    permission_bits_at(entry.path, entry.meta) & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(entry: &Entry) -> bool {
    entry.meta.is_dir() || crate::common::meta::is_executable(entry.path, entry.meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_samefile_reference_never_matches_an_unrelated_entry() {
        let path = std::env::temp_dir().join(format!("agentcmd-samefile-{}", std::process::id()));
        fs::write(&path, b"x").unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let entry = Entry {
            path: &path,
            root: &path,
            depth: 0,
            meta: &metadata,
            now_ns: 0,
        };
        assert!(!test_matches(&Test::SameFile(None), &entry));
        assert!(test_matches(
            &Test::SameFile(crate::common::meta::file_key(&path, &metadata).ok()),
            &entry
        ));
        fs::remove_file(path).unwrap();
    }
}
