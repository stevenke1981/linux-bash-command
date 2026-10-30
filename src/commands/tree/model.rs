//! Reads the directory tree into [`Node`]s, applying the entry filters (hidden names,
//! `-I`, `-P`, `-d`, `.gitignore`), `--filelimit`, `--prune` and the sort order.
//!
//! Symlinks are never descended into, but a symlink to a directory counts as a directory for
//! filtering and for the totals, as tree 2.x does.

use std::fs::{self, FileType, Metadata};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;

use super::glob::Patterns;
use super::sort::Order;
use crate::common::meta::{mode_string, permission_bits};

/// What kind of entry a node is. Windows has no FIFOs, sockets or device files, so those
/// variants are never built there.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
pub enum Class {
    Dir,
    File,
    Link { target: String, to_dir: bool },
    Fifo,
    Socket,
    CharDev,
    BlockDev,
}

impl Class {
    /// Directories, and symlinks that point at directories.
    pub fn is_dir_like(&self) -> bool {
        matches!(self, Class::Dir | Class::Link { to_dir: true, .. })
    }

    /// The `type` value used in JSON output.
    pub fn json_type(&self) -> &'static str {
        match self {
            Class::Dir => "directory",
            Class::File => "file",
            Class::Link { .. } => "link",
            Class::Fifo => "fifo",
            Class::Socket => "socket",
            Class::CharDev => "char",
            Class::BlockDev => "block",
        }
    }
}

/// Metadata shown with `-p`, `-s`, `-h`, `-D` and used for sorting.
#[derive(Clone, Debug, Default)]
pub struct Stat {
    pub size: u64,
    pub mode_text: String,
    pub mtime: i64,
    pub ctime: i64,
    pub executable: bool,
}

/// One entry in the output tree.
#[derive(Debug)]
pub struct Node {
    /// The basename, or the operand as given for a root.
    pub name: String,
    pub path: PathBuf,
    pub class: Class,
    pub stat: Stat,
    pub children: Vec<Node>,
    /// The directory was read, so its children are known (possibly none).
    pub listed: bool,
    /// The directory could not be read.
    pub error: bool,
    /// A root operand that does not exist.
    pub missing: bool,
    /// The directory had more entries than `--filelimit`; the count is kept for the message.
    pub over_limit: Option<usize>,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.class.is_dir_like()
    }
}

/// Options that decide which entries appear and in what order.
pub struct Filter {
    pub all: bool,
    pub dirs_only: bool,
    pub max_depth: Option<usize>,
    pub ignore: Option<Patterns>,
    pub include: Option<Patterns>,
    pub matchdirs: bool,
    pub prune: bool,
    pub filelimit: Option<usize>,
    pub gitignore: bool,
    pub order: Order,
}

/// A directory entry, read but not yet turned into a node.
struct Entry {
    name: String,
    path: PathBuf,
    meta: Metadata,
    class: Class,
}

/// Scans one root operand. Its existence is not filtered by any option.
pub fn scan_root(arg: &str, filter: &Filter) -> Node {
    let path = PathBuf::from(arg);
    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            let class = classify(&path, &meta);
            let entry = Entry {
                name: arg.to_string(),
                path,
                meta,
                class,
            };
            // A root is at depth 0; its children are at depth 1, which `-L` counts from.
            build(entry, 0, filter, &[])
        }
        Err(_) => Node {
            name: arg.to_string(),
            path,
            class: Class::File,
            stat: Stat::default(),
            children: Vec::new(),
            listed: false,
            error: true,
            missing: true,
            over_limit: None,
        },
    }
}

fn build(entry: Entry, level: usize, filter: &Filter, ignores: &[Gitignore]) -> Node {
    let stat = stat_of(&entry.path, &entry.meta, &entry.class);
    let mut node = Node {
        name: entry.name,
        path: entry.path,
        class: entry.class,
        stat,
        children: Vec::new(),
        listed: false,
        error: false,
        missing: false,
        over_limit: None,
    };
    let within_depth = filter.max_depth.is_none_or(|max| level < max);
    if node.class == Class::Dir && within_depth {
        descend(&mut node, level, filter, ignores);
    }
    node
}

fn descend(node: &mut Node, level: usize, filter: &Filter, ignores: &[Gitignore]) {
    let entries = match read_entries(&node.path) {
        Ok(entries) => entries,
        Err(_) => {
            node.error = true;
            return;
        }
    };
    node.listed = true;
    let stack = if filter.gitignore {
        with_ignore_file(&node.path, ignores)
    } else {
        ignores.to_vec()
    };
    let kept: Vec<Entry> = entries
        .into_iter()
        .filter(|entry| keep(entry, filter, &stack))
        .collect();
    if let Some(limit) = filter.filelimit {
        if kept.len() > limit {
            node.over_limit = Some(kept.len());
            node.listed = false;
            return;
        }
    }
    let mut children = Vec::with_capacity(kept.len());
    for entry in kept {
        let child = build(entry, level + 1, filter, &stack);
        if !prunable(&child, filter) {
            children.push(child);
        }
    }
    filter.order.apply(&mut children);
    node.children = children;
}

/// `--prune`: an empty directory that was read is dropped, unless `--matchdirs` keeps it
/// because its name matches the `-P` pattern.
fn prunable(node: &Node, filter: &Filter) -> bool {
    if !filter.prune || node.class != Class::Dir || !node.listed || !node.children.is_empty() {
        return false;
    }
    let matched = filter.matchdirs
        && filter
            .include
            .as_ref()
            .is_some_and(|pattern| pattern.matches(&node.name));
    !matched
}

fn read_entries(dir: &Path) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else {
            continue;
        };
        let path = item.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let class = classify(&path, &meta);
        out.push(Entry {
            name: item.file_name().to_string_lossy().into_owned(),
            path,
            meta,
            class,
        });
    }
    Ok(out)
}

/// Whether an entry survives the name, type and ignore filters.
fn keep(entry: &Entry, filter: &Filter, ignores: &[Gitignore]) -> bool {
    if !filter.all && entry.name.starts_with('.') {
        return false;
    }
    let is_dir = entry.class.is_dir_like();
    if filter.dirs_only && !is_dir {
        return false;
    }
    if filter
        .ignore
        .as_ref()
        .is_some_and(|p| p.matches(&entry.name))
    {
        return false;
    }
    if !is_dir
        && filter
            .include
            .as_ref()
            .is_some_and(|p| !p.matches(&entry.name))
    {
        return false;
    }
    !gitignored(&entry.path, is_dir, ignores)
}

/// The ignore files that apply to a directory: those of its ancestors plus its own
/// `.gitignore`, deepest last.
fn with_ignore_file(dir: &Path, ignores: &[Gitignore]) -> Vec<Gitignore> {
    let mut stack = ignores.to_vec();
    let file = dir.join(".gitignore");
    if file.is_file() {
        let mut builder = GitignoreBuilder::new(dir);
        if builder.add(&file).is_none() {
            if let Ok(gitignore) = builder.build() {
                stack.push(gitignore);
            }
        }
    }
    stack
}

/// The deepest ignore file with an opinion decides; a whitelist rule keeps the entry.
fn gitignored(path: &Path, is_dir: bool, ignores: &[Gitignore]) -> bool {
    for gitignore in ignores.iter().rev() {
        match gitignore.matched(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

/// Classifies an entry from its own (not followed) metadata.
fn classify(path: &Path, meta: &Metadata) -> Class {
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = fs::read_link(path)
            .map(|target| target.to_string_lossy().into_owned())
            .unwrap_or_default();
        let to_dir = fs::metadata(path).is_ok_and(|followed| followed.is_dir());
        return Class::Link { target, to_dir };
    }
    if kind.is_dir() {
        return Class::Dir;
    }
    special_kind(&kind).unwrap_or(Class::File)
}

#[cfg(unix)]
fn special_kind(kind: &FileType) -> Option<Class> {
    use std::os::unix::fs::FileTypeExt;
    if kind.is_fifo() {
        Some(Class::Fifo)
    } else if kind.is_socket() {
        Some(Class::Socket)
    } else if kind.is_char_device() {
        Some(Class::CharDev)
    } else if kind.is_block_device() {
        Some(Class::BlockDev)
    } else {
        None
    }
}

#[cfg(not(unix))]
fn special_kind(_kind: &FileType) -> Option<Class> {
    None
}

fn stat_of(path: &Path, meta: &Metadata, class: &Class) -> Stat {
    Stat {
        size: meta.len(),
        mode_text: mode_string(path, meta),
        mtime: seconds(meta.modified().ok()),
        ctime: change_seconds(meta),
        executable: *class == Class::File && permission_bits(path, meta) & 0o111 != 0,
    }
}

fn seconds(time: Option<SystemTime>) -> i64 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(unix)]
fn change_seconds(meta: &Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    meta.ctime()
}

#[cfg(not(unix))]
fn change_seconds(meta: &Metadata) -> i64 {
    seconds(meta.modified().ok())
}

/// Totals over the shown entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub dirs: usize,
    pub files: usize,
}

/// Counts the nodes that are shown. A missing root is not counted.
pub fn tally(nodes: &[Node]) -> Counts {
    let mut counts = Counts::default();
    for node in nodes {
        if node.missing {
            continue;
        }
        if node.is_dir() {
            counts.dirs += 1;
        } else {
            counts.files += 1;
        }
        let inner = tally(&node.children);
        counts.dirs += inner.dirs;
        counts.files += inner.files;
    }
    counts
}
