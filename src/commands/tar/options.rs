use crate::common::args::{self, Item, OptSpec};

pub const HELP: &str = "Usage: tar [OPTION]... [FILE]...\nCreate, list, or extract a tar archive.\n  -c, --create               create an archive\n  -x, --extract, --get       extract an archive\n  -t, --list                 list an archive\n  -f, --file=ARCHIVE         archive file, or - for standard I/O\n  -z, --gzip, --gunzip       filter through gzip\n  -v, --verbose              list files processed\n  -C, --directory=DIR        change directory for following operands\n      --exclude=PATTERN      exclude matching names\n      --strip-components=N   remove N leading path components\n  -O, --to-stdout            extract files to standard output\n  -k, --keep-old-files       do not replace existing files\n      --overwrite            overwrite existing files\n  -p, --preserve-permissions  preserve extracted permissions\n  -h, --dereference          follow symlinks while creating\n  -P, --absolute-names       preserve absolute names\n      --no-same-owner        do not restore ownership\n  -T, --files-from=FILE      read member names from FILE\n      --null                 use NUL-delimited names with -T\n      --wildcards            interpret member selectors as globs\n  -a, --auto-compress        select gzip by archive suffix\n";
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Mode {
    Create,
    Extract,
    List,
}
#[derive(Clone, Debug)]
pub enum Event {
    Directory(String),
    Operand(String),
    FileList(String),
}
#[derive(Debug)]
pub struct Options {
    pub mode: Mode,
    pub archive: String,
    pub gzip: bool,
    pub verbose: bool,
    pub excludes: Vec<String>,
    pub strip: usize,
    pub to_stdout: bool,
    pub keep: bool,
    pub preserve: bool,
    pub dereference: bool,
    pub absolute: bool,
    pub null: bool,
    pub wildcards: bool,
    pub events: Vec<Event>,
}
fn specs() -> Vec<OptSpec> {
    vec![
        OptSpec::flag("create", 'c'),
        OptSpec::flag("extract", 'x'),
        OptSpec::flag("get", '\0'),
        OptSpec::flag("list", 't'),
        OptSpec::flag("append", 'r'),
        OptSpec::value("file", 'f'),
        OptSpec::flag("gzip", 'z'),
        OptSpec::flag("gunzip", '\0'),
        OptSpec::flag("verbose", 'v'),
        OptSpec::value("directory", 'C'),
        OptSpec::value("exclude", '\0'),
        OptSpec::value("strip-components", '\0'),
        OptSpec::flag("to-stdout", 'O'),
        OptSpec::flag("keep-old-files", 'k'),
        OptSpec::flag("overwrite", '\0'),
        OptSpec::flag("preserve-permissions", 'p'),
        OptSpec::flag("dereference", 'h'),
        OptSpec::flag("absolute-names", 'P'),
        OptSpec::flag("no-same-owner", '\0'),
        OptSpec::value("files-from", 'T'),
        OptSpec::flag("null", '\0'),
        OptSpec::flag("wildcards", '\0'),
        OptSpec::flag("auto-compress", 'a'),
        OptSpec::flag("bzip2", 'j'),
        OptSpec::flag("xz", 'J'),
        OptSpec::flag("zstd", '\0'),
        OptSpec::flag("lzma", '\0'),
    ]
}
#[derive(Default)]
struct Draft {
    modes: Vec<Mode>,
    archive: Option<String>,
    gzip: bool,
    auto: bool,
    verbose: bool,
    excludes: Vec<String>,
    strip: usize,
    to_stdout: bool,
    keep: bool,
    preserve: bool,
    dereference: bool,
    absolute: bool,
    null: bool,
    wildcards: bool,
    events: Vec<Event>,
}
fn old_options(argv: &[String]) -> Vec<String> {
    let Some(first) = argv.first() else {
        return Vec::new();
    };
    if first.starts_with('-')
        || !first.chars().all(|c| "cxtfzvCOkhpPaTjrJ".contains(c))
        || !first.chars().any(|c| "cxt".contains(c))
    {
        return argv.to_vec();
    }
    let mut remaining = argv[1..].iter();
    let mut result = Vec::new();
    for option in first.chars() {
        result.push(format!("-{option}"));
        if matches!(option, 'f' | 'C' | 'T') {
            if let Some(value) = remaining.next() {
                result.push(value.clone());
            } else {
                return result;
            }
        }
    }
    result.extend(remaining.cloned());
    result
}
pub fn parse(argv: &[String]) -> Result<Options, i32> {
    let argv = old_options(argv);
    let mut all_specs = specs();
    all_specs.extend([OptSpec::flag("help", '\0'), OptSpec::flag("version", '\0')]);
    let items = args::parse(&argv, &all_specs, true)
        .map_err(|message| crate::common::error::usage("tar", message, 2))?;
    if let Some(Item::Opt(id, _)) = items
        .iter()
        .find(|item| matches!(item, Item::Opt("help" | "version", _)))
    {
        return Err(args::help_version("tar", &[format!("--{id}")], HELP, true).unwrap_or(0));
    }
    let mut d = Draft::default();
    for item in items {
        apply(&mut d, item)?;
    }
    if d.modes.len() != 1 {
        return Err(crate::common::error::usage(
            "tar",
            "exactly one of -c, -x or -t is required",
            2,
        ));
    }
    let archive = d.archive.unwrap_or_else(|| "-".into());
    let gzip = d.gzip || (d.auto && (archive.ends_with(".tar.gz") || archive.ends_with(".tgz")));
    Ok(Options {
        mode: d.modes[0],
        archive,
        gzip,
        verbose: d.verbose,
        excludes: d.excludes,
        strip: d.strip,
        to_stdout: d.to_stdout,
        keep: d.keep,
        preserve: d.preserve,
        dereference: d.dereference,
        absolute: d.absolute,
        null: d.null,
        wildcards: d.wildcards,
        events: d.events,
    })
}
fn apply(d: &mut Draft, item: Item) -> Result<(), i32> {
    match item {
        Item::Operand(v) => d.events.push(Event::Operand(v)),
        Item::Opt("create", _) => d.modes.push(Mode::Create),
        Item::Opt("extract" | "get", _) => d.modes.push(Mode::Extract),
        Item::Opt("list", _) => d.modes.push(Mode::List),
        Item::Opt(id @ ("append" | "bzip2" | "xz" | "zstd" | "lzma"), _) => {
            return Err(crate::common::error::usage(
                "tar",
                format!("option '--{id}' is not supported by agentcmd"),
                2,
            ))
        }
        Item::Opt("file", Some(v)) => d.archive = Some(v),
        Item::Opt("gzip" | "gunzip", _) => d.gzip = true,
        Item::Opt("verbose", _) => d.verbose = true,
        Item::Opt("directory", Some(v)) => d.events.push(Event::Directory(v)),
        Item::Opt("files-from", Some(v)) => d.events.push(Event::FileList(v)),
        Item::Opt("exclude", Some(v)) => d.excludes.push(v),
        Item::Opt("strip-components", Some(v)) => {
            d.strip = v
                .parse()
                .map_err(|_| crate::common::error::usage("tar", "invalid strip count", 2))?
        }
        Item::Opt("to-stdout", _) => d.to_stdout = true,
        Item::Opt("keep-old-files", _) => d.keep = true,
        Item::Opt("overwrite", _) => d.keep = false,
        Item::Opt("preserve-permissions", _) => d.preserve = true,
        Item::Opt("dereference", _) => d.dereference = true,
        Item::Opt("absolute-names", _) => d.absolute = true,
        Item::Opt("no-same-owner", _) => {}
        Item::Opt("null", _) => d.null = true,
        Item::Opt("wildcards", _) => d.wildcards = true,
        Item::Opt("auto-compress", _) => d.auto = true,
        _ => return Err(crate::common::error::usage("tar", "invalid option", 2)),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_style() {
        let o = parse(&["xzf".into(), "a.tgz".into()]).unwrap();
        assert_eq!(o.mode, Mode::Extract);
        assert!(o.gzip);
        assert_eq!(o.archive, "a.tgz");
    }
    #[test]
    fn ordered_dirs() {
        let o = parse(&[
            "-cf".into(),
            "a".into(),
            "-C".into(),
            "d".into(),
            "f".into(),
        ])
        .unwrap();
        assert!(matches!(
            &o.events[..],
            [Event::Directory(_), Event::Operand(_)]
        ));
    }
    #[test]
    fn incompatible_modes() {
        assert!(parse(&["-cx".into()]).is_err());
        assert!(parse(&["-j".into(), "-c".into()]).is_err());
    }
}
