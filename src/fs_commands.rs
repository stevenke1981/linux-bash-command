use crate::common::color::When;
use crate::common::error::io_text;
use crate::common::meta::is_executable;
use std::cmp::Ordering;
use std::env;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::display_path;
use crate::ls_style::{Layout, Style};

#[path = "ls_options.rs"]
mod ls_options;
#[path = "ls_output.rs"]
mod ls_output;

pub fn pwd(args: &[String]) -> i32 {
    let physical = match parse_pwd_mode(args) {
        Ok(physical) => physical,
        Err(code) => return code,
    };
    let resolved = match env::current_dir() {
        Ok(cwd) if physical => physical_path(&cwd),
        Ok(cwd) => Ok(logical_path(cwd)),
        Err(error) => Err(error),
    };
    match resolved {
        Ok(path) => match writeln!(io::stdout().lock(), "{}", display_path(&path)) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("pwd: cannot write output: {}", io_text(&error));
                1
            }
        },
        Err(error) => {
            eprintln!(
                "pwd: cannot determine current directory: {}",
                io_text(&error)
            );
            1
        }
    }
}

/// Returns true for `-P` (physical) and false for `-L` (logical); the last one wins.
fn parse_pwd_mode(args: &[String]) -> Result<bool, i32> {
    let mut physical = false;
    let mut options = true;
    for arg in args {
        let expanded;
        let arg = if options && arg.starts_with("--") && arg != "--" {
            expanded =
                crate::common::args::expand_long(arg, &["physical", "logical", "help", "version"])
                    .map_err(|error| crate::common::error::usage("pwd", error, 1))?;
            &expanded
        } else {
            arg
        };
        if options && arg == "--" {
            options = false;
        } else if options && (arg == "-h" || arg == "--help") {
            return Err(crate::common::io::output("pwd", |out| {
                writeln!(out, "Usage: pwd [-L | -P]\nPrint the current working directory.\n\nOptions:\n  -L, --logical   Print $PWD when it names the current directory (default)\n  -P, --physical  Print the directory with symbolic links resolved\n  -h, --help      Show this help\n      --version             Show version\n\nExit status: 0 on success, 1 on error.")
            }));
        } else if options && arg == "--version" {
            return Err(
                crate::common::args::help_version("pwd", &["--version".into()], "", true).unwrap(),
            );
        } else if options && arg == "--physical" {
            physical = true;
        } else if options && arg == "--logical" {
            physical = false;
        } else if options && arg.starts_with("--") {
            if let Some((name, _)) = arg.split_once('=') {
                return Err(crate::common::error::usage(
                    "pwd",
                    format!("option '{name}' doesn't allow an argument"),
                    1,
                ));
            }
            return Err(crate::common::error::usage(
                "pwd",
                format!("unrecognized option '{arg}'"),
                1,
            ));
        } else if options && arg.len() > 1 && arg.starts_with('-') {
            for flag in arg[1..].chars() {
                match flag {
                    'P' => physical = true,
                    'L' => physical = false,
                    _ => {
                        return Err(crate::common::error::usage(
                            "pwd",
                            format!("invalid option -- '{flag}'"),
                            1,
                        ));
                    }
                }
            }
        } else {
            return Err(crate::common::error::usage(
                "pwd",
                "this command does not accept arguments",
                1,
            ));
        }
    }
    Ok(physical)
}

fn physical_path(cwd: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(cwd).map(strip_verbatim_prefix)
}

/// `$PWD` is used only when it is an absolute path to the current directory.
fn logical_path(cwd: PathBuf) -> PathBuf {
    match env::var_os("PWD").map(PathBuf::from) {
        Some(candidate) if candidate.is_absolute() && same_directory(&candidate, &cwd) => candidate,
        _ => cwd,
    }
}

fn same_directory(left: &Path, right: &Path) -> bool {
    matches!(
        (fs::canonicalize(left), fs::canonicalize(right)),
        (Ok(a), Ok(b)) if a == b
    )
}

#[cfg(windows)]
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => PathBuf::from(text),
    }
}

#[cfg(not(windows))]
fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    path
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SortKey {
    #[default]
    Name,
    Natural,
    Time,
    Size,
    Unsorted,
    Extension,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum TimeStyle {
    /// `YYYY-MM-DD HH:MM`, the default.
    #[default]
    Minutes,
    /// `YYYY-MM-DD HH:MM:SS +0000`, from `--full-time` or `--time-style=full-iso`.
    Seconds,
    Iso,
    Locale,
}

#[derive(Default)]
struct LsOptions {
    all: bool,
    /// `.` and `..` are listed only with -a (not -A).
    dot_entries: bool,
    long: bool,
    classify: bool,
    slash: bool,
    human: bool,
    recursive: bool,
    directory: bool,
    reverse: bool,
    group_dirs_first: bool,
    sort: SortKey,
    time_style: TimeStyle,
    /// `--color` as given; `None` means the environment decides.
    color: Option<When>,
    layout: Layout,
    style: Style,
    inode: bool,
    blocks: bool,
    block_size: Option<u64>,
    allocated_block: Option<u64>,
    si: bool,
    ignore: Vec<crate::find_command::glob::Glob>,
    hide: Vec<crate::find_command::glob::Glob>,
    backups: bool,
    clock: ls_options::Clock,
    follow: u8,
    width: Option<usize>,
    comma: bool,
    quote: ls_output::Quoting,
    file_type: bool,
    time_format: Option<String>,
}

struct Entry {
    /// Text shown for the entry: the base name, or the argument as given.
    name: String,
    path: PathBuf,
    metadata: fs::Metadata,
}

impl Entry {
    fn is_real_dir(&self) -> bool {
        self.metadata.is_dir()
    }
}

enum Argument {
    Entry(Entry),
    Directory(PathBuf),
}

const LS_USAGE: &str = ls_options::HELP;

pub fn ls(args: &[String]) -> i32 {
    let (options, paths) = match parse_ls(args) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let headers = !options.directory && (paths.len() > 1 || options.recursive);
    let mut listing = Listing {
        options,
        headers,
        printed: false,
        status: 0,
        ancestors: Vec::new(),
    };
    let stdout = io::stdout();
    let mut out = BufWriter::with_capacity(crate::common::io::BUFFER_SIZE, stdout.lock());
    match listing.run(&paths, &mut out).and_then(|()| out.flush()) {
        Ok(()) => listing.status,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            eprintln!("ls: write error: {}", io_text(&error));
            1
        }
    }
}

fn parse_ls(args: &[String]) -> Result<(LsOptions, Vec<PathBuf>), i32> {
    ls_options::parse(args)
}

struct Listing {
    options: LsOptions,
    headers: bool,
    printed: bool,
    status: i32,
    ancestors: Vec<crate::common::meta::FileKey>,
}

impl Listing {
    fn run(&mut self, paths: &[PathBuf], out: &mut dyn Write) -> io::Result<()> {
        let mut entries = Vec::new();
        let mut directories = Vec::new();
        for path in paths {
            match self.argument(path) {
                Some(Argument::Entry(entry)) => entries.push(entry),
                Some(Argument::Directory(directory)) => directories.push(directory),
                None => {}
            }
        }
        self.print_entries(&mut entries, out)?;
        for directory in directories {
            self.list_directory(&directory, out, true)?;
        }
        Ok(())
    }

    fn argument(&mut self, path: &Path) -> Option<Argument> {
        let metadata = match if self.options.follow > 0 {
            fs::metadata(path)
        } else {
            fs::symlink_metadata(path)
        } {
            Ok(metadata) => metadata,
            Err(error) => {
                eprintln!(
                    "ls: cannot access '{}': {}",
                    display_path(path),
                    io_text(&error)
                );
                self.status = 2;
                return None;
            }
        };
        if self.lists_contents(path, &metadata) {
            return Some(Argument::Directory(path.to_path_buf()));
        }
        Some(Argument::Entry(Entry {
            name: display_path(path),
            path: path.to_path_buf(),
            metadata,
        }))
    }

    /// A command-line symlink to a directory lists the target unless -l or -d is given, as GNU ls does.
    fn lists_contents(&self, path: &Path, metadata: &fs::Metadata) -> bool {
        if self.options.directory {
            return false;
        }
        if metadata.is_dir() {
            return true;
        }
        !self.options.long
            && metadata.file_type().is_symlink()
            && fs::metadata(path)
                .map(|target| target.is_dir())
                .unwrap_or(false)
    }

    fn list_directory(
        &mut self,
        directory: &Path,
        out: &mut dyn Write,
        root: bool,
    ) -> io::Result<()> {
        let identity = fs::metadata(directory)
            .ok()
            .and_then(|metadata| crate::common::meta::file_key(directory, &metadata).ok());
        if identity
            .as_ref()
            .is_some_and(|id| self.ancestors.contains(id))
        {
            eprintln!(
                "ls: {}: not listing already-listed directory",
                display_path(directory)
            );
            self.status = self.status.max(2);
            return Ok(());
        }
        let Some(mut entries) = self.read_entries(directory, root) else {
            return Ok(());
        };
        if let Some(id) = &identity {
            self.ancestors.push(id.clone());
        }
        if self.headers {
            if self.printed {
                writeln!(out)?;
            }
            writeln!(out, "{}:", display_path(directory))?;
            self.printed = true;
        }
        if self.options.blocks {
            let bytes = entries
                .iter()
                .map(|entry| crate::common::meta::allocated_bytes(&entry.metadata))
                .sum();
            writeln!(
                out,
                "total {}",
                ls_output::allocated_size(bytes, &self.options)
            )?;
        }
        self.print_entries(&mut entries, out)?;
        if self.options.recursive {
            for entry in entries
                .iter()
                .filter(|entry| entry.is_real_dir() && !is_dot_name(&entry.name))
            {
                self.list_directory(&entry.path, out, false)?;
            }
        }
        if identity.is_some() {
            self.ancestors.pop();
        }
        Ok(())
    }

    fn read_entries(&mut self, directory: &Path, root: bool) -> Option<Vec<Entry>> {
        let items = match fs::read_dir(directory) {
            Ok(items) => items,
            Err(error) => {
                eprintln!(
                    "ls: cannot open directory '{}': {}",
                    display_path(directory),
                    io_text(&error)
                );
                self.status = self.status.max(if root { 2 } else { 1 });
                return None;
            }
        };
        let mut entries = Vec::new();
        if self.options.dot_entries {
            entries.extend(
                dot_entries(directory)
                    .into_iter()
                    .filter(|entry| ls_output::visible(&entry.name, &self.options)),
            );
        }
        for item in items {
            match item {
                Ok(item) => entries.extend(self.dir_entry(&item)),
                Err(error) => {
                    eprintln!(
                        "ls: cannot read directory '{}': {}",
                        display_path(directory),
                        io_text(&error)
                    );
                    self.status = self.status.max(1);
                }
            }
        }
        Some(entries)
    }

    fn dir_entry(&mut self, item: &fs::DirEntry) -> Option<Entry> {
        let name = item.file_name().to_string_lossy().into_owned();
        if (!self.options.all && name.starts_with('.')) || !ls_output::visible(&name, &self.options)
        {
            return None;
        }
        let path = item.path();
        match if self.options.follow == 2 {
            fs::metadata(&path)
        } else {
            item.metadata()
        } {
            Ok(metadata) => Some(Entry {
                name,
                path,
                metadata,
            }),
            Err(error) => {
                eprintln!(
                    "ls: cannot inspect '{}': {}",
                    display_path(&path),
                    io_text(&error)
                );
                self.status = self.status.max(1);
                None
            }
        }
    }

    fn print_entries(&mut self, entries: &mut [Entry], out: &mut dyn Write) -> io::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        sort_entries(entries, &self.options);
        let lines = if self.options.long {
            ls_output::long_lines(entries, &self.options)
        } else {
            let cells: Vec<String> = entries
                .iter()
                .map(|entry| entry_text(entry, &self.options))
                .collect();
            ls_output::arrange(cells, &self.options)
        };
        for line in lines {
            writeln!(out, "{line}")?;
        }
        self.printed = true;
        Ok(())
    }
}

fn is_dot_name(name: &str) -> bool {
    name == "." || name == ".."
}

/// The `.` and `..` entries that `-a` lists first, taken from the directory itself and its parent.
fn dot_entries(directory: &Path) -> Vec<Entry> {
    [".", ".."]
        .into_iter()
        .filter_map(|name| {
            let path = directory.join(name);
            let metadata = fs::symlink_metadata(&path).ok()?;
            Some(Entry {
                name: name.to_string(),
                path,
                metadata,
            })
        })
        .collect()
}

/// Colored name followed by its classify mark; the mark stays outside the color.
fn entry_text(entry: &Entry, options: &LsOptions) -> String {
    ls_output::entry_text(entry, options)
}

/// Name plus classify mark, followed by ` -> target` for symlinks in long format.
fn long_name(entry: &Entry, options: &LsOptions) -> String {
    let mut text = entry_text(entry, options);
    if entry.metadata.file_type().is_symlink() {
        if let Ok(target) = fs::read_link(&entry.path) {
            text.push_str(" -> ");
            text.push_str(&ls_output::quote(&display_path(&target), options.quote));
        }
    }
    text
}

fn indicator(entry: &Entry, options: &LsOptions) -> &'static str {
    if !options.classify && !options.slash && !options.file_type {
        return "";
    }
    let file_type = entry.metadata.file_type();
    if file_type.is_dir() {
        return "/";
    }
    if !options.classify && !options.file_type {
        return "";
    }
    if file_type.is_symlink() {
        "@"
    } else if options.classify && is_executable(&entry.path, &entry.metadata) {
        "*"
    } else {
        ""
    }
}

fn sort_entries(entries: &mut [Entry], options: &LsOptions) {
    entries.sort_by(|left, right| compare_entries(left, right, options));
    if options.reverse {
        entries.reverse();
    }
    if options.group_dirs_first {
        entries.sort_by_key(|entry| !entry.is_real_dir());
    }
}

fn compare_entries(left: &Entry, right: &Entry, options: &LsOptions) -> Ordering {
    match options.sort {
        SortKey::Name => left.name.cmp(&right.name),
        SortKey::Natural => natural_cmp(&left.name, &right.name),
        SortKey::Time => ls_output::time(right, options.clock)
            .cmp(&ls_output::time(left, options.clock))
            .then_with(|| left.name.cmp(&right.name)),
        SortKey::Size => right
            .metadata
            .len()
            .cmp(&left.metadata.len())
            .then_with(|| left.name.cmp(&right.name)),
        SortKey::Unsorted => Ordering::Equal,
        SortKey::Extension => extension(&left.name)
            .cmp(extension(&right.name))
            .then_with(|| left.name.cmp(&right.name)),
    }
}

fn extension(name: &str) -> &str {
    name.rsplit_once('.').map_or("", |(_, suffix)| suffix)
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let (mut left, mut right) = (left.as_bytes(), right.as_bytes());
    loop {
        match (left.first(), right.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let (left_digits, left_rest) = split_digits(left);
                let (right_digits, right_rest) = split_digits(right);
                match compare_numbers(left_digits, right_digits) {
                    Ordering::Equal => {}
                    other => return other,
                }
                left = left_rest;
                right = right_rest;
            }
            (Some(a), Some(b)) => {
                match a.cmp(b) {
                    Ordering::Equal => {}
                    other => return other,
                }
                left = &left[1..];
                right = &right[1..];
            }
        }
    }
}

fn split_digits(text: &[u8]) -> (&[u8], &[u8]) {
    let end = text
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

fn compare_numbers(left: &[u8], right: &[u8]) -> Ordering {
    let (left, right) = (trim_leading_zeros(left), trim_leading_zeros(right));
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn trim_leading_zeros(digits: &[u8]) -> &[u8] {
    let start = digits
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(digits.len());
    &digits[start..]
}

#[cfg(test)]
mod ls_tests {
    use super::natural_cmp;
    use crate::common::size::human_size;
    use std::cmp::Ordering;

    #[test]
    fn human_sizes_round_up_like_gnu() {
        assert_eq!(human_size(0), "0");
        assert_eq!(human_size(1000), "1000");
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1025), "1.1K");
        assert_eq!(human_size(1500), "1.5K");
        assert_eq!(human_size(4096), "4.0K");
        assert_eq!(human_size(10 * 1024), "10K");
        assert_eq!(human_size(1_048_550), "1.0M");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0G");
    }

    #[test]
    fn natural_order_compares_numbers_by_value() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("file010", "file10"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "b"), Ordering::Less);
    }
}
