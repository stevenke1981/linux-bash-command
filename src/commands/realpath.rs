//! `realpath` and `readlink`: canonical file names and symbolic-link targets.
//!
//! Both share [`canonical`], which resolves a path under one of three existence rules:
//! every component exists (`-e`), every component but the last exists (the default), or
//! nothing has to exist (`-m`). Output uses `common::path::display_path`, and Windows
//! verbatim prefixes are removed.

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::path::{display_path, strip_verbatim};

const REALPATH_HELP: &str = r#"Usage: realpath [OPTION]... FILE...
Print the resolved absolute file name;
all but the last component must exist

  -e, --canonicalize-existing  all components of the path must exist
  -m, --canonicalize-missing   no path components need exist or be a directory
  -L, --logical                resolve '..' components before symlinks
  -P, --physical               resolve symlinks as encountered (default)
  -q, --quiet                  suppress most error messages
      --relative-base=DIR      print absolute paths unless paths below DIR
      --relative-to=DIR        print the resolved path relative to DIR
  -s, --strip, --no-symlinks   don't expand symlinks
  -z, --zero                   end each output line with NUL, not newline
      --help     display this help and exit
      --version  output version information and exit
"#;

const READLINK_HELP: &str = r#"Usage: readlink [OPTION]... FILE...
Print value of a symbolic link or canonical file name.

  -f, --canonicalize            canonicalize by following every symlink in
                                every component of the given name recursively;
                                all but the last component must exist
  -e, --canonicalize-existing   canonicalize by following every symlink in
                                every component of the given name recursively,
                                all components must exist
  -m, --canonicalize-missing    canonicalize by following every symlink in
                                every component of the given name recursively,
                                without requirements on components existence
  -n, --no-newline             do not output the trailing delimiter
  -q, --quiet
  -s, --silent                 suppress most error messages
  -v, --verbose                report error messages
  -z, --zero                   end each output line with NUL, not newline
      --help     display this help and exit
      --version  output version information and exit
"#;

const REALPATH_SPECS: &[OptSpec] = &[
    OptSpec::flag("exists", Some('e'), Some("canonicalize-existing")),
    OptSpec::flag("missing", Some('m'), Some("canonicalize-missing")),
    OptSpec::flag("logical", Some('L'), Some("logical")),
    OptSpec::flag("physical", Some('P'), Some("physical")),
    OptSpec::flag("quiet", Some('q'), Some("quiet")),
    OptSpec::req("relative-to", None, Some("relative-to")),
    OptSpec::req("relative-base", None, Some("relative-base")),
    OptSpec::flag("strip", Some('s'), Some("strip")),
    OptSpec::flag("strip", None, Some("no-symlinks")),
    OptSpec::flag("zero", Some('z'), Some("zero")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const READLINK_SPECS: &[OptSpec] = &[
    OptSpec::flag("canonicalize", Some('f'), Some("canonicalize")),
    OptSpec::flag("exists", Some('e'), Some("canonicalize-existing")),
    OptSpec::flag("missing", Some('m'), Some("canonicalize-missing")),
    OptSpec::flag("no-newline", Some('n'), Some("no-newline")),
    OptSpec::flag("quiet", Some('q'), Some("quiet")),
    OptSpec::flag("silent", Some('s'), Some("silent")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("zero", Some('z'), Some("zero")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// How many components must exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Existence {
    /// Every component (`-e`).
    All,
    /// All but the last (`-f`, `realpath` default).
    AllButLast,
    /// None (`-m`).
    None,
}

/// How symbolic links are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Links {
    /// Resolve symlinks as encountered (`-P` and the default).
    Physical,
    /// Remove `..` lexically before resolving symlinks (`-L`).
    Logical,
    /// Never resolve symlinks (`-s`).
    Strip,
}

/// Output framing shared by both commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Framing {
    /// Terminator after each result; `None` for `readlink -n`.
    delimiter: Option<char>,
    /// Whether failures are printed.
    report_errors: bool,
}

/// Entry point for `realpath`.
pub fn realpath(args: &[String]) -> i32 {
    let items = match Parser::new("realpath", REALPATH_SPECS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage("realpath", &err, EXIT_FAIL),
    };
    let config = match realpath_config(&items) {
        Ok(config) => config,
        Err(code) => return code,
    };
    if config.operands.is_empty() {
        eprintln!("realpath: missing operand\nTry 'realpath --help' for more information.");
        return EXIT_FAIL;
    }
    let framing = Framing {
        delimiter: Some(if config.zero { '\0' } else { '\n' }),
        report_errors: !config.quiet,
    };
    run_each(&config.operands, "realpath", framing, |path| {
        let resolved = canonical(path, config.existence, config.links)?;
        Ok(relative_output(&resolved, &config))
    })
}

/// Entry point for `readlink`.
pub fn readlink(args: &[String]) -> i32 {
    let items = match Parser::new("readlink", READLINK_SPECS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage("readlink", &err, EXIT_FAIL),
    };
    let config = match readlink_config(&items) {
        Ok(config) => config,
        Err(code) => return code,
    };
    if config.operands.is_empty() {
        eprintln!("readlink: missing operand\nTry 'readlink --help' for more information.");
        return EXIT_FAIL;
    }
    let framing = Framing {
        delimiter: match (config.no_newline, config.zero) {
            (true, _) => None,
            (false, true) => Some('\0'),
            (false, false) => Some('\n'),
        },
        report_errors: config.verbose && !config.quiet,
    };
    run_each(&config.operands, "readlink", framing, |path| {
        match config.mode {
            Some(existence) => canonical(path, existence, Links::Physical),
            None => fs::read_link(path).map_err(not_a_link_is_invalid),
        }
    })
}

/// Windows reports reading a non-link as `ERROR_NOT_A_REPARSE_POINT`; GNU readlink reports
/// `EINVAL` ("Invalid argument"), so the error is mapped to that.
fn not_a_link_is_invalid(error: io::Error) -> io::Error {
    const ERROR_NOT_A_REPARSE_POINT: i32 = 4390;
    if cfg!(windows) && error.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT) {
        io::Error::from(io::ErrorKind::InvalidInput)
    } else {
        error
    }
}

/// Options after parsing `realpath`.
#[derive(Debug)]
struct RealpathConfig {
    existence: Existence,
    links: Links,
    quiet: bool,
    zero: bool,
    relative_to: Option<PathBuf>,
    relative_base: Option<PathBuf>,
    operands: Vec<String>,
}

fn realpath_config(items: &[Item]) -> Result<RealpathConfig, i32> {
    let mut config = RealpathConfig {
        existence: Existence::AllButLast,
        links: Links::Physical,
        quiet: false,
        zero: false,
        relative_to: None,
        relative_base: None,
        operands: Vec::new(),
    };
    for item in items {
        match item {
            Item::Operand(text) => config.operands.push(text.clone()),
            Item::Opt { id, value } => match *id {
                "help" => {
                    print!("{REALPATH_HELP}");
                    return Err(EXIT_OK);
                }
                "version" => return Err(print_version("realpath")),
                "exists" => config.existence = Existence::All,
                "missing" => config.existence = Existence::None,
                "logical" => config.links = Links::Logical,
                "physical" => config.links = Links::Physical,
                "quiet" => config.quiet = true,
                "strip" => config.links = Links::Strip,
                "zero" => config.zero = true,
                "relative-to" => config.relative_to = value.as_deref().map(PathBuf::from),
                "relative-base" => config.relative_base = value.as_deref().map(PathBuf::from),
                _ => {}
            },
        }
    }
    Ok(config)
}

/// Options after parsing `readlink`.
#[derive(Debug)]
struct ReadlinkConfig {
    /// `Some` when a canonicalizing option was given.
    mode: Option<Existence>,
    no_newline: bool,
    quiet: bool,
    verbose: bool,
    zero: bool,
    operands: Vec<String>,
}

fn readlink_config(items: &[Item]) -> Result<ReadlinkConfig, i32> {
    let mut config = ReadlinkConfig {
        mode: None,
        no_newline: false,
        quiet: false,
        verbose: false,
        zero: false,
        operands: Vec::new(),
    };
    for item in items {
        match item {
            Item::Operand(text) => config.operands.push(text.clone()),
            Item::Opt { id, .. } => match *id {
                "help" => {
                    print!("{READLINK_HELP}");
                    return Err(EXIT_OK);
                }
                "version" => return Err(print_version("readlink")),
                "canonicalize" => config.mode = Some(Existence::AllButLast),
                "exists" => config.mode = Some(Existence::All),
                "missing" => config.mode = Some(Existence::None),
                "no-newline" => config.no_newline = true,
                "quiet" | "silent" => config.quiet = true,
                "verbose" => config.verbose = true,
                "zero" => config.zero = true,
                _ => {}
            },
        }
    }
    Ok(config)
}

/// Applies `resolve` to each operand and prints the results. Exit status is 1 if any
/// operand failed; failures are printed only when `framing.report_errors` is set.
fn run_each(
    operands: &[String],
    cmd: &str,
    framing: Framing,
    resolve: impl Fn(&Path) -> io::Result<PathBuf>,
) -> i32 {
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    for operand in operands {
        match resolve(Path::new(operand)) {
            Ok(path) => {
                let text = display_path(&strip_verbatim(&path));
                let written = match framing.delimiter {
                    Some(end) => write!(out, "{text}{end}"),
                    None => write!(out, "{text}"),
                };
                if let Err(error) = written {
                    if is_broken_pipe(&error) {
                        return EXIT_OK;
                    }
                    eprintln!("{cmd}: write error: {}", io_error_text(&error));
                    return EXIT_FAIL;
                }
            }
            Err(error) => {
                status = EXIT_FAIL;
                if framing.report_errors {
                    eprintln!("{cmd}: {operand}: {}", io_error_text(&error));
                }
            }
        }
    }
    let flushed = finish_output(cmd, &mut out);
    if status == EXIT_OK {
        flushed
    } else {
        status
    }
}

/// Applies the `--relative-base` and `--relative-to` rules to a resolved path.
fn relative_output(resolved: &Path, config: &RealpathConfig) -> PathBuf {
    if let Some(base) = &config.relative_base {
        let base = absolute_lexical(base);
        if !resolved.starts_with(&base) {
            return resolved.to_path_buf();
        }
        if config.relative_to.is_none() {
            return relative_path(resolved, &base);
        }
    }
    match &config.relative_to {
        Some(dir) => relative_path(resolved, &absolute_lexical(dir)),
        None => resolved.to_path_buf(),
    }
}

/// Resolves `path` to an absolute name under the given existence and link rules.
fn canonical(path: &Path, existence: Existence, links: Links) -> io::Result<PathBuf> {
    match links {
        Links::Strip => {
            let absolute = absolute_lexical(path);
            check_existence(&absolute, existence).map(|()| absolute)
        }
        // `-L` removes `..` before following links; `-P` follows links first, so `..` is
        // left for the file system to resolve.
        Links::Logical => resolve_physical(&absolute_lexical(path), existence),
        Links::Physical => resolve_physical(&make_absolute(path), existence),
    }
}

/// `path` made absolute by prefixing the working directory. Nothing is normalized.
fn make_absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// Canonicalizes with symlinks resolved, honouring the existence rule for missing parts.
fn resolve_physical(absolute: &Path, existence: Existence) -> io::Result<PathBuf> {
    match existence {
        Existence::All => fs::canonicalize(absolute).map(|real| strip_verbatim(&real)),
        Existence::AllButLast => resolve_all_but_last(absolute),
        Existence::None => resolve_missing(absolute),
    }
}

/// Every component but the last must exist; the last may be missing.
fn resolve_all_but_last(absolute: &Path) -> io::Result<PathBuf> {
    if let Ok(real) = fs::canonicalize(absolute) {
        return Ok(strip_verbatim(&real));
    }
    let parent = absolute.parent().unwrap_or(absolute);
    let real_parent = strip_verbatim(&fs::canonicalize(parent)?);
    if !real_parent.is_dir() {
        return Err(io::Error::from(io::ErrorKind::NotADirectory));
    }
    Ok(match absolute.file_name() {
        Some(name) => real_parent.join(name),
        None => real_parent,
    })
}

/// Nothing has to exist: canonicalize the longest existing prefix and append the rest.
fn resolve_missing(absolute: &Path) -> io::Result<PathBuf> {
    let (existing, rest) = split_existing(absolute);
    let base = match fs::canonicalize(&existing) {
        Ok(real) => strip_verbatim(&real),
        Err(_) => existing,
    };
    Ok(rest.iter().fold(base, |acc, part| acc.join(part)))
}

/// Splits `path` into its longest existing prefix and the missing components after it.
fn split_existing(path: &Path) -> (PathBuf, Vec<PathBuf>) {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(PathBuf::from(name));
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    rest.reverse();
    (existing, rest)
}

/// For `-s`: the path must exist under the rule, but links are kept as they are.
fn check_existence(absolute: &Path, existence: Existence) -> io::Result<()> {
    let target = match existence {
        Existence::All => absolute,
        Existence::AllButLast => absolute.parent().unwrap_or(absolute),
        Existence::None => return Ok(()),
    };
    fs::symlink_metadata(target).map(|_| ())
}

/// An absolute form of `path` with `.` and `..` removed lexically. Relative paths are taken
/// from the working directory.
fn absolute_lexical(path: &Path) -> PathBuf {
    normalize_lexical(&make_absolute(path))
}

/// Removes `.` and resolves `..` against earlier components, without touching the disk.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(Component::RootDir);
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// The path of `target` relative to the directory `base`. Both must be absolute and
/// normalized; different roots (Windows drives) yield `target` itself.
fn relative_path(target: &Path, base: &Path) -> PathBuf {
    let target_parts: Vec<Component> = target.components().collect();
    let base_parts: Vec<Component> = base.components().collect();
    if target_parts.first() != base_parts.first() {
        return target.to_path_buf();
    }
    let common = target_parts
        .iter()
        .zip(base_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = PathBuf::new();
    for _ in common..base_parts.len() {
        out.push("..");
    }
    for part in &target_parts[common..] {
        out.push(part.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

#[cfg(test)]
mod tests;
