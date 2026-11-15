use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    datetime,
    error::{io_text, usage},
    io::output,
    meta,
    size::human,
};
use crate::find_command::glob::Glob;
use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const HELP: &str = "Usage: du [OPTION]... [FILE]...\nSummarize disk usage of each file, recursively for directories.\n  -0, --null                 end each output with NUL\n  -a, --all                  write counts for all files\n      --apparent-size        use apparent sizes instead of allocated blocks\n  -B, --block-size=SIZE      scale sizes by SIZE bytes\n  -b, --bytes                equivalent to --apparent-size --block-size=1\n  -c, --total                produce a grand total\n  -D, -H, --dereference-args follow symbolic links on the command line\n  -d, --max-depth=N          print directories at most N levels deep\n  -h, --human-readable       print sizes in powers of 1024\n  -k                         use 1024-byte blocks\n  -L, --dereference          follow all symbolic links\n  -l, --count-links          count hard links repeatedly\n  -m                         use 1048576-byte blocks\n  -P, --no-dereference       never follow symbolic links (default)\n  -S, --separate-dirs        exclude subdirectories from directory sizes\n      --si                   print sizes in powers of 1000\n  -s, --summarize            display only totals for operands\n  -t, --threshold=SIZE       include sizes >= SIZE (or <= -SIZE)\n      --time[=WORD]          show latest mtime, atime/access, or ctime/status\n      --time-style=STYLE     full-iso, long-iso, iso, or +FORMAT\n  -X, --exclude-from=FILE    exclude patterns read from FILE\n      --exclude=PATTERN      exclude matching files\n  -x, --one-file-system      skip directories on different file systems\n      --help                 display this help\n      --version              output version information\n";

#[derive(Clone, Copy, PartialEq)]
enum Follow {
    Never,
    Args,
    Always,
}
#[derive(Clone, Copy)]
enum Scale {
    Blocks(u64),
    Human(u64),
}
#[derive(Clone, Copy)]
enum Clock {
    Modified,
    Accessed,
    Changed,
}
struct Options {
    delimiter: u8,
    all: bool,
    apparent: bool,
    scale: Scale,
    total: bool,
    follow: Follow,
    depth: usize,
    links: bool,
    separate: bool,
    threshold: Option<(bool, u64)>,
    clock: Option<Clock>,
    time_style: String,
    excludes: Vec<Glob>,
    one_fs: bool,
    paths: Vec<PathBuf>,
}

pub(crate) fn parse_size(text: &str) -> Result<u64, String> {
    const UNITS: &[(&str, u64)] = &[
        ("KiB", 1 << 10),
        ("MiB", 1 << 20),
        ("GiB", 1 << 30),
        ("TiB", 1 << 40),
        ("PiB", 1 << 50),
        ("EiB", 1 << 60),
        ("KB", 1000),
        ("MB", 1000000),
        ("GB", 1000000000),
        ("TB", 1000000000000),
        ("PB", 1000000000000000),
        ("EB", 1000000000000000000),
        ("K", 1 << 10),
        ("k", 1 << 10),
        ("M", 1 << 20),
        ("G", 1 << 30),
        ("T", 1 << 40),
        ("P", 1 << 50),
        ("E", 1 << 60),
        ("b", 512),
    ];
    let (digits, unit) = crate::common::size::split_suffix(text, UNITS, 1);
    let count = if digits.is_empty() {
        1
    } else {
        digits
            .parse::<u64>()
            .map_err(|_| format!("invalid size '{text}'"))?
    };
    count
        .checked_mul(unit)
        .ok_or_else(|| format!("invalid size '{text}'"))
}

fn parse(args: &[String]) -> Result<Options, i32> {
    let specs = [
        OptSpec::flag("null", '0'),
        OptSpec::flag("all", 'a'),
        OptSpec::flag("apparent-size", '\0'),
        OptSpec::value("block-size", 'B'),
        OptSpec::flag("bytes", 'b'),
        OptSpec::flag("total", 'c'),
        OptSpec::flag("dereference-args", 'D'),
        OptSpec {
            id: "dereference-args",
            short: 'H',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec::value("max-depth", 'd'),
        OptSpec::flag("human-readable", 'h'),
        OptSpec::flag("inodes", '\0'),
        OptSpec {
            id: "kibibytes",
            short: 'k',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec::flag("dereference", 'L'),
        OptSpec::flag("count-links", 'l'),
        OptSpec {
            id: "mebibytes",
            short: 'm',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec::flag("no-dereference", 'P'),
        OptSpec::flag("separate-dirs", 'S'),
        OptSpec::flag("si", '\0'),
        OptSpec::flag("summarize", 's'),
        OptSpec::value("threshold", 't'),
        OptSpec::optional("time", '\0'),
        OptSpec::value("time-style", '\0'),
        OptSpec::value("exclude-from", 'X'),
        OptSpec::value("exclude", '\0'),
        OptSpec::flag("one-file-system", 'x'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = args::parse(args, &specs, true).map_err(|error| usage("du", error, 1))?;
    let mut options = Options {
        delimiter: b'\n',
        all: false,
        apparent: false,
        scale: Scale::Blocks(1024),
        total: false,
        follow: Follow::Never,
        depth: usize::MAX,
        links: false,
        separate: false,
        threshold: None,
        clock: None,
        time_style: "%Y-%m-%d %H:%M".into(),
        excludes: Vec::new(),
        one_fs: false,
        paths: Vec::new(),
    };
    let mut summarize = false;
    let mut max_depth = None;
    for item in items {
        if let Item::Opt(id @ ("help" | "version"), _) = &item {
            return Err(args::help_version("du", &[format!("--{id}")], HELP, true).unwrap());
        }
        let result: Result<(), String> = (|| {
            match item {
                Item::Operand(path) => options.paths.push(path.into()),
                Item::Opt("null", _) => options.delimiter = 0,
                Item::Opt("all", _) => options.all = true,
                Item::Opt("apparent-size", _) => options.apparent = true,
                Item::Opt("bytes", _) => {
                    options.apparent = true;
                    options.scale = Scale::Blocks(1);
                }
                Item::Opt("block-size", Some(value)) => {
                    let size = parse_size(&value)?;
                    if size == 0 {
                        return Err("invalid block size '0'".into());
                    }
                    options.scale = Scale::Blocks(size);
                }
                Item::Opt("total", _) => options.total = true,
                Item::Opt("dereference-args", _) => options.follow = Follow::Args,
                Item::Opt("max-depth", Some(value)) => {
                    max_depth = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| format!("invalid maximum depth '{value}'"))?,
                    )
                }
                Item::Opt("human-readable", _) => options.scale = Scale::Human(1024),
                Item::Opt("inodes", _) => {
                    return Err("option '--inodes' is not supported by agentcmd".into())
                }
                Item::Opt("", _) => unreachable!("short-only options have distinct ids"),
                Item::Opt("dereference", _) => options.follow = Follow::Always,
                Item::Opt("no-dereference", _) => options.follow = Follow::Never,
                Item::Opt("count-links", _) => options.links = true,
                Item::Opt("separate-dirs", _) => options.separate = true,
                Item::Opt("si", _) => options.scale = Scale::Human(1000),
                Item::Opt("summarize", _) => summarize = true,
                Item::Opt("threshold", Some(value)) => {
                    let (negative, value) = value
                        .strip_prefix('-')
                        .map_or((false, value.as_str()), |text| (true, text));
                    options.threshold = Some((negative, parse_size(value)?));
                }
                Item::Opt("time", value) => {
                    options.clock = Some(match value.as_deref().unwrap_or("mtime") {
                        "mtime" | "modification" => Clock::Modified,
                        "atime" | "access" | "use" => Clock::Accessed,
                        "ctime" | "status" => Clock::Changed,
                        other => return Err(format!("invalid argument '{other}' for 'time'")),
                    })
                }
                Item::Opt("time-style", Some(value)) => {
                    options.time_style = match value.as_str() {
                        "full-iso" => "%Y-%m-%d %H:%M:%S.%N %z".into(),
                        "long-iso" => "%Y-%m-%d %H:%M".into(),
                        "iso" => "%Y-%m-%d".into(),
                        _ if value.starts_with('+') => value[1..].into(),
                        _ => return Err(format!("invalid argument '{value}' for 'time-style'")),
                    }
                }
                Item::Opt("exclude", Some(value)) => {
                    options.excludes.push(Glob::new(&value, false))
                }
                Item::Opt("exclude-from", Some(value)) => {
                    let mut bytes = Vec::new();
                    crate::common::io::input(&value)
                        .and_then(|mut input| input.read_to_end(&mut bytes))
                        .map_err(|error| format!("{value}: {}", io_text(&error)))?;
                    for pattern in String::from_utf8_lossy(&bytes).lines() {
                        options.excludes.push(Glob::new(pattern, false));
                    }
                }
                Item::Opt("one-file-system", _) => options.one_fs = true,
                Item::Opt("kibibytes", _) => options.scale = Scale::Blocks(1024),
                Item::Opt("mebibytes", _) => options.scale = Scale::Blocks(1048576),
                _ => unreachable!(),
            }
            Ok(())
        })();
        if let Err(error) = result {
            return Err(usage("du", error, 1));
        }
    }
    if summarize && (options.all || max_depth.is_some_and(|depth| depth != 0)) {
        return Err(usage(
            "du",
            "summarizing conflicts with --all or --max-depth",
            1,
        ));
    }
    options.depth = if summarize {
        0
    } else {
        max_depth.unwrap_or(usize::MAX)
    };
    if options.paths.is_empty() {
        options.paths.push(".".into());
    }
    Ok(options)
}

#[derive(Default)]
struct Total {
    size: u64,
    time: Option<SystemTime>,
    directory: bool,
}
impl Total {
    fn add(&mut self, other: &Self) {
        self.size = self.size.saturating_add(other.size);
        self.time = self.time.max(other.time);
    }
}
struct Walker<'a> {
    options: &'a Options,
    out: &'a mut dyn Write,
    seen: HashSet<meta::FileKey>,
    ancestors: HashSet<meta::FileKey>,
    grand: Total,
    failed: bool,
}
impl Walker<'_> {
    fn time(&self, path: &Path, metadata: &Metadata) -> Option<SystemTime> {
        match self.options.clock? {
            Clock::Modified => metadata.modified().ok(),
            Clock::Accessed => metadata.accessed().ok(),
            Clock::Changed => meta::change_time(path, metadata),
        }
    }
    fn error(&mut self, path: &Path, error: &io::Error) {
        eprintln!(
            "du: cannot access '{}': {}",
            crate::display_path(path),
            io_text(error)
        );
        self.failed = true;
    }
    fn visit(&mut self, path: &Path, depth: usize, device: Option<u64>) -> io::Result<Total> {
        let shown = crate::display_path(path);
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        if self
            .options
            .excludes
            .iter()
            .any(|glob| glob.is_match(&shown) || glob.is_match(&name))
        {
            return Ok(Total::default());
        }
        let follow = self.options.follow == Follow::Always
            || (depth == 0 && self.options.follow == Follow::Args);
        let metadata = match if follow {
            fs::metadata(path)
        } else {
            fs::symlink_metadata(path)
        } {
            Ok(metadata) => metadata,
            Err(error) => {
                self.error(path, &error);
                return Ok(Total::default());
            }
        };
        let id = meta::file_identity(path, &metadata);
        let identity = id.key(path, &metadata).ok();
        if self.options.one_fs && device.is_some_and(|device| device != id.dev) {
            return Ok(Total::default());
        }
        if metadata.is_dir()
            && identity
                .as_ref()
                .is_some_and(|key| self.ancestors.contains(key))
        {
            self.error(path, &io::Error::other("Too many levels of symbolic links"));
            return Ok(Total::default());
        }
        if !self.options.links
            && identity
                .as_ref()
                .is_some_and(|key| !self.seen.insert(key.clone()))
        {
            return Ok(Total::default());
        }
        let mut total = Total {
            size: if self.options.apparent {
                metadata.len()
            } else {
                meta::allocated_bytes(&metadata)
            },
            time: self.time(path, &metadata),
            directory: metadata.is_dir(),
        };
        self.grand.add(&total);
        if metadata.is_dir() {
            if let Some(key) = &identity {
                self.ancestors.insert(key.clone());
            }
            match fs::read_dir(path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => {
                                let child = self.visit(
                                    &entry.path(),
                                    depth + 1,
                                    Some(device.unwrap_or(id.dev)),
                                )?;
                                if !self.options.separate || !child.directory {
                                    total.add(&child);
                                }
                            }
                            Err(error) => self.error(path, &error),
                        }
                    }
                }
                Err(error) => self.error(path, &error),
            }
            if let Some(key) = identity {
                self.ancestors.remove(&key);
            }
        }
        if (metadata.is_dir() && depth <= self.options.depth)
            || (self.options.all && depth <= self.options.depth)
            || depth == 0
        {
            self.print(&total, &shown)?;
        }
        Ok(total)
    }
    fn print(&mut self, total: &Total, path: &str) -> io::Result<()> {
        if self.options.threshold.is_some_and(|(negative, size)| {
            if negative {
                total.size > size
            } else {
                total.size < size
            }
        }) {
            return Ok(());
        }
        let size = match self.options.scale {
            Scale::Blocks(block) => total.size.div_ceil(block).to_string(),
            Scale::Human(base) => human(total.size, base),
        };
        write!(self.out, "{size}\t")?;
        if self.options.clock.is_some() {
            let time = total
                .time
                .and_then(|time| datetime::from_system_time(time).ok())
                .and_then(|time| datetime::format(&time, &self.options.time_style).ok())
                .unwrap_or_else(|| "-".into());
            write!(self.out, "{time}\t")?;
        }
        self.out.write_all(path.as_bytes())?;
        self.out.write_all(&[self.options.delimiter])
    }
}

pub fn run(args: &[String]) -> i32 {
    let options = match parse(args) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let mut failed = false;
    let status = output("du", |out| {
        let mut walker = Walker {
            options: &options,
            out,
            seen: HashSet::new(),
            ancestors: HashSet::new(),
            grand: Total::default(),
            failed: false,
        };
        for path in &options.paths {
            walker.visit(path, 0, None)?;
        }
        if options.total {
            let total = std::mem::take(&mut walker.grand);
            walker.print(&total, "total")?;
        }
        failed = walker.failed;
        Ok(())
    });
    status.max(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn size_binary() {
        assert_eq!(parse_size("2M").unwrap(), 2 << 20);
    }
    #[test]
    fn size_decimal() {
        assert_eq!(parse_size("2MB").unwrap(), 2000000);
    }
    #[test]
    fn size_unit_only() {
        assert_eq!(parse_size("K").unwrap(), 1024);
    }
    #[test]
    fn size_invalid() {
        assert!(parse_size("3x").is_err());
    }
    #[test]
    fn size_overflow() {
        assert!(parse_size("18446744073709551615K").is_err());
    }
    #[test]
    fn totals_keep_latest_time() {
        let mut first = Total {
            size: 2,
            time: Some(std::time::UNIX_EPOCH),
            directory: false,
        };
        first.add(&Total {
            size: 3,
            time: None,
            directory: false,
        });
        assert_eq!(first.size, 5);
        assert_eq!(first.time, Some(std::time::UNIX_EPOCH));
    }
}
