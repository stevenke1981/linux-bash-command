//! Option parsing for `ls`: GNU short and long options, the values of `-I`, `-T`, `-w` and
//! the `--sort`, `--time`, `--time-style`, `--quoting-style`, `--indicator-style`,
//! `--block-size`, `--hide` and `--ignore` options.

use crate::common::args::prepare_legacy;
use crate::common::error::{print_version, EXIT_FAIL, EXIT_TROUBLE};

pub const LS_USAGE: &str = "Usage: ls [OPTION]... [FILE]...
List information about the FILEs (the current directory by default).
Times are shown in the time zone named by TZ (UTC when TZ is unset and the system zone is UTC).

Selection:
  -a, --all                  Do not ignore entries starting with .
  -A, --almost-all           Do not list implied . and ..
  -B, --ignore-backups       Do not list implied entries ending with ~
  -d, --directory            List directories themselves, not their contents
  -f                         Same as -a -U
  -I, --ignore=PATTERN       Do not list implied entries matching shell PATTERN
      --hide=PATTERN         Like -I, but overridden by -a or -A
  -R, --recursive            List subdirectories recursively
  -L, --dereference          Show the target of symbolic links
  -H, --dereference-command-line
                             Follow symbolic links named on the command line

Formats:
  -l                         Long format: mode, links, owner, group, size, time, name
  -g, -o                     Like -l, without the owner or the group column
  -G, --no-group             Like -l, without the group column
  -n, --numeric-uid-gid      Like -l, with numeric user and group ids
  -i, --inode                Print the inode number of each file
  -s, --size                 Print the allocated size of each file, in blocks
  -1                         One entry per line
  -C                         Entries in columns, sorted down the columns
  -x                         Entries in columns, sorted across the rows
  -m                         Entries separated by commas
  -w, --width=COLS           Set the output width (0 means unlimited)
  -T, --tabsize=COLS         Tab stops for column padding (0 uses spaces only)
  -h, --human-readable       With -l or -s, sizes such as 512, 4.0K, 12M
      --si                   Like -h, with powers of 1000
  -k                         Block counts in 1K units (for -s)
      --block-size=SIZE      Scale sizes by SIZE: N, or K, M, G, T, P (powers of 1024)
                             or KB, MB, ... (powers of 1000); N and a suffix omit the suffix
  -p, --indicator-style=slash
                             Append / to directories
  -F, --classify[=WHEN]      Append an indicator: / directories, @ links, | FIFOs,
                             = sockets, * executables (WHEN: always, auto, never)
      --file-type            Like -F, without the * for executables
      --indicator-style=WORD  none, slash, file-type or classify
  -q, --hide-control-chars  Accepted; names are shown as stored
      --show-control-chars  Accepted; names are shown as stored
  -N, --literal              Print entry names without quoting
  -b, --escape               Like --quoting-style=escape
  -Q, --quote-name           Quote names with double quotes (--quoting-style=c)
      --quoting-style=WORD   literal, shell, shell-always, shell-escape,
                             shell-escape-always, c or escape

Sorting:
  -t                         Sort by time, newest first
  -S                         Sort by size, largest first
  -X                         Sort by extension
  -v                         Sort names in natural order (file2 before file10)
  -U                         Do not sort; use directory order
      --sort=WORD            none, size, time, version, extension or width
  -r, --reverse              Reverse the sort order
      --group-directories-first
                             List directories before files
  -c                         With -t or -l, use the status change time
  -u                         With -t or -l, use the access time
      --time=WORD            atime (access, use), ctime (status), mtime (modification)
                             or birth (creation): the time shown and sorted by
      --full-time            Like -l --time-style=full-iso
      --time-style=STYLE     full-iso, long-iso (default), iso, locale, or +FORMAT

Miscellaneous:
      --color[=WHEN]         Accepted; output is never colored
  -D, --dired                Not supported
      --help                 Show this help
      --version              Show the version

Exit status: 0 on success, 2 if a path operand could not be accessed or listed, 1 if an
entry inside a directory could not be read.
";

/// Long options that take a separate value (`--ignore PAT`), for the legacy pre-parser.
const VALUE_LONGS: &[&str] = &[
    "block-size",
    "hide",
    "ignore",
    "indicator-style",
    "quoting-style",
    "sort",
    "tabsize",
    "time",
    "time-style",
    "width",
];

/// Every long option `ls` knows, for unique-prefix expansion.
const LS_LONGS: &[&str] = &[
    "all",
    "almost-all",
    "block-size",
    "classify",
    "color",
    "dereference",
    "dereference-command-line",
    "dereference-command-line-symlink-to-dir",
    "dired",
    "directory",
    "escape",
    "file-type",
    "full-time",
    "group-directories-first",
    "help",
    "hide",
    "hide-control-chars",
    "human-readable",
    "ignore",
    "ignore-backups",
    "indicator-style",
    "inode",
    "kibibytes",
    "literal",
    "no-group",
    "numeric-uid-gid",
    "quote-name",
    "quoting-style",
    "recursive",
    "reverse",
    "show-control-chars",
    "si",
    "size",
    "sort",
    "tabsize",
    "time",
    "time-style",
    "version",
    "width",
];

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum SortKey {
    #[default]
    Name,
    Natural,
    Time,
    Size,
    Extension,
    Width,
    Unsorted,
}

/// Which timestamp is shown and sorted by.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum TimeField {
    #[default]
    Modified,
    Accessed,
    Changed,
    Birth,
}

/// How a size is printed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SizeFormat {
    /// Exact byte count.
    Bytes,
    /// Rounded up to whole `unit` bytes, followed by `suffix` (possibly empty).
    Scaled { unit: u64, suffix: &'static str },
    /// Human readable with the given base (1024 or 1000).
    Human(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Quoting {
    Literal,
    Shell,
    ShellAlways,
    ShellEscape,
    ShellEscapeAlways,
    C,
    Escape,
    /// The default when stdout is a terminal (`shell-escape`) is chosen by the caller.
    #[default]
    Unset,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Indicator {
    #[default]
    None,
    Slash,
    FileType,
    Classify,
    /// `--classify=auto`: classify only when stdout is a terminal.
    ClassifyAuto,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Format {
    /// One per line, or columns when stdout is a terminal.
    #[default]
    Auto,
    Long,
    Single,
    Columns,
    Across,
    Commas,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Deref {
    #[default]
    Never,
    /// `-H`: symbolic links named on the command line.
    CommandLine,
    /// `-L`: every symbolic link.
    All,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TimeStyle {
    /// `YYYY-MM-DD HH:MM`, the default (`long-iso`, `locale`).
    LongIso,
    /// `YYYY-MM-DD HH:MM:SS.NNNNNNNNN +ZZZZ` (`full-iso`, `--full-time`).
    FullIso,
    /// `MM-DD HH:MM` for recent times, `YYYY-MM-DD` otherwise.
    Iso,
    /// A user format given as `+FORMAT`.
    Custom(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LsOptions {
    /// `-a` (also `.` and `..`) or `-A`: dot-prefixed names are listed.
    pub all: bool,
    pub dot_entries: bool,
    pub format: Format,
    pub owner: bool,
    pub group: bool,
    pub numeric_ids: bool,
    pub inode: bool,
    pub blocks: bool,
    pub indicator: Indicator,
    pub quoting: Quoting,
    pub directory: bool,
    pub recursive: bool,
    pub reverse: bool,
    pub group_dirs_first: bool,
    pub sort: SortKey,
    pub time: TimeField,
    pub time_style: TimeStyle,
    /// Block counts (`-s`) format; default 1K units.
    pub block_format: SizeFormat,
    /// Sizes (`-l`) format; default exact bytes.
    pub size_format: SizeFormat,
    pub width: Option<usize>,
    pub tabsize: usize,
    pub ignore: Vec<String>,
    pub hide: Vec<String>,
    pub ignore_backups: bool,
    pub deref: Deref,
}

impl Default for LsOptions {
    fn default() -> Self {
        Self {
            all: false,
            dot_entries: false,
            format: Format::Auto,
            owner: true,
            group: true,
            numeric_ids: false,
            inode: false,
            blocks: false,
            indicator: Indicator::None,
            quoting: Quoting::Unset,
            directory: false,
            recursive: false,
            reverse: false,
            group_dirs_first: false,
            sort: SortKey::Name,
            time: TimeField::Modified,
            time_style: TimeStyle::LongIso,
            block_format: SizeFormat::Scaled {
                unit: 1024,
                suffix: "",
            },
            size_format: SizeFormat::Bytes,
            width: None,
            tabsize: 8,
            ignore: Vec::new(),
            hide: Vec::new(),
            ignore_backups: false,
            deref: Deref::Never,
        }
    }
}

/// Outcome of parsing: run with these options, or stop with a status (help, version, errors).
pub enum Parsed {
    Run(Box<LsOptions>, Vec<String>),
    Exit(i32),
}

/// Parses the command line. Printing of errors and `--help`/`--version` happens here.
pub fn parse(args: &[String]) -> Parsed {
    let expanded = match prepare_legacy("ls", args, LS_LONGS, VALUE_LONGS, "ITw", EXIT_TROUBLE) {
        Ok(expanded) => expanded,
        Err(code) => return Parsed::Exit(code),
    };
    let mut parser = Parser {
        options: LsOptions::default(),
        time_explicit: false,
        classify: Indicator::None,
    };
    let mut paths = Vec::new();
    let mut index = 0;
    while index < expanded.len() {
        let arg = expanded[index].as_str();
        index += 1;
        if arg == "--" {
            paths.extend(expanded[index..].iter().cloned());
            break;
        }
        if arg == "-" || !arg.starts_with('-') {
            paths.push(arg.to_string());
            continue;
        }
        let outcome = if let Some(name) = arg.strip_prefix("--") {
            parser.long(name, &expanded, &mut index)
        } else {
            parser.short_cluster(arg, &expanded, &mut index)
        };
        match outcome {
            Ok(()) => {}
            Err(Stop::Help) => {
                print!("{LS_USAGE}");
                return Parsed::Exit(0);
            }
            Err(Stop::Version) => return Parsed::Exit(print_version("ls")),
            Err(Stop::Usage(code)) => return Parsed::Exit(code),
        }
    }
    if paths.is_empty() {
        paths.push(".".to_string());
    }
    let mut options = parser.options;
    options.indicator = parser.classify;
    Parsed::Run(Box::new(options), paths)
}

/// Why option parsing stopped early.
enum Stop {
    Help,
    Version,
    /// A usage error that has already been reported; the status to return.
    Usage(i32),
}

struct Parser {
    options: LsOptions,
    /// `--time` was given, so `-c` and `-u` no longer choose the time field.
    time_explicit: bool,
    /// The indicator chosen by -p, -F, --file-type or --indicator-style.
    classify: Indicator,
}

fn usage_error(message: &str) -> Stop {
    eprintln!("ls: {message}\nTry 'ls --help' for more information.");
    Stop::Usage(EXIT_TROUBLE)
}

/// A word that is not one of an option's choices. GNU reports these with status 1 (argmatch).
fn argmatch_error(message: &str) -> Stop {
    eprintln!("ls: {message}\nTry 'ls --help' for more information.");
    Stop::Usage(EXIT_FAIL)
}

fn unsupported(option: &str) -> Stop {
    eprintln!(
        "ls: option '{option}' is not supported by agentcmd\nTry 'ls --help' for more information."
    );
    Stop::Usage(EXIT_TROUBLE)
}

impl Parser {
    fn long(&mut self, body: &str, args: &[String], index: &mut usize) -> Result<(), Stop> {
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (body, None),
        };
        match name {
            "help" => return Err(Stop::Help),
            "version" => return Err(Stop::Version),
            "all" => self.show_all(true),
            "almost-all" => self.show_all(false),
            "ignore-backups" => self.options.ignore_backups = true,
            "directory" => self.options.directory = true,
            "recursive" => self.options.recursive = true,
            "dereference" => self.options.deref = Deref::All,
            "dereference-command-line" | "dereference-command-line-symlink-to-dir" => {
                self.options.deref = Deref::CommandLine
            }
            "inode" => self.options.inode = true,
            "size" => self.options.blocks = true,
            "numeric-uid-gid" => {
                self.options.numeric_ids = true;
                self.options.format = Format::Long;
            }
            "no-group" => self.options.group = false,
            "human-readable" => self.human(1024),
            "si" => self.human(1000),
            "kibibytes" => self.options.block_format = kibi_blocks(),
            "block-size" => {
                let value = take_value(name, inline, args, index)?;
                let format = parse_block_size(&value).map_err(|message| usage_error(&message))?;
                self.options.block_format = format.clone();
                self.options.size_format = format;
            }
            "literal" => self.options.quoting = Quoting::Literal,
            "quote-name" => self.options.quoting = Quoting::C,
            "escape" => self.options.quoting = Quoting::Escape,
            "quoting-style" => {
                let value = take_value(name, inline, args, index)?;
                self.options.quoting =
                    parse_quoting(&value).map_err(|message| argmatch_error(&message))?;
            }
            "classify" => {
                self.classify = match inline {
                    None | Some("always" | "yes" | "force") => Indicator::Classify,
                    Some("auto" | "tty" | "if-tty") => Indicator::ClassifyAuto,
                    Some("never" | "no" | "none") => Indicator::None,
                    Some(other) => {
                        return Err(argmatch_error(&format!(
                            "invalid argument '{other}' for '--classify'"
                        )))
                    }
                };
            }
            "file-type" => self.classify = Indicator::FileType,
            "indicator-style" => {
                let value = take_value(name, inline, args, index)?;
                self.classify =
                    parse_indicator(&value).map_err(|message| argmatch_error(&message))?;
            }
            "color" => match inline {
                None
                | Some(
                    "always" | "yes" | "force" | "auto" | "tty" | "if-tty" | "never" | "no"
                    | "none",
                ) => {}
                Some(other) => {
                    return Err(usage_error(&format!(
                        "invalid argument '{other}' for '--color'"
                    )))
                }
            },
            "hide-control-chars" | "show-control-chars" => {}
            "hide" => {
                let value = take_value(name, inline, args, index)?;
                self.options.hide.push(value);
            }
            "ignore" => {
                let value = take_value(name, inline, args, index)?;
                self.options.ignore.push(value);
            }
            "full-time" => self.options.time_style = TimeStyle::FullIso,
            "time" => {
                let value = take_value(name, inline, args, index)?;
                self.options.time =
                    parse_time(&value).map_err(|message| argmatch_error(&message))?;
                self.time_explicit = true;
            }
            "time-style" => {
                let value = take_value(name, inline, args, index)?;
                self.options.time_style =
                    parse_time_style(&value).map_err(|message| usage_error(&message))?;
            }
            "sort" => {
                let value = take_value(name, inline, args, index)?;
                self.options.sort =
                    parse_sort(&value).map_err(|message| argmatch_error(&message))?;
            }
            "group-directories-first" => self.options.group_dirs_first = true,
            "reverse" => self.options.reverse = true,
            "width" => {
                let value = take_value(name, inline, args, index)?;
                self.set_width(&value)?;
            }
            "tabsize" => {
                let value = take_value(name, inline, args, index)?;
                self.options.tabsize =
                    parse_count(&value, "tab size").map_err(|message| usage_error(&message))?;
            }
            "dired" => return Err(unsupported("--dired")),
            _ => {
                eprintln!(
                    "ls: unrecognized option '--{body}'\nTry 'ls --help' for more information."
                );
                return Err(Stop::Usage(EXIT_TROUBLE));
            }
        }
        Ok(())
    }

    fn short_cluster(&mut self, arg: &str, args: &[String], index: &mut usize) -> Result<(), Stop> {
        let letters: Vec<char> = arg[1..].chars().collect();
        for (position, letter) in letters.iter().enumerate() {
            let rest: String = letters[position + 1..].iter().collect();
            match letter {
                'I' | 'T' | 'w' => {
                    let value = if rest.is_empty() {
                        take_value(&letter.to_string(), None, args, index)?
                    } else {
                        rest
                    };
                    self.short_value(*letter, &value)?;
                    break;
                }
                _ => self.short_flag(*letter)?,
            }
        }
        Ok(())
    }

    fn short_value(&mut self, letter: char, value: &str) -> Result<(), Stop> {
        match letter {
            'I' => self.options.ignore.push(value.to_string()),
            'T' => {
                self.options.tabsize =
                    parse_count(value, "tab size").map_err(|message| usage_error(&message))?
            }
            _ => self.set_width(value)?,
        }
        Ok(())
    }

    /// `-w COLS`: 0 means no limit.
    fn set_width(&mut self, value: &str) -> Result<(), Stop> {
        let count = parse_count(value, "line width").map_err(|message| usage_error(&message))?;
        self.options.width = Some(if count == 0 { usize::MAX } else { count });
        Ok(())
    }

    fn short_flag(&mut self, letter: char) -> Result<(), Stop> {
        match letter {
            'A' => self.show_all(false),
            'f' => {
                self.show_all(true);
                self.options.sort = SortKey::Unsorted;
            }
            'a' => self.show_all(true),
            'c' => self.choose_time(TimeField::Changed),
            'u' => self.choose_time(TimeField::Accessed),
            'h' => self.human(1024),
            'F' => self.classify = Indicator::Classify,
            'p' => self.classify = Indicator::Slash,
            'g' | 'o' => {
                if letter == 'g' {
                    self.options.owner = false;
                } else {
                    self.options.group = false;
                }
                self.options.format = Format::Long;
            }
            'n' => {
                self.options.numeric_ids = true;
                self.options.format = Format::Long;
            }
            'B' => self.options.ignore_backups = true,
            'b' => self.options.quoting = Quoting::Escape,
            'C' => self.options.format = Format::Columns,
            'd' => self.options.directory = true,
            'G' => self.options.group = false,
            'H' => self.options.deref = Deref::CommandLine,
            'i' => self.options.inode = true,
            'k' => self.options.block_format = kibi_blocks(),
            'l' => self.options.format = Format::Long,
            'L' => self.options.deref = Deref::All,
            'm' => self.options.format = Format::Commas,
            'N' => self.options.quoting = Quoting::Literal,
            'q' => {}
            'Q' => self.options.quoting = Quoting::C,
            'R' => self.options.recursive = true,
            'r' => self.options.reverse = true,
            's' => self.options.blocks = true,
            'S' => self.options.sort = SortKey::Size,
            't' => self.options.sort = SortKey::Time,
            'U' => self.options.sort = SortKey::Unsorted,
            'v' => self.options.sort = SortKey::Natural,
            'X' => self.options.sort = SortKey::Extension,
            'x' => self.options.format = Format::Across,
            '1' => self.options.format = Format::Single,
            'D' => return Err(unsupported("-D")),
            'Z' => return Err(unsupported("-Z")),
            _ => {
                eprintln!(
                    "ls: invalid option -- '{letter}'\nTry 'ls --help' for more information."
                );
                return Err(Stop::Usage(EXIT_TROUBLE));
            }
        }
        Ok(())
    }

    fn show_all(&mut self, dot_entries: bool) {
        self.options.all = true;
        self.options.dot_entries = dot_entries;
    }

    /// `-c` and `-u` pick the time field unless `--time` was given.
    fn choose_time(&mut self, field: TimeField) {
        if !self.time_explicit {
            self.options.time = field;
        }
    }

    /// `-h` and `--si`: sizes and block counts are human readable.
    fn human(&mut self, base: u64) {
        self.options.block_format = SizeFormat::Human(base);
        self.options.size_format = SizeFormat::Human(base);
    }
}

fn kibi_blocks() -> SizeFormat {
    SizeFormat::Scaled {
        unit: 1024,
        suffix: "",
    }
}

/// The value of an option that needs one: `--name=VALUE`, or the next argument.
fn take_value(
    name: &str,
    inline: Option<&str>,
    args: &[String],
    index: &mut usize,
) -> Result<String, Stop> {
    if let Some(value) = inline {
        return Ok(value.to_string());
    }
    let value = args
        .get(*index)
        .cloned()
        .ok_or_else(|| usage_error(&format!("option requires an argument -- '{name}'")))?;
    *index += 1;
    Ok(value)
}

/// A non-negative count for `-w` or `-T`.
fn parse_count(text: &str, what: &str) -> Result<usize, String> {
    text.parse::<usize>()
        .map_err(|_| format!("invalid {what}: '{text}'"))
}

fn parse_time(word: &str) -> Result<TimeField, String> {
    match word {
        "atime" | "access" | "use" => Ok(TimeField::Accessed),
        "ctime" | "status" => Ok(TimeField::Changed),
        "mtime" | "modification" => Ok(TimeField::Modified),
        "birth" | "creation" => Ok(TimeField::Birth),
        _ => Err(format!("invalid argument '{word}' for '--time'")),
    }
}

fn parse_sort(word: &str) -> Result<SortKey, String> {
    match word {
        "none" => Ok(SortKey::Unsorted),
        "size" => Ok(SortKey::Size),
        "time" => Ok(SortKey::Time),
        "version" => Ok(SortKey::Natural),
        "extension" => Ok(SortKey::Extension),
        "width" => Ok(SortKey::Width),
        _ => Err(format!("invalid argument '{word}' for '--sort'")),
    }
}

fn parse_quoting(word: &str) -> Result<Quoting, String> {
    match word {
        "literal" => Ok(Quoting::Literal),
        "shell" => Ok(Quoting::Shell),
        "shell-always" => Ok(Quoting::ShellAlways),
        "shell-escape" => Ok(Quoting::ShellEscape),
        "shell-escape-always" => Ok(Quoting::ShellEscapeAlways),
        "c" => Ok(Quoting::C),
        "escape" => Ok(Quoting::Escape),
        _ => Err(format!("invalid argument '{word}' for '--quoting-style'")),
    }
}

fn parse_indicator(word: &str) -> Result<Indicator, String> {
    match word {
        "none" => Ok(Indicator::None),
        "slash" => Ok(Indicator::Slash),
        "file-type" => Ok(Indicator::FileType),
        "classify" => Ok(Indicator::Classify),
        _ => Err(format!("invalid argument '{word}' for '--indicator-style'")),
    }
}

fn parse_time_style(word: &str) -> Result<TimeStyle, String> {
    if let Some(format) = word.strip_prefix('+') {
        return Ok(TimeStyle::Custom(format.to_string()));
    }
    match word {
        "full-iso" => Ok(TimeStyle::FullIso),
        "long-iso" | "locale" => Ok(TimeStyle::LongIso),
        "iso" => Ok(TimeStyle::Iso),
        _ => Err(format!("invalid argument '{word}' for '--time-style'")),
    }
}

/// `--block-size=SIZE`. A leading count is used as the unit (`1K` is 1024 bytes, printed as
/// numbers); a bare unit letter also prints that suffix (`K`, `KB` as `kB`).
pub fn parse_block_size(text: &str) -> Result<SizeFormat, String> {
    const UNITS: [(&str, u64, &str); 10] = [
        ("KB", 1000, "kB"),
        ("MB", 1000 * 1000, "MB"),
        ("GB", 1000 * 1000 * 1000, "GB"),
        ("TB", 1000u64.pow(4), "TB"),
        ("PB", 1000u64.pow(5), "PB"),
        ("K", 1024, "K"),
        ("M", 1024 * 1024, "M"),
        ("G", 1024 * 1024 * 1024, "G"),
        ("T", 1024u64.pow(4), "T"),
        ("P", 1024u64.pow(5), "P"),
    ];
    let invalid = || format!("invalid --block-size argument '{text}'");
    let digits_end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (count_text, unit_text) = text.split_at(digits_end);
    let count = if count_text.is_empty() {
        1
    } else {
        count_text.parse::<u64>().map_err(|_| invalid())?
    };
    let (factor, suffix) = if unit_text.is_empty() {
        (1, "")
    } else {
        UNITS
            .iter()
            .find(|(name, _, _)| *name == unit_text || unit_text == format!("{name}B").as_str())
            .map(|(_, factor, suffix)| (*factor, *suffix))
            .ok_or_else(invalid)?
    };
    let unit = count
        .checked_mul(factor)
        .filter(|unit| *unit > 0)
        .ok_or_else(invalid)?;
    let suffix = if count_text.is_empty() { suffix } else { "" };
    Ok(SizeFormat::Scaled { unit, suffix })
}

#[cfg(test)]
#[path = "ls_options_tests.rs"]
mod tests;
