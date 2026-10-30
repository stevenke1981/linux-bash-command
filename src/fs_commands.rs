use crate::common::args::prepare_legacy;
use crate::common::error::{io_error_text, EXIT_FAIL, EXIT_TROUBLE};
use crate::common::meta::{inode_nlink, mode_string};
use crate::common::path::strip_verbatim;
use crate::common::size::human_size;
use std::cmp::Ordering;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::display_path;

const SECONDS_PER_DAY: i64 = 86_400;

/// Accepts `--color`, `--colour` and their `=WHEN` forms. Output is never colored.
pub(crate) fn is_color_option(arg: &str) -> bool {
    let when = arg
        .strip_prefix("--color")
        .or_else(|| arg.strip_prefix("--colour"));
    match when {
        Some("") => true,
        Some(value) => matches!(
            value,
            "=auto"
                | "=always"
                | "=never"
                | "=yes"
                | "=no"
                | "=tty"
                | "=force"
                | "=none"
                | "=if-tty"
        ),
        None => false,
    }
}

const PWD_LONGS: &[&str] = &["help", "logical", "physical"];
const LS_LONGS: &[&str] = &[
    "all",
    "almost-all",
    "classify",
    "color",
    "directory",
    "full-time",
    "group-directories-first",
    "help",
    "human-readable",
    "recursive",
    "reverse",
    "show-control-chars",
    "time-style",
];

pub fn pwd(args: &[String]) -> i32 {
    let expanded = match prepare_legacy("pwd", args, PWD_LONGS, &[], "", EXIT_FAIL) {
        Ok(expanded) => expanded,
        Err(code) => return code,
    };
    let args = expanded.as_slice();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("Usage: pwd [-L | -P]\nPrint the current working directory.\n\nOptions:\n  -L, --logical   Print $PWD when it names the current directory (default)\n  -P, --physical  Print the directory with symbolic links resolved\n  -h, --help      Show this help\n\nExit status: 0 on success, 1 on error.");
        return 0;
    }
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
                eprintln!("pwd: cannot write output: {}", io_error_text(&error));
                1
            }
        },
        Err(error) => {
            eprintln!(
                "pwd: cannot determine current directory: {}",
                io_error_text(&error)
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
        if options && arg == "--" {
            options = false;
        } else if options && arg == "--physical" {
            physical = true;
        } else if options && arg == "--logical" {
            physical = false;
        } else if options && arg.starts_with("--") {
            eprintln!("pwd: unknown option: {arg}");
            return Err(1);
        } else if options && arg.len() > 1 && arg.starts_with('-') {
            for flag in arg[1..].chars() {
                match flag {
                    'P' => physical = true,
                    'L' => physical = false,
                    _ => {
                        eprintln!("pwd: unknown option: -{flag}");
                        return Err(1);
                    }
                }
            }
        } else {
            eprintln!("pwd: this command does not accept arguments");
            return Err(1);
        }
    }
    Ok(physical)
}

fn physical_path(cwd: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(cwd).map(|path| strip_verbatim(&path))
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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SortKey {
    #[default]
    Name,
    Natural,
    Time,
    Size,
    Unsorted,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum TimeStyle {
    /// `YYYY-MM-DD HH:MM`, the default.
    #[default]
    Minutes,
    /// `YYYY-MM-DD HH:MM:SS +0000`, from `--full-time` or `--time-style=full-iso`.
    Seconds,
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

const LS_USAGE: &str = "Usage: ls [OPTION]... [PATH]...\nList directory entries, one per line. Times are shown in UTC.\n\nOptions:\n  -a, --all                Include dot-prefixed names, then . and ..\n  -A, --almost-all         Include dot-prefixed names, without . and ..\n  -f                       Same as -a -U\n  -l, -n, -o, -g           Long format: mode, link count, owner '-', group '-', size, time, name\n  -h, --human-readable     With -l, print sizes such as 512, 4.0K, 12M\n  -F, --classify           Append / to directories, @ to symlinks, * to executables\n  -p                       Append / to directories\n  -d, --directory          List directories themselves, not their contents\n  -R, --recursive          List subdirectories recursively\n  -t                       Sort by modification time, newest first\n  -S                       Sort by size, largest first\n  -v                       Sort names in natural order (file2 before file10)\n  -U                       Do not sort; use directory order\n  -r, --reverse            Reverse the sort order\n      --group-directories-first\n                           List directories before files\n      --full-time          With -l, show seconds and +0000\n      --time-style=STYLE   full-iso (seconds), or long-iso, iso, locale (minutes)\n  -x, -C, -1, -G           Accepted; output is always one entry per line\n      --color[=WHEN]       Accepted; output is never colored\n      --show-control-chars Accepted and ignored\n  -i and -s are not supported and exit 2\n\nExit status: 0 on success, 2 if a path operand could not be accessed or listed, 1 if an entry inside a directory could not be read.\n";

pub fn ls(args: &[String]) -> i32 {
    let expanded = match prepare_legacy("ls", args, LS_LONGS, &[], "", EXIT_TROUBLE) {
        Ok(expanded) => expanded,
        Err(code) => return code,
    };
    let (options, paths) = match parse_ls(&expanded) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let headers = !options.directory && (paths.len() > 1 || options.recursive);
    let mut listing = Listing {
        options,
        headers,
        printed: false,
        failed: false,
        trouble: false,
    };
    let stdout = io::stdout();
    let mut out = stdout.lock();
    match listing.run(&paths, &mut out) {
        Ok(()) if listing.trouble => 2,
        Ok(()) if listing.failed => 1,
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ls: cannot write output: {}", io_error_text(&error));
            2
        }
    }
}

fn parse_ls(args: &[String]) -> Result<(LsOptions, Vec<PathBuf>), i32> {
    let mut options = LsOptions::default();
    let mut paths = Vec::new();
    let mut end_of_options = false;
    for arg in args {
        if end_of_options || arg == "-" || !arg.starts_with('-') {
            paths.push(PathBuf::from(arg));
        } else if arg == "--" {
            end_of_options = true;
        } else if arg == "--help" {
            print!("{LS_USAGE}");
            return Err(0);
        } else if let Some(name) = arg.strip_prefix("--") {
            parse_ls_long(name, arg, &mut options)?;
        } else {
            parse_ls_short(&arg[1..], &mut options)?;
        }
    }
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    Ok((options, paths))
}

fn parse_ls_short(flags: &str, options: &mut LsOptions) -> Result<(), i32> {
    for flag in flags.chars() {
        match flag {
            'a' => {
                options.all = true;
                options.dot_entries = true;
            }
            'A' => options.all = true,
            'f' => {
                options.all = true;
                options.dot_entries = true;
                options.sort = SortKey::Unsorted;
            }
            'l' | 'n' | 'o' | 'g' => options.long = true,
            'F' => options.classify = true,
            'p' => options.slash = true,
            'h' => options.human = true,
            'R' => options.recursive = true,
            'd' => options.directory = true,
            'r' => options.reverse = true,
            't' => options.sort = SortKey::Time,
            'S' => options.sort = SortKey::Size,
            'U' => options.sort = SortKey::Unsorted,
            'v' => options.sort = SortKey::Natural,
            'x' | 'C' | '1' | 'G' => {}
            'i' | 's' => {
                eprintln!("ls: -{flag} is not supported");
                return Err(2);
            }
            _ => {
                eprintln!("ls: unknown option: -{flag}");
                return Err(2);
            }
        }
    }
    Ok(())
}

fn parse_ls_long(name: &str, arg: &str, options: &mut LsOptions) -> Result<(), i32> {
    if is_color_option(arg) || name == "show-control-chars" {
        return Ok(());
    }
    match name {
        "all" => {
            options.all = true;
            options.dot_entries = true;
        }
        "almost-all" => options.all = true,
        "classify" => options.classify = true,
        "directory" => options.directory = true,
        "recursive" => options.recursive = true,
        "reverse" => options.reverse = true,
        "human-readable" => options.human = true,
        "group-directories-first" => options.group_dirs_first = true,
        "full-time" => options.time_style = TimeStyle::Seconds,
        _ => match name.strip_prefix("time-style=") {
            Some(style) => options.time_style = parse_time_style(style)?,
            None => {
                eprintln!("ls: unknown option: {arg}");
                return Err(2);
            }
        },
    }
    Ok(())
}

fn parse_time_style(style: &str) -> Result<TimeStyle, i32> {
    match style {
        "full-iso" => Ok(TimeStyle::Seconds),
        "long-iso" | "iso" | "locale" => Ok(TimeStyle::Minutes),
        _ => {
            eprintln!("ls: unsupported time style: {style}");
            Err(2)
        }
    }
}

struct Listing {
    options: LsOptions,
    headers: bool,
    printed: bool,
    /// A minor error (inside a directory): exit status 1.
    failed: bool,
    /// A command-line operand could not be used: exit status 2 (serious trouble).
    trouble: bool,
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
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                eprintln!(
                    "ls: cannot access '{}': {}",
                    display_path(path),
                    io_error_text(&error)
                );
                self.trouble = true;
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

    /// `top` is true for a directory named on the command line.
    fn list_directory(
        &mut self,
        directory: &Path,
        out: &mut dyn Write,
        top: bool,
    ) -> io::Result<()> {
        let Some(mut entries) = self.read_entries(directory, top) else {
            return Ok(());
        };
        if self.headers {
            if self.printed {
                writeln!(out)?;
            }
            writeln!(out, "{}:", display_path(directory))?;
            self.printed = true;
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
        Ok(())
    }

    fn read_entries(&mut self, directory: &Path, top: bool) -> Option<Vec<Entry>> {
        let items = match fs::read_dir(directory) {
            Ok(items) => items,
            Err(error) => {
                eprintln!(
                    "ls: cannot open directory '{}': {}",
                    display_path(directory),
                    io_error_text(&error)
                );
                if top {
                    self.trouble = true;
                } else {
                    self.failed = true;
                }
                return None;
            }
        };
        let mut entries = Vec::new();
        if self.options.dot_entries {
            entries.extend(dot_entries(directory));
        }
        for item in items {
            match item {
                Ok(item) => entries.extend(self.dir_entry(&item)),
                Err(error) => {
                    eprintln!(
                        "ls: cannot read directory '{}': {}",
                        display_path(directory),
                        io_error_text(&error)
                    );
                    self.failed = true;
                }
            }
        }
        Some(entries)
    }

    fn dir_entry(&mut self, item: &fs::DirEntry) -> Option<Entry> {
        let name = item.file_name().to_string_lossy().into_owned();
        if !self.options.all && name.starts_with('.') {
            return None;
        }
        let path = item.path();
        match item.metadata() {
            Ok(metadata) => Some(Entry {
                name,
                path,
                metadata,
            }),
            Err(error) => {
                eprintln!(
                    "ls: cannot inspect '{}': {}",
                    display_path(&path),
                    io_error_text(&error)
                );
                self.failed = true;
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
            long_lines(entries, &self.options)
        } else {
            short_lines(entries, &self.options)
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

fn short_lines(entries: &[Entry], options: &LsOptions) -> Vec<String> {
    entries
        .iter()
        .map(|entry| format!("{}{}", entry.name, indicator(entry, options)))
        .collect()
}

fn long_lines(entries: &[Entry], options: &LsOptions) -> Vec<String> {
    let sizes: Vec<String> = entries
        .iter()
        .map(|entry| size_text(&entry.metadata, options.human))
        .collect();
    let size_width = sizes.iter().map(String::len).max().unwrap_or(0);
    let link_width = entries
        .iter()
        .map(|entry| inode_nlink(&entry.metadata).1.to_string().len())
        .max()
        .unwrap_or(1);
    entries
        .iter()
        .zip(&sizes)
        .map(|(entry, size)| {
            let modified = modified_time(&entry.metadata);
            format!(
                "{} {:>link_width$} - - {:>size_width$} {} {}",
                mode_string(&entry.path, &entry.metadata),
                inode_nlink(&entry.metadata).1,
                size,
                format_time(modified, options.time_style),
                long_name(entry, options)
            )
        })
        .collect()
}

/// Name plus classify mark, followed by ` -> target` for symlinks in long format.
fn long_name(entry: &Entry, options: &LsOptions) -> String {
    let mut text = format!("{}{}", entry.name, indicator(entry, options));
    if entry.metadata.file_type().is_symlink() {
        if let Ok(target) = fs::read_link(&entry.path) {
            text.push_str(" -> ");
            text.push_str(&display_path(&target));
        }
    }
    text
}

fn indicator(entry: &Entry, options: &LsOptions) -> &'static str {
    if !options.classify && !options.slash {
        return "";
    }
    let file_type = entry.metadata.file_type();
    if file_type.is_dir() {
        return "/";
    }
    if !options.classify {
        return "";
    }
    if file_type.is_symlink() {
        "@"
    } else if is_executable(&entry.path, &entry.metadata) {
        "*"
    } else {
        ""
    }
}

fn sort_entries(entries: &mut [Entry], options: &LsOptions) {
    entries.sort_by(|left, right| compare_entries(left, right, options.sort));
    if options.reverse {
        entries.reverse();
    }
    if options.group_dirs_first {
        entries.sort_by_key(|entry| !entry.is_real_dir());
    }
}

fn compare_entries(left: &Entry, right: &Entry, sort: SortKey) -> Ordering {
    match sort {
        SortKey::Name => left.name.cmp(&right.name),
        SortKey::Natural => natural_cmp(&left.name, &right.name),
        SortKey::Time => modified_time(&right.metadata)
            .cmp(&modified_time(&left.metadata))
            .then_with(|| left.name.cmp(&right.name)),
        SortKey::Size => right
            .metadata
            .len()
            .cmp(&left.metadata.len())
            .then_with(|| left.name.cmp(&right.name)),
        SortKey::Unsorted => Ordering::Equal,
    }
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

fn modified_time(metadata: &fs::Metadata) -> SystemTime {
    metadata.modified().unwrap_or(UNIX_EPOCH)
}

fn size_text(metadata: &fs::Metadata, human: bool) -> String {
    if human {
        human_size(metadata.len())
    } else {
        metadata.len().to_string()
    }
}

/// Formats a timestamp in UTC without a date-time crate.
fn format_time(time: SystemTime, style: TimeStyle) -> String {
    let seconds = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            let secs = i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX);
            -secs
        }
    };
    let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
    let second_of_day = seconds.rem_euclid(SECONDS_PER_DAY);
    let hour = second_of_day / 3600;
    let minute = second_of_day % 3600 / 60;
    let second = second_of_day % 60;
    match style {
        TimeStyle::Minutes => format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}"),
        TimeStyle::Seconds => {
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} +0000")
        }
    }
}

/// Converts days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(unix)]
fn is_executable(_path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(path: &Path, metadata: &fs::Metadata) -> bool {
    const EXECUTABLE_EXTENSIONS: [&str; 5] = ["exe", "bat", "cmd", "com", "ps1"];
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    metadata.is_file()
        && EXECUTABLE_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
}

#[cfg(test)]
mod ls_tests {
    use super::{civil_from_days, human_size, natural_cmp};
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
    fn civil_dates_match_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(20_391), (2025, 10, 30));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn natural_order_compares_numbers_by_value() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("file010", "file10"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "b"), Ordering::Less);
    }
}
