//! Command-line handling for `gzip`, `gunzip` and `zcat`.

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

/// Name used in messages. GNU gzip prints `gzip:` even when invoked as `gunzip` or `zcat`.
pub const MSG_NAME: &str = "gzip";
pub const DEFAULT_SUFFIX: &str = ".gz";
pub const DEFAULT_LEVEL: u32 = 6;

/// The utility the user invoked; it sets the default mode and the usage text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prog {
    Gzip,
    Gunzip,
    Zcat,
}

impl Prog {
    pub fn name(self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Gunzip => "gunzip",
            Self::Zcat => "zcat",
        }
    }
}

/// What the run does with each operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Compress,
    Decompress,
    List,
    Test,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Mode,
    pub to_stdout: bool,
    pub force: bool,
    pub keep: bool,
    pub no_name: bool,
    pub restore_name: bool,
    pub quiet: bool,
    pub recursive: bool,
    pub verbose: bool,
    pub level: u32,
    pub suffix: String,
    pub files: Vec<String>,
}

const SPECS: &[OptSpec] = &[
    OptSpec::flag("help", Some('h'), Some("help")),
    OptSpec::flag("version", Some('V'), Some("version")),
    OptSpec::flag("stdout", Some('c'), Some("stdout")),
    OptSpec::flag("stdout", None, Some("to-stdout")),
    OptSpec::flag("decompress", Some('d'), Some("decompress")),
    OptSpec::flag("decompress", None, Some("uncompress")),
    OptSpec::flag("force", Some('f'), Some("force")),
    OptSpec::flag("keep", Some('k'), Some("keep")),
    OptSpec::flag("list", Some('l'), Some("list")),
    OptSpec::flag("no-name", Some('n'), Some("no-name")),
    OptSpec::flag("name", Some('N'), Some("name")),
    OptSpec::flag("quiet", Some('q'), Some("quiet")),
    OptSpec::flag("recursive", Some('r'), Some("recursive")),
    OptSpec::req("suffix", Some('S'), Some("suffix")),
    OptSpec::flag("test", Some('t'), Some("test")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("level1", Some('1'), Some("fast")),
    OptSpec::flag("level2", Some('2'), None),
    OptSpec::flag("level3", Some('3'), None),
    OptSpec::flag("level4", Some('4'), None),
    OptSpec::flag("level5", Some('5'), None),
    OptSpec::flag("level6", Some('6'), None),
    OptSpec::flag("level7", Some('7'), None),
    OptSpec::flag("level8", Some('8'), None),
    OptSpec::flag("level9", Some('9'), Some("best")),
    OptSpec::flag("synchronous", None, Some("synchronous")),
    OptSpec::flag("rsyncable", None, Some("rsyncable")),
];

const HELP_TAIL: &str = "\
 -c, --stdout      write on standard output, keep original files unchanged
 -d, --decompress  decompress
 -f, --force       force overwrite of output file and compress links
 -h, --help        give this help
 -k, --keep        keep (don't delete) input files
 -l, --list        list compressed file contents
 -n, --no-name     do not save or restore the original name and timestamp
 -N, --name        save or restore the original name and timestamp
 -q, --quiet       suppress all warnings
 -r, --recursive   operate recursively on directories
 -S, --suffix=SUF  use suffix SUF on compressed files
 -t, --test        test compressed file integrity
 -v, --verbose     verbose mode
 -V, --version     display version number
 -1, --fast        compress faster
 -9, --best        compress better
     --synchronous  accepted; output is not forced to disk
     --rsyncable   accepted; no effect

With no FILE, or when FILE is -, read standard input.

Exit status is 0 if OK, 1 if an error occurred, 2 if a warning was issued.
";

/// Full help text for `prog`.
pub fn help_text(prog: Prog) -> String {
    let summary = match prog {
        Prog::Gzip => "Compress or uncompress FILEs (by default, compress FILES in-place).",
        Prog::Gunzip => "Uncompress FILEs (by default, in-place).",
        Prog::Zcat => "Uncompress FILEs to standard output.",
    };
    format!(
        "Usage: {} [OPTION]... [FILE]...\n{summary}\n\n{HELP_TAIL}",
        prog.name()
    )
}

/// Parses the arguments. `Err(code)` means the process should exit with `code` now
/// (help, version or a usage error has already been printed).
pub fn parse(prog: Prog, args: &[String]) -> Result<Options, i32> {
    let items = match Parser::new(MSG_NAME, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return Err(report_usage(MSG_NAME, &error, EXIT_FAIL)),
    };
    let mut opts = Options {
        mode: match prog {
            Prog::Gzip => Mode::Compress,
            Prog::Gunzip | Prog::Zcat => Mode::Decompress,
        },
        to_stdout: prog == Prog::Zcat,
        force: false,
        keep: false,
        no_name: false,
        restore_name: false,
        quiet: false,
        recursive: false,
        verbose: false,
        level: DEFAULT_LEVEL,
        suffix: DEFAULT_SUFFIX.to_string(),
        files: Vec::new(),
    };
    let mut list = false;
    let mut test = false;
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{}", help_text(prog));
                return Err(EXIT_OK);
            }
            Item::Opt { id: "version", .. } => return Err(print_version(prog.name())),
            Item::Opt { id, value } => apply(&mut opts, &mut list, &mut test, id, value),
            Item::Operand(name) => opts.files.push(name),
        }
    }
    if list {
        opts.mode = Mode::List;
    } else if test {
        opts.mode = Mode::Test;
    }
    Ok(opts)
}

fn apply(opts: &mut Options, list: &mut bool, test: &mut bool, id: &str, value: Option<String>) {
    match id {
        "stdout" => opts.to_stdout = true,
        "decompress" => opts.mode = Mode::Decompress,
        "force" => opts.force = true,
        "keep" => opts.keep = true,
        "list" => *list = true,
        "no-name" => {
            opts.no_name = true;
            opts.restore_name = false;
        }
        "name" => {
            opts.restore_name = true;
            opts.no_name = false;
        }
        "quiet" => opts.quiet = true,
        "recursive" => opts.recursive = true,
        "suffix" => opts.suffix = value.unwrap_or_default(),
        "test" => *test = true,
        "verbose" => opts.verbose = true,
        "best" => opts.level = 9,
        other => {
            if let Some(digit) = other
                .strip_prefix("level")
                .and_then(|d| d.parse::<u32>().ok())
            {
                opts.level = digit;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn program_names_pick_default_modes() {
        let gz = parse(Prog::Gzip, &args(&["a"])).unwrap();
        assert_eq!((gz.mode, gz.to_stdout), (Mode::Compress, false));
        let gunzip = parse(Prog::Gunzip, &args(&[])).unwrap();
        assert_eq!((gunzip.mode, gunzip.to_stdout), (Mode::Decompress, false));
        let zcat = parse(Prog::Zcat, &args(&["a.gz"])).unwrap();
        assert_eq!((zcat.mode, zcat.to_stdout), (Mode::Decompress, true));
    }

    #[test]
    fn bundled_short_options_and_levels_parse() {
        let opts = parse(Prog::Gzip, &args(&["-dkv9", "x.gz"])).unwrap();
        assert_eq!(opts.mode, Mode::Decompress);
        assert!(opts.keep && opts.verbose);
        assert_eq!(opts.level, 9);
        assert_eq!(opts.files, vec!["x.gz".to_string()]);
    }

    #[test]
    fn list_and_test_override_the_mode() {
        assert_eq!(
            parse(Prog::Gzip, &args(&["-l", "a.gz"])).unwrap().mode,
            Mode::List
        );
        assert_eq!(
            parse(Prog::Gunzip, &args(&["-t"])).unwrap().mode,
            Mode::Test
        );
    }

    #[test]
    fn suffix_and_name_flags_take_values() {
        let opts = parse(Prog::Gzip, &args(&["-S", ".z", "--name"])).unwrap();
        assert_eq!(opts.suffix, ".z");
        assert!(opts.restore_name);
        let long = parse(Prog::Gzip, &args(&["--suffix=zz", "--fast"])).unwrap();
        assert_eq!((long.suffix.as_str(), long.level), ("zz", 1));
    }

    #[test]
    fn unknown_option_is_a_usage_error() {
        assert_eq!(
            parse(Prog::Gzip, &args(&["--bogus"])).unwrap_err(),
            EXIT_FAIL
        );
    }

    #[test]
    fn help_names_the_invoked_program() {
        assert!(help_text(Prog::Gunzip).starts_with("Usage: gunzip [OPTION]"));
        assert!(help_text(Prog::Gzip).contains("--suffix=SUF"));
    }
}
