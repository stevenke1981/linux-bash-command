//! `du`: estimates file space usage. Directories are printed after their contents, in
//! directory-read order, as GNU does. Block usage is `st_blocks * 512` on Unix; the
//! apparent size (`-b`, `--apparent-size`) counts file bytes only.

mod format;
mod pattern;

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use jiff::tz::TimeZone;

use crate::commands::stat::platform::{raw_stat, RawStat, S_IFDIR, S_IFMT};
use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::datetime::zone_from_env;
use crate::common::error::{io_error_text, print_version, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::path::display_path;
use format::{parse_size, size_text, time_text, Scale, TimeStyle};

const HELP: &str = r#"Usage: du [OPTION]... [FILE]...
Summarize disk usage of the set of FILEs, recursively for directories.

Mandatory arguments to long options are mandatory for short options too.
  -0, --null          end each output line with NUL, not newline
  -a, --all           write counts for all files, not just directories
      --apparent-size  print apparent sizes, rather than disk usage; although
                         the apparent size is usually smaller, it may be
                         larger due to holes in ('sparse') files, internal
                         fragmentation, indirect blocks, and the like
  -B, --block-size=SIZE  scale sizes by SIZE before printing them; e.g.,
                         '-BM' prints sizes in units of 1,048,576 bytes;
                         see SIZE format below
  -b, --bytes         equivalent to '--apparent-size --block-size=1'
  -c, --total         produce a grand total
  -D, -H, --dereference-args  dereference only symlinks that are listed on the
                         command line
  -d, --max-depth=N   print the total for a directory (or file, with --all)
                         only if it is N or fewer levels below the command
                         line argument;  --max-depth=0 is the same as
                         --summarize
  -h, --human-readable  print sizes in human readable format (e.g., 1K 234M 2G)
      --si            like -h, but use powers of 1000 not 1024
  -k                  like --block-size=1K
  -L, --dereference   dereference all symbolic links
  -l, --count-links   count sizes many times if hard linked
  -m                  like --block-size=1M
  -P, --no-dereference  don't follow any symbolic links (this is the default)
  -S, --separate-dirs for directories do not include size of subdirectories
  -s, --summarize     display only a total for each argument
  -t, --threshold=SIZE  exclude entries smaller than SIZE if positive, or
                         entries greater than SIZE if negative
      --time          show time of the last modification of any file in the
                         directory, or any of its subdirectories
      --time=WORD     show time as WORD instead of modification time:
                         atime, access, use, ctime, status, birth or creation
      --time-style=STYLE  show times using style STYLE: full-iso, long-iso,
                         iso, +FORMAT
  -x, --one-file-system  skip directories on different file systems
  -X, --exclude-from=FILE  exclude files that match any pattern in FILE
      --exclude=PATTERN  exclude files that match PATTERN
      --help     display this help and exit
      --version  output version information and exit

Display values are in units of the first available SIZE from --block-size,
and the DU_BLOCK_SIZE, BLOCK_SIZE and BLOCKSIZE environment variables.
Otherwise, units default to 1024 bytes (or 512 if POSIXLY_CORRECT is set).

SIZE is an integer and optional unit (example: 10M is 10*1024*1024).  Units
are K, M, G, T, P, E (powers of 1024) or KB, MB, ... (powers of 1000).
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("null", Some('0'), Some("null")),
    OptSpec::flag("all", Some('a'), Some("all")),
    OptSpec::flag("apparent", None, Some("apparent-size")),
    OptSpec::req("block", Some('B'), Some("block-size")),
    OptSpec::flag("bytes", Some('b'), Some("bytes")),
    OptSpec::flag("total", Some('c'), Some("total")),
    OptSpec::flag("deref-args", Some('D'), None),
    OptSpec::flag("deref-args", Some('H'), Some("dereference-args")),
    OptSpec::req("depth", Some('d'), Some("max-depth")),
    OptSpec::flag("human", Some('h'), Some("human-readable")),
    OptSpec::flag("inodes", None, Some("inodes")),
    OptSpec::flag("k", Some('k'), None),
    OptSpec::flag("deref", Some('L'), Some("dereference")),
    OptSpec::flag("links", Some('l'), Some("count-links")),
    OptSpec::flag("m", Some('m'), None),
    OptSpec::flag("no-deref", Some('P'), Some("no-dereference")),
    OptSpec::flag("separate", Some('S'), Some("separate-dirs")),
    OptSpec::flag("si", None, Some("si")),
    OptSpec::flag("summarize", Some('s'), Some("summarize")),
    OptSpec::req("threshold", Some('t'), Some("threshold")),
    OptSpec::opt("time", None, Some("time")),
    OptSpec::req("time-style", None, Some("time-style")),
    OptSpec::req("exclude-from", Some('X'), Some("exclude-from")),
    OptSpec::req("exclude", None, Some("exclude")),
    OptSpec::flag("one-fs", Some('x'), Some("one-file-system")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Which symbolic links are followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Deref {
    /// `-P`, the default: never.
    Never,
    /// `-D`/`-H`: only command-line operands.
    Operands,
    /// `-L`: everywhere.
    All,
}

/// The file time shown by `--time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeField {
    Modified,
    Accessed,
    Changed,
    Born,
}

/// Everything the options select.
#[derive(Debug)]
struct Config {
    all: bool,
    apparent: bool,
    scale: Scale,
    count_links: bool,
    one_file_system: bool,
    deref: Deref,
    max_depth: Option<u64>,
    separate_dirs: bool,
    summarize: bool,
    total: bool,
    /// `-t`: the size and whether it was negative.
    threshold: Option<(bool, u64)>,
    time: Option<TimeField>,
    time_style: TimeStyle,
    excludes: Vec<String>,
    nul: bool,
    operands: Vec<String>,
}

/// Entry point.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("du", SPECS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage("du", &err, EXIT_FAIL),
    };
    let config = match configure(&items) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let zone = zone_from_env();
    let mut walk = Walk {
        config: &config,
        out: stdout_writer(),
        zone,
        seen: HashSet::new(),
        ancestors: Vec::new(),
        status: EXIT_OK,
    };
    let mut grand_total = 0u64;
    let operands: Vec<String> = if config.operands.is_empty() {
        vec![".".to_string()]
    } else {
        config.operands.clone()
    };
    for operand in &operands {
        grand_total += walk.top(Path::new(operand)).unwrap_or(0);
    }
    if config.total && walk.write_line(grand_total, "total", None).is_err() {
        return EXIT_FAIL;
    }
    let flushed = finish_output("du", &mut walk.out);
    if walk.status != EXIT_OK {
        walk.status
    } else {
        flushed
    }
}

/// Applies the options. Returns the exit status to use when parsing ends the run early.
fn configure(items: &[Item]) -> Result<Config, i32> {
    let mut config = Config {
        all: false,
        apparent: false,
        scale: Scale::Unit(1024),
        count_links: false,
        one_file_system: false,
        deref: Deref::Never,
        max_depth: None,
        separate_dirs: false,
        summarize: false,
        total: false,
        threshold: None,
        time: None,
        time_style: TimeStyle::Long,
        excludes: Vec::new(),
        nul: false,
        operands: Vec::new(),
    };
    for item in items {
        match item {
            Item::Operand(text) => config.operands.push(text.clone()),
            Item::Opt { id, value } => apply(&mut config, id, value.as_deref())?,
        }
    }
    Ok(config)
}

fn apply(config: &mut Config, id: &str, value: Option<&str>) -> Result<(), i32> {
    let text = value.unwrap_or_default();
    match id {
        "help" => {
            print!("{HELP}");
            return Err(EXIT_OK);
        }
        "version" => return Err(print_version("du")),
        "inodes" => return Err(unsupported("du", "--inodes")),
        "null" => config.nul = true,
        "all" => config.all = true,
        "apparent" => config.apparent = true,
        "bytes" => {
            config.apparent = true;
            config.scale = Scale::Bytes;
        }
        "block" => config.scale = Scale::Unit(parse_block(text)?),
        "total" => config.total = true,
        "deref-args" => config.deref = Deref::Operands,
        "depth" => config.max_depth = Some(parse_depth(text)?),
        "human" => config.scale = Scale::Human(1024),
        "si" => config.scale = Scale::Human(1000),
        "k" => config.scale = Scale::Unit(1024),
        "m" => config.scale = Scale::Unit(1024 * 1024),
        "deref" => config.deref = Deref::All,
        "links" => config.count_links = true,
        "no-deref" => config.deref = Deref::Never,
        "separate" => config.separate_dirs = true,
        "summarize" => config.summarize = true,
        "threshold" => config.threshold = Some(parse_threshold(text)?),
        "time" => config.time = Some(parse_time_field(value)?),
        "time-style" => config.time_style = parse_time_style(text)?,
        "exclude" => config.excludes.push(text.to_string()),
        "exclude-from" => config.excludes.extend(read_patterns(text)?),
        "one-fs" => config.one_file_system = true,
        _ => {}
    }
    Ok(())
}

fn parse_block(text: &str) -> Result<u64, i32> {
    match parse_size(text) {
        Some(size) if size > 0 => Ok(size),
        _ => Err(invalid_option(text, "--block-size")),
    }
}

fn parse_depth(text: &str) -> Result<u64, i32> {
    text.parse().map_err(|_| {
        eprintln!("du: invalid maximum depth '{text}'");
        EXIT_FAIL
    })
}

fn parse_threshold(text: &str) -> Result<(bool, u64), i32> {
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    match parse_size(body) {
        Some(size) => Ok((negative, size)),
        None => Err(invalid_option(text, "--threshold")),
    }
}

fn parse_time_field(value: Option<&str>) -> Result<TimeField, i32> {
    Ok(match value {
        None => TimeField::Modified,
        Some("atime" | "access" | "use") => TimeField::Accessed,
        Some("ctime" | "status") => TimeField::Changed,
        Some("birth" | "creation") => TimeField::Born,
        Some(other) => return Err(invalid_option(other, "--time")),
    })
}

fn parse_time_style(text: &str) -> Result<TimeStyle, i32> {
    TimeStyle::parse(text).ok_or_else(|| invalid_option(text, "--time-style"))
}

fn invalid_option(value: &str, option: &str) -> i32 {
    eprintln!(
        "du: invalid argument '{value}' for '{option}'\nTry 'du --help' for more information."
    );
    EXIT_FAIL
}

/// Reads one exclusion pattern per line from `path` (`-` is standard input).
fn read_patterns(path: &str) -> Result<Vec<String>, i32> {
    let text = if path == "-" {
        io::read_to_string(io::stdin())
    } else {
        fs::read_to_string(path)
    };
    match text {
        Ok(text) => Ok(text
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()),
        Err(error) => {
            eprintln!(
                "du: cannot open '{path}' for reading: {}",
                io_error_text(&error)
            );
            Err(EXIT_FAIL)
        }
    }
}

/// Walk state shared by the recursion.
struct Walk<'a> {
    config: &'a Config,
    out: io::BufWriter<io::Stdout>,
    zone: TimeZone,
    /// (device, inode) of non-directory files already counted.
    seen: HashSet<(u64, u64)>,
    /// (device, inode) of directories on the current path, for `-L` loop detection.
    ancestors: Vec<(u64, u64)>,
    status: i32,
}

impl Walk<'_> {
    /// Visits one command-line operand. Returns its total, or `None` when it was skipped.
    fn top(&mut self, operand: &Path) -> Option<u64> {
        let follow = self.config.deref != Deref::Never;
        let stat = self.stat(operand, follow)?;
        self.visit(operand, &stat, 0, stat.dev, true)
    }

    fn stat(&mut self, path: &Path, follow: bool) -> Option<RawStat> {
        let meta = if follow {
            fs::metadata(path)
        } else {
            fs::symlink_metadata(path)
        };
        match meta {
            Ok(meta) => Some(raw_stat(path, &meta)),
            Err(error) => {
                self.fail(&format!(
                    "cannot access '{}': {}",
                    display_path(path),
                    io_error_text(&error)
                ));
                None
            }
        }
    }

    fn fail(&mut self, message: &str) {
        eprintln!("du: {message}");
        self.status = EXIT_FAIL;
    }

    /// Counts `path`, whose metadata is `stat`, and everything below it.
    fn visit(
        &mut self,
        path: &Path,
        stat: &RawStat,
        depth: u64,
        root_dev: u64,
        top: bool,
    ) -> Option<u64> {
        if self.excluded(path) {
            return None;
        }
        if self.config.one_file_system && stat.dev != root_dev {
            return None;
        }
        let is_dir = is_directory(stat);
        // A directory is counted once per run, however many operands reach it. A file with
        // several links is counted once unless `-l` asks for every link.
        let counted_once = is_dir || (!self.config.count_links && stat.nlink > 1);
        if counted_once && !self.seen.insert((stat.dev, stat.ino)) {
            return Some(0);
        }
        let own = entry_bytes(stat, is_dir, self.config.apparent);
        let mut total = own;
        if is_dir {
            if self.config.deref == Deref::All && self.ancestors.contains(&(stat.dev, stat.ino)) {
                return Some(0);
            }
            total += self.visit_children(path, stat, depth, root_dev);
        }
        if self.should_print(depth, top, is_dir) && !self.below_threshold(total) {
            let time = self.time_of(stat);
            if self.write_line(total, &display_path(path), time).is_err() {
                self.status = EXIT_FAIL;
            }
        }
        Some(total)
    }

    /// Sums the children of directory `path`, skipping subdirectory totals under `-S`.
    fn visit_children(&mut self, path: &Path, stat: &RawStat, depth: u64, root_dev: u64) -> u64 {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                self.fail(&format!(
                    "cannot read directory '{}': {}",
                    display_path(path),
                    io_error_text(&error)
                ));
                return 0;
            }
        };
        self.ancestors.push((stat.dev, stat.ino));
        let mut sum = 0;
        for entry in entries {
            let Ok(entry) = entry else {
                self.fail(&format!("cannot read directory '{}'", display_path(path)));
                break;
            };
            let child_path = path.join(entry.file_name());
            let follow = self.config.deref == Deref::All;
            let Some(child) = self.stat(&child_path, follow) else {
                continue;
            };
            let child_is_dir = is_directory(&child);
            if let Some(size) = self.visit(&child_path, &child, depth + 1, root_dev, false) {
                if !(child_is_dir && self.config.separate_dirs) {
                    sum += size;
                }
            }
        }
        self.ancestors.pop();
        sum
    }

    fn excluded(&self, path: &Path) -> bool {
        if self.config.excludes.is_empty() {
            return false;
        }
        let whole = display_path(path);
        let base = path
            .file_name()
            .map_or_else(|| whole.clone(), |name| name.to_string_lossy().into_owned());
        self.config
            .excludes
            .iter()
            .any(|pattern| pattern::matches(pattern, &base) || pattern::matches(pattern, &whole))
    }

    /// Whether an entry at `depth` is listed: directories by default, other files with `-a`
    /// or when they are operands. `-s` lists only operands.
    fn should_print(&self, depth: u64, top: bool, is_dir: bool) -> bool {
        if self.config.summarize {
            return top;
        }
        if self.config.max_depth.is_some_and(|max| depth > max) {
            return false;
        }
        is_dir || top || self.config.all
    }

    fn below_threshold(&self, total: u64) -> bool {
        match self.config.threshold {
            None => false,
            Some((false, size)) => total < size,
            Some((true, size)) => total > size,
        }
    }

    fn time_of(&self, stat: &RawStat) -> Option<(i64, u32)> {
        let field = self.config.time?;
        Some(match field {
            TimeField::Modified => stat.mtime,
            TimeField::Accessed => stat.atime,
            TimeField::Changed => stat.ctime,
            TimeField::Born => stat.btime.unwrap_or((0, 0)),
        })
    }

    /// Writes `SIZE\t[TIME\t]NAME` and the terminator.
    fn write_line(&mut self, bytes: u64, name: &str, time: Option<(i64, u32)>) -> io::Result<()> {
        let size = size_text(bytes, self.config.scale);
        let time_column = match time {
            Some(time) => format!("{}\t", time_text(time, &self.config.time_style, &self.zone)),
            None => String::new(),
        };
        let terminator = if self.config.nul { '\0' } else { '\n' };
        let result = write!(self.out, "{size}\t{time_column}{name}{terminator}");
        match result {
            Err(error) if is_broken_pipe(&error) => Ok(()),
            other => other,
        }
    }
}

/// Bytes charged for one entry. Block usage counts directories too; the apparent size
/// counts only non-directory bytes.
fn entry_bytes(stat: &RawStat, is_dir: bool, apparent: bool) -> u64 {
    if apparent {
        if is_dir {
            0
        } else {
            stat.size
        }
    } else {
        stat.blocks * 512
    }
}

fn is_directory(stat: &RawStat) -> bool {
    stat.mode & S_IFMT == S_IFDIR
}

#[cfg(test)]
mod tests;
