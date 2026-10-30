//! Expression tree for find, plus evaluation of the tests against one entry.

use super::glob::Glob;
use super::owner;
use super::printf::Piece;
use crate::common::meta::{inode_nlink, kind_char, permission_bits};
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
    /// Creation (birth) time, as `-newerB..`.
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
    /// `-uid`, `-user`: the owner id equals the value.
    Uid(u32),
    /// `-gid`, `-group`: the group id equals the value.
    Gid(u32),
    /// `-nouser`: the owner id has no account entry.
    NoUser,
    /// `-nogroup`: the group id has no group entry.
    NoGroup,
    /// `-links [+-]N`: the hard-link count compares with `count`.
    Links {
        cmp: Cmp,
        count: u64,
    },
    /// `-inum N`: the inode number equals `inode`.
    Inum(u64),
    /// `-samefile F`: same device and inode as the reference file.
    SameFile {
        dev: u64,
        inode: u64,
    },
    /// `-lname` and `-ilname`: the symbolic link's target matches the glob.
    LinkName(Glob),
    /// `-xtype`: like `-type`, but a symbolic link is classified by its target.
    XType(Vec<char>),
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
    },
    /// `-exec CMD {} +`, queued into batch number `index` of the plan.
    Batch(usize),
    /// `-ok CMD {} ;` or `-okdir CMD {} ;`: asks on stderr and reads a line from stdin.
    Confirm {
        argv: Vec<String>,
        in_dir: bool,
    },
    /// `-fprint`, `-fprint0`, `-fprintf` and `-fls`: write to output sink `sink`.
    FilePrint {
        sink: usize,
        nul: bool,
    },
    FilePrintf {
        sink: usize,
        pieces: Vec<Piece>,
    },
    FileLs {
        sink: usize,
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

/// Device and inode numbers, which identify a file for `-samefile` and `-inum`.
#[cfg(unix)]
pub fn dev_ino(meta: &Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (meta.dev(), meta.ino())
}

/// Windows has no inode numbers here; `-samefile` and `-inum` are rejected at parse time.
#[cfg(not(unix))]
pub fn dev_ino(_meta: &Metadata) -> (u64, u64) {
    (0, 0)
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
        } => time_ns(entry.meta, *clock).is_some_and(|time| {
            let periods = (entry.now_ns - time).div_euclid(*unit_ns);
            cmp.holds(periods, i128::from(*count))
        }),
        Test::Newer {
            clock,
            reference_ns,
        } => time_ns(entry.meta, *clock).is_some_and(|time| time > *reference_ns),
        Test::Perm(check) => perm_matches(entry, *check),
        Test::Access(access) => access_ok(entry, *access),
        Test::Uid(uid) => owner::owner_uid(entry.meta) == *uid,
        Test::Gid(gid) => owner::owner_gid(entry.meta) == *gid,
        Test::NoUser => owner::has_no_user(entry.meta),
        Test::NoGroup => owner::has_no_group(entry.meta),
        Test::Links { cmp, count } => {
            cmp.holds(i128::from(inode_nlink(entry.meta).1), i128::from(*count))
        }
        Test::Inum(inode) => inode_nlink(entry.meta).0 == *inode,
        Test::SameFile { dev, inode } => dev_ino(entry.meta) == (*dev, *inode),
        Test::LinkName(glob) => link_target(entry).is_some_and(|target| glob.is_match(&target)),
        Test::XType(kinds) => xtype_matches(entry, kinds),
    }
}

/// The text of a symbolic link's target, or `None` when the entry is not a link.
fn link_target(entry: &Entry) -> Option<String> {
    if !entry.meta.file_type().is_symlink() {
        return None;
    }
    fs::read_link(entry.path)
        .ok()
        .map(|target| display_path(&target))
}

/// `-xtype`: a link counts as the type of its target; a dangling link is `l`.
fn xtype_matches(entry: &Entry, kinds: &[char]) -> bool {
    if !entry.meta.file_type().is_symlink() {
        return kinds.contains(&kind_char(entry.meta));
    }
    match fs::metadata(entry.path) {
        Ok(target) => kinds.contains(&kind_char(&target)),
        Err(_) => kinds.contains(&'l'),
    }
}

/// `-perm` is rejected at parse time on Windows, so the synthesised bits only matter
/// on Unix in practice.
fn perm_matches(entry: &Entry, check: PermCheck) -> bool {
    let mode = permission_bits(entry.path, entry.meta);
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
    permission_bits(entry.path, entry.meta) & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(entry: &Entry) -> bool {
    if entry.meta.is_dir() {
        return true;
    }
    entry
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "exe" | "bat" | "cmd" | "com"
            )
        })
}
