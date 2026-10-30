//! Command-line handling for `tar`, including GNU's "old style" first argument (`tar xzf a`).

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, unsupported, EXIT_FAIL, EXIT_OK};

pub const CMD: &str = "tar";

/// Operation requested with `-c`, `-x` or `-t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Create,
    Extract,
    List,
}

/// One file operand and the `-C` directory in effect where it appeared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operand {
    pub name: String,
    pub chdir: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub action: Action,
    /// `None` and `-` mean standard input (read) or standard output (write).
    pub archive: Option<String>,
    pub gzip: bool,
    pub auto_compress: bool,
    pub verbose: bool,
    pub operands: Vec<Operand>,
    /// Last `-C` seen; the extraction directory.
    pub extract_dir: Option<String>,
    pub excludes: Vec<String>,
    pub strip: usize,
    pub to_stdout: bool,
    pub keep_old: bool,
    pub preserve_permissions: bool,
    pub absolute_names: bool,
    pub dereference: bool,
    pub files_from: Vec<String>,
    pub null_separated: bool,
}

const SPECS: &[OptSpec] = &[
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
    OptSpec::flag("create", Some('c'), Some("create")),
    OptSpec::flag("extract", Some('x'), Some("extract")),
    OptSpec::flag("extract", None, Some("get")),
    OptSpec::flag("list", Some('t'), Some("list")),
    OptSpec::req("file", Some('f'), Some("file")),
    OptSpec::flag("gzip", Some('z'), Some("gzip")),
    OptSpec::flag("gzip", None, Some("gunzip")),
    OptSpec::flag("gzip", None, Some("ungzip")),
    OptSpec::flag("auto", Some('a'), Some("auto-compress")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::req("chdir", Some('C'), Some("directory")),
    OptSpec::req("exclude", None, Some("exclude")),
    OptSpec::req("strip", None, Some("strip-components")),
    OptSpec::flag("to-stdout", Some('O'), Some("to-stdout")),
    OptSpec::flag("keep-old", Some('k'), Some("keep-old-files")),
    OptSpec::flag("overwrite", None, Some("overwrite")),
    OptSpec::flag("preserve", Some('p'), Some("preserve-permissions")),
    OptSpec::flag("preserve", None, Some("same-permissions")),
    OptSpec::flag("absolute", Some('P'), Some("absolute-names")),
    OptSpec::flag("dereference", Some('h'), Some("dereference")),
    OptSpec::flag("no-same-owner", None, Some("no-same-owner")),
    OptSpec::req("files-from", Some('T'), Some("files-from")),
    OptSpec::flag("null", None, Some("null")),
    OptSpec::flag("wildcards", None, Some("wildcards")),
    OptSpec::flag("unsupported-j", Some('j'), Some("bzip2")),
    OptSpec::flag("unsupported-J", Some('J'), Some("xz")),
    OptSpec::flag("unsupported-zstd", None, Some("zstd")),
    OptSpec::flag("unsupported-lzma", None, Some("lzma")),
];

const OPTION_LETTERS: &str = "cxtfzavCOkpPhTjJ";

pub const HELP: &str = "\
Usage: tar [OPTION...] [FILE]...
Create, list or extract tar archives.

Main operation mode:
  -c, --create               create a new archive
  -x, --extract, --get       extract files from an archive
  -t, --list                 list the contents of an archive

Operation modifiers:
  -f, --file=ARCHIVE         use archive file ARCHIVE (- for standard input or output)
  -z, --gzip, --gunzip       filter the archive through gzip
  -a, --auto-compress        choose gzip from the archive file name (.tar.gz, .tgz)
  -C, --directory=DIR        change to DIR before performing any operations
  -T, --files-from=FILE      get names to operate on from FILE (- for standard input)
      --null                 with -T, names are separated by NUL bytes
      --exclude=PATTERN      exclude members whose names match PATTERN
      --wildcards            accepted; patterns always use shell wildcards
      --strip-components=N   strip N leading components from member names on extraction

Overwrite control:
  -k, --keep-old-files       do not replace existing files
      --overwrite            replace existing files (the default)

Select output options:
  -v, --verbose              verbosely list files processed
  -O, --to-stdout            extract files to standard output

Handling of file attributes:
  -p, --preserve-permissions keep the permissions from the archive
  -h, --dereference          follow symbolic links when creating an archive
  -P, --absolute-names       do not strip leading '/' from member names
      --no-same-owner        do not restore ownership (the default)

Informative output:
      --help                 give this help list
      --version              print program version

Exit status is 0 on success, 1 when files changed or differ, 2 on fatal errors.
";

/// Parses `args` (already split from the command name). `Err(code)` means the process
/// should exit with `code` now.
pub fn parse(args: &[String]) -> Result<Options, i32> {
    let normalized = old_style(args);
    let items = match Parser::new(CMD, SPECS).parse(&normalized) {
        Ok(items) => items,
        Err(error) => return Err(report_usage(CMD, &error, EXIT_FAIL)),
    };
    let mut opts = Options {
        action: Action::Extract,
        archive: None,
        gzip: false,
        auto_compress: false,
        verbose: false,
        operands: Vec::new(),
        extract_dir: None,
        excludes: Vec::new(),
        strip: 0,
        to_stdout: false,
        keep_old: false,
        preserve_permissions: false,
        absolute_names: false,
        dereference: false,
        files_from: Vec::new(),
        null_separated: false,
    };
    let mut action = None;
    let mut chdir: Option<String> = None;
    for item in items {
        match item {
            Item::Opt { id, value } => {
                if let Some(code) = apply(&mut opts, &mut action, &mut chdir, id, value) {
                    return Err(code);
                }
            }
            Item::Operand(name) => opts.operands.push(Operand {
                name,
                chdir: chdir.clone(),
            }),
        }
    }
    opts.extract_dir = chdir;
    let Some(action) = action else {
        eprintln!(
            "{CMD}: You must specify one of the '-Acdtrux', '--delete' or '--test-label' options\n\
             Try '{CMD} --help' or '{CMD} --usage' for more information."
        );
        return Err(EXIT_FAIL);
    };
    opts.action = action;
    Ok(opts)
}

/// Applies one parsed option. Returns an exit code when the option ends the run.
fn apply(
    opts: &mut Options,
    action: &mut Option<Action>,
    chdir: &mut Option<String>,
    id: &str,
    value: Option<String>,
) -> Option<i32> {
    match id {
        "help" => {
            print!("{HELP}");
            return Some(EXIT_OK);
        }
        "version" => return Some(print_version(CMD)),
        "create" => *action = Some(Action::Create),
        "extract" => *action = Some(Action::Extract),
        "list" => *action = Some(Action::List),
        "file" => opts.archive = value,
        "gzip" => opts.gzip = true,
        "auto" => opts.auto_compress = true,
        "verbose" => opts.verbose = true,
        "chdir" => *chdir = value,
        "exclude" => opts.excludes.extend(value),
        "strip" => match value.as_deref().map(str::parse::<usize>) {
            Some(Ok(count)) => opts.strip = count,
            _ => {
                eprintln!(
                    "{CMD}: invalid number of components: '{}'",
                    value.unwrap_or_default()
                );
                return Some(EXIT_FAIL);
            }
        },
        "to-stdout" => opts.to_stdout = true,
        "keep-old" => opts.keep_old = true,
        "overwrite" | "no-same-owner" | "wildcards" => {}
        "preserve" => opts.preserve_permissions = true,
        "absolute" => opts.absolute_names = true,
        "dereference" => opts.dereference = true,
        "files-from" => opts.files_from.extend(value),
        "null" => opts.null_separated = true,
        other => return Some(unsupported_option(other)),
    }
    None
}

fn unsupported_option(id: &str) -> i32 {
    let shown = match id {
        "unsupported-j" => "-j",
        "unsupported-J" => "-J",
        "unsupported-zstd" => "--zstd",
        "unsupported-lzma" => "--lzma",
        other => other,
    };
    unsupported(CMD, shown)
}

/// GNU accepts a first argument without a leading dash as a cluster of option letters:
/// `tar xzf a.tgz` means `tar -xzf a.tgz`. Other arguments are left alone.
fn old_style(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = args.to_vec();
    if let Some(first) = args.first() {
        let looks_like_cluster = !first.is_empty()
            && !first.starts_with('-')
            && first.chars().all(|c| OPTION_LETTERS.contains(c));
        if looks_like_cluster {
            out[0] = format!("-{first}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn old_style_cluster_becomes_options() {
        assert_eq!(old_style(&args(&["xzf", "a.tgz"]))[0], "-xzf");
        assert_eq!(old_style(&args(&["-c", "x"]))[0], "-c");
        assert_eq!(old_style(&args(&["file.tar"]))[0], "file.tar");
    }

    #[test]
    fn bundled_create_takes_next_word_for_f() {
        let opts = parse(&args(&["czf", "a.tgz", "src"])).unwrap();
        assert_eq!(opts.action, Action::Create);
        assert!(opts.gzip);
        assert_eq!(opts.archive.as_deref(), Some("a.tgz"));
        assert_eq!(opts.operands[0].name, "src");
    }

    #[test]
    fn chdir_applies_to_later_operands_only() {
        let opts = parse(&args(&[
            "-c", "-f", "a.tar", "-C", "one", "x", "-C", "two", "y",
        ]))
        .unwrap();
        assert_eq!(opts.operands[0].chdir.as_deref(), Some("one"));
        assert_eq!(opts.operands[1].chdir.as_deref(), Some("two"));
        assert_eq!(opts.extract_dir.as_deref(), Some("two"));
    }

    #[test]
    fn missing_mode_is_a_usage_error() {
        assert_eq!(parse(&args(&["-f", "a.tar"])).unwrap_err(), EXIT_FAIL);
    }

    #[test]
    fn unsupported_compressions_are_refused() {
        assert_eq!(
            parse(&args(&["-cj", "-f", "a.tbz"])).unwrap_err(),
            EXIT_FAIL
        );
    }

    #[test]
    fn strip_and_exclude_parse_values() {
        let opts = parse(&args(&["-x", "--strip-components=2", "--exclude=*.o"])).unwrap();
        assert_eq!(opts.strip, 2);
        assert_eq!(opts.excludes, vec!["*.o".to_string()]);
    }
}
