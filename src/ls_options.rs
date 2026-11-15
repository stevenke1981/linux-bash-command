use super::{Layout, LsOptions, SortKey, Style, TimeStyle, LS_USAGE};
use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    color,
    error::usage,
};
use crate::find_command::glob::Glob;
use std::path::PathBuf;

pub(super) const HELP: &str = "Usage: ls [OPTION]... [PATH]...\nList directory entries.\n  -a, --all                   include . and .. and hidden names\n  -A, --almost-all            include hidden names without . and ..\n  -f                          do not sort; include hidden names\n  -l, -n, -o, -g              long listing (owner/group shown as '-')\n  -h, --human-readable        sizes in powers of 1024\n      --si                    sizes in powers of 1000\n  -F, --classify              append file type indicators\n  -p                          append / to directories\n      --file-type             append indicators except *\n      --indicator-style=WORD  none, slash, file-type, classify\n  -d, --directory             list directories themselves\n  -R, --recursive             list subdirectories recursively\n  -t -S -v -U -X              sort by time, size, version, none, extension\n      --sort=WORD             time, size, version, none, extension\n  -r, --reverse               reverse sorting\n      --group-directories-first\n                              list directories first\n  -c -u                       select status change or access time\n      --time=WORD             mtime/modification, atime/access/use, ctime/status\n      --full-time             long format with full timestamps\n      --time-style=STYLE      full-iso, long-iso, iso, locale, +FORMAT\n  -i, --inode                 print inode numbers\n  -s, --size                  print allocated blocks\n  -k                          allocated sizes in 1024-byte blocks\n      --block-size=SIZE       scale sizes by SIZE\n  -I, --ignore=PATTERN        ignore matching names\n  -B, --ignore-backups        ignore names ending in ~\n      --hide=PATTERN          hide names unless -a or -A is given\n  -Q, --quote-name            quote names as C strings\n  -N, --literal               print names literally\n      --quoting-style=WORD    literal, shell, shell-always, shell-escape, c, escape\n  -m                          comma-separated output\n  -C -x -1                    columns, across rows, or one entry per line\n  -L, --dereference           follow all symbolic links\n  -H, --dereference-command-line\n                              follow command-line symbolic links\n  -w, --width=COLS            set output width (0 means unlimited)\n  -T, --tabsize=COLS          accepted\n  -G                          accepted and ignored\n      --color[=WHEN]          always, auto, or never\n      --colour[=WHEN]         alias for --color\n      --show-control-chars    accepted and ignored\n      --help                  display this help\n      --version               output version information\nExit status: 0 on success, 1 on minor errors, 2 on serious errors.\n";

#[derive(Clone, Copy, Default)]
pub(super) enum Clock {
    #[default]
    Modified,
    Accessed,
    Changed,
    Birth,
}

fn short(id: &'static str, short: char) -> OptSpec {
    OptSpec {
        id,
        short,
        long: "",
        has_arg: HasArg::No,
    }
}

pub(super) fn parse(args: &[String]) -> Result<(LsOptions, Vec<PathBuf>), i32> {
    let specs = [
        OptSpec::flag("all", 'a'),
        OptSpec::flag("almost-all", 'A'),
        short("unsorted-all", 'f'),
        short("long", 'l'),
        short("numeric", 'n'),
        short("no-group", 'o'),
        short("no-owner", 'g'),
        OptSpec::flag("human-readable", 'h'),
        OptSpec::flag("classify", 'F'),
        short("slash", 'p'),
        OptSpec::flag("directory", 'd'),
        OptSpec::flag("recursive", 'R'),
        short("sort-time", 't'),
        short("sort-size", 'S'),
        short("sort-natural", 'v'),
        short("sort-none", 'U'),
        short("sort-extension", 'X'),
        OptSpec::flag("reverse", 'r'),
        OptSpec::flag("group-directories-first", '\0'),
        OptSpec::flag("full-time", '\0'),
        OptSpec::value("time-style", '\0'),
        OptSpec::value("time", '\0'),
        short("columns", 'C'),
        short("across", 'x'),
        short("one", '1'),
        short("ignore-group", 'G'),
        OptSpec::optional("color", '\0'),
        OptSpec::optional("colour", '\0'),
        OptSpec::flag("show-control-chars", '\0'),
        OptSpec::flag("inode", 'i'),
        short("ctime", 'c'),
        short("atime", 'u'),
        OptSpec::value("sort", '\0'),
        OptSpec::value("ignore", 'I'),
        OptSpec::flag("ignore-backups", 'B'),
        OptSpec::value("hide", '\0'),
        OptSpec::flag("quote-name", 'Q'),
        OptSpec::flag("literal", 'N'),
        OptSpec::value("quoting-style", '\0'),
        short("comma", 'm'),
        OptSpec::flag("dereference", 'L'),
        OptSpec::flag("dereference-command-line", 'H'),
        OptSpec::value("width", 'w'),
        OptSpec::value("tabsize", 'T'),
        OptSpec::flag("size", 's'),
        short("kibibytes", 'k'),
        OptSpec::value("block-size", '\0'),
        OptSpec::flag("si", '\0'),
        OptSpec::value("indicator-style", '\0'),
        OptSpec::flag("file-type", '\0'),
        short("dired", 'D'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = args::parse(args, &specs, true).map_err(|error| usage("ls", error, 2))?;
    let mut options = LsOptions::default();
    let mut paths = Vec::new();
    let (mut explicit_sort, mut short_clock) = (false, false);
    for item in items {
        if let Item::Opt(id @ ("help" | "version"), _) = &item {
            return Err(args::help_version("ls", &[format!("--{id}")], LS_USAGE, true).unwrap());
        }
        let result: Result<(), String> = (|| {
            match item {
                Item::Operand(path) => paths.push(PathBuf::from(path)),
                Item::Opt("all", _) => {
                    options.all = true;
                    options.dot_entries = true;
                }
                Item::Opt("almost-all", _) => options.all = true,
                Item::Opt("unsorted-all", _) => {
                    options.all = true;
                    options.dot_entries = true;
                    options.sort = SortKey::Unsorted;
                    explicit_sort = true;
                }
                Item::Opt("long" | "numeric" | "no-group" | "no-owner", _) => options.long = true,
                Item::Opt("human-readable", _) => {
                    options.human = true;
                    options.si = false;
                    options.block_size = None;
                    options.allocated_block = None;
                }
                Item::Opt("classify", _) => {
                    options.classify = true;
                    options.slash = false;
                    options.file_type = false;
                }
                Item::Opt("slash", _) => {
                    options.slash = true;
                    options.classify = false;
                    options.file_type = false;
                }
                Item::Opt("directory", _) => options.directory = true,
                Item::Opt("recursive", _) => options.recursive = true,
                Item::Opt(
                    "sort-time" | "sort-size" | "sort-natural" | "sort-none" | "sort-extension",
                    _,
                ) => {
                    explicit_sort = true;
                    options.sort = match item_id(&item) {
                        "sort-time" => SortKey::Time,
                        "sort-size" => SortKey::Size,
                        "sort-natural" => SortKey::Natural,
                        "sort-none" => SortKey::Unsorted,
                        _ => SortKey::Extension,
                    };
                }
                Item::Opt("reverse", _) => options.reverse = true,
                Item::Opt("group-directories-first", _) => options.group_dirs_first = true,
                Item::Opt("full-time", _) => {
                    options.long = true;
                    options.time_style = TimeStyle::Seconds;
                    options.time_format = None;
                }
                Item::Opt("time-style", Some(value)) => {
                    options.time_format = match value.as_str() {
                        "iso" | "locale" => {
                            options.time_style = if value == "iso" {
                                TimeStyle::Iso
                            } else {
                                TimeStyle::Locale
                            };
                            None
                        }
                        _ => Some(match value.as_str() {
                            "full-iso" => "%Y-%m-%d %H:%M:%S.%N %z".into(),
                            "long-iso" => "%Y-%m-%d %H:%M".into(),
                            _ if value.starts_with('+') => value[1..].into(),
                            _ => {
                                return Err(format!("invalid argument '{value}' for 'time-style'"))
                            }
                        }),
                    };
                }
                Item::Opt("time", Some(value)) => {
                    options.clock = match value.as_str() {
                        "mtime" | "modification" => Clock::Modified,
                        "atime" | "access" | "use" => Clock::Accessed,
                        "ctime" | "status" => Clock::Changed,
                        "birth" | "creation" => Clock::Birth,
                        _ => return Err(format!("invalid argument '{value}' for 'time'")),
                    }
                }
                Item::Opt("ctime", _) => {
                    options.clock = Clock::Changed;
                    short_clock = true;
                }
                Item::Opt("atime", _) => {
                    options.clock = Clock::Accessed;
                    short_clock = true;
                }
                Item::Opt("columns", _) => {
                    options.layout = Layout::Columns;
                    options.comma = false;
                }
                Item::Opt("across", _) => {
                    options.layout = Layout::Across;
                    options.comma = false;
                }
                Item::Opt("one", _) => {
                    options.layout = Layout::OneLine;
                    options.comma = false;
                }
                Item::Opt("ignore-group" | "show-control-chars", _) => {}
                Item::Opt("color" | "colour", value) => {
                    options.color = Some(color::parse_when(value.as_deref(), color::When::Always)?)
                }
                Item::Opt("inode", _) => options.inode = true,
                Item::Opt("sort", Some(value)) => {
                    explicit_sort = true;
                    options.sort = match value.as_str() {
                        "none" => SortKey::Unsorted,
                        "size" => SortKey::Size,
                        "time" => SortKey::Time,
                        "version" => SortKey::Natural,
                        "extension" => SortKey::Extension,
                        _ => return Err(format!("invalid argument '{value}' for 'sort'")),
                    };
                }
                Item::Opt("ignore", Some(value)) => options.ignore.push(Glob::new(&value, false)),
                Item::Opt("hide", Some(value)) => options.hide.push(Glob::new(&value, false)),
                Item::Opt("ignore-backups", _) => options.backups = true,
                Item::Opt("quote-name", _) => options.quote = super::ls_output::Quoting::C,
                Item::Opt("literal", _) => options.quote = super::ls_output::Quoting::Literal,
                Item::Opt("quoting-style", Some(value)) => {
                    options.quote = super::ls_output::Quoting::parse(&value)?
                }
                Item::Opt("comma", _) => {
                    options.comma = true;
                    options.long = false;
                }
                Item::Opt("dereference", _) => options.follow = 2,
                Item::Opt("dereference-command-line", _) => options.follow = 1,
                Item::Opt("width", Some(value)) => {
                    options.width = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| format!("invalid line width '{value}'"))?,
                    )
                }
                Item::Opt("tabsize", Some(value)) => {
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("invalid tab size '{value}'"))?;
                }
                Item::Opt("size", _) => options.blocks = true,
                Item::Opt("kibibytes", _) => options.allocated_block = Some(1024),
                Item::Opt("block-size", Some(value)) => {
                    let size = crate::commands::du::parse_size(&value)?;
                    if size == 0 {
                        return Err("invalid block size '0'".into());
                    }
                    options.block_size = Some(size);
                    options.allocated_block = Some(size);
                    options.human = false;
                    options.si = false;
                }
                Item::Opt("si", _) => {
                    options.human = true;
                    options.si = true;
                    options.block_size = None;
                    options.allocated_block = None;
                }
                Item::Opt("indicator-style", Some(value)) => {
                    options.classify = false;
                    options.slash = false;
                    options.file_type = false;
                    match value.as_str() {
                        "none" => {}
                        "slash" => options.slash = true,
                        "file-type" => options.file_type = true,
                        "classify" => options.classify = true,
                        _ => {
                            return Err(format!("invalid argument '{value}' for 'indicator-style'"))
                        }
                    }
                }
                Item::Opt("file-type", _) => {
                    options.file_type = true;
                    options.classify = false;
                    options.slash = false;
                }
                Item::Opt("dired", _) => {
                    return Err("option '-D' is not supported by agentcmd".into())
                }
                _ => unreachable!(),
            }
            Ok(())
        })();
        if let Err(error) = result {
            return Err(usage("ls", error, 2));
        }
    }
    if short_clock && !options.long && !explicit_sort {
        options.sort = SortKey::Time;
    }
    if paths.is_empty() {
        paths.push(".".into());
    }
    options.style = Style::new(color::enabled(options.color));
    Ok((options, paths))
}

fn item_id(item: &Item) -> &str {
    match item {
        Item::Opt(id, _) => id,
        _ => unreachable!(),
    }
}
