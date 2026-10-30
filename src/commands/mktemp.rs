//! `mktemp`: creates a unique file or directory from a template ending in `X`s, or with
//! `-u`, only prints a name that does not exist yet.

use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::path::display_path;

const HELP: &str = r#"Usage: mktemp [OPTION]... [TEMPLATE]
Create a temporary file or directory, safely, and print its name.
TEMPLATE must contain at least 3 consecutive 'X's in last component.
If TEMPLATE is not specified, use tmp.XXXXXXXXXX, and --tmpdir is implied.
Files are created u+rw, and directories u+rwx, minus umask restrictions.

      --suffix=SUFF     append SUFF to TEMPLATE; SUFF must not contain a slash.
                          This option is implied if TEMPLATE does not end in X
  -d, --directory       create a directory, not a file
  -u, --dry-run         do not create anything; merely print a name (unsafe)
  -q, --quiet           suppress diagnostics about file/dir-creation failure
  -p DIR                use DIR as a prefix; implies -t [deprecated]
      --tmpdir[=DIR]    interpret TEMPLATE relative to DIR; if DIR is not
                          specified, use $TMPDIR if set, else /tmp.  With
                          this option, TEMPLATE must not be an absolute name;
                          unlike with -t, TEMPLATE may contain slashes, but
                          mktemp creates only the final component
  -t                    interpret TEMPLATE as a single file name component,
                          relative to a directory: $TMPDIR, if set; else the
                          directory specified via --tmpdir [deprecated], or
                          /tmp; see the description of --tmpdir
      --help     display this help and exit
      --version  output version information and exit
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::req("suffix", None, Some("suffix")),
    OptSpec::flag("directory", Some('d'), Some("directory")),
    OptSpec::flag("dry-run", Some('u'), Some("dry-run")),
    OptSpec::flag("quiet", Some('q'), Some("quiet")),
    OptSpec::req("tmpdir-flag", Some('p'), None),
    OptSpec::opt("tmpdir", None, Some("tmpdir")),
    OptSpec::flag("t", Some('t'), None),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Number of attempts before giving up on name collisions.
const ATTEMPTS: u32 = 100;

/// The characters substituted for `X`.
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

/// Parsed command line.
#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    directory: bool,
    dry_run: bool,
    quiet: bool,
    suffix: Option<String>,
    /// `-p DIR` or `--tmpdir=DIR`.
    tmpdir: Option<String>,
    /// `--tmpdir` or `-t` given without a directory: use the system temporary directory.
    use_system_tmp: bool,
    single_component: bool,
    template: Option<String>,
}

/// Entry point.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("mktemp", SPECS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage("mktemp", &err, EXIT_FAIL),
    };
    let options = match collect(&items) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let (dir, template) = match resolve_template(&options) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("mktemp: {message}");
            return EXIT_FAIL;
        }
    };
    let result = make(&options, &dir, &template);
    match result {
        Ok(path) => print_path(&path),
        Err(message) => {
            if !options.quiet {
                eprintln!("mktemp: {message}");
            }
            EXIT_FAIL
        }
    }
}

fn collect(items: &[Item]) -> Result<Options, i32> {
    let mut options = Options::default();
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text.clone()),
            Item::Opt { id, value } => match *id {
                "help" => {
                    print!("{HELP}");
                    return Err(EXIT_OK);
                }
                "version" => return Err(print_version("mktemp")),
                "suffix" => options.suffix = value.clone(),
                "directory" => options.directory = true,
                "dry-run" => options.dry_run = true,
                "quiet" => options.quiet = true,
                "tmpdir-flag" => {
                    options.tmpdir = value.clone();
                    options.single_component = true;
                }
                "tmpdir" => match value {
                    Some(dir) => options.tmpdir = Some(dir.clone()),
                    None => options.use_system_tmp = true,
                },
                "t" => options.single_component = true,
                _ => {}
            },
        }
    }
    match operands.as_slice() {
        [] => {}
        [single] => options.template = Some(single.clone()),
        [_, _, ..] => {
            eprintln!("mktemp: too many templates\nTry 'mktemp --help' for more information.");
            return Err(EXIT_FAIL);
        }
    }
    Ok(options)
}

/// Works out the directory to create in and the template's file name.
fn resolve_template(options: &Options) -> Result<(PathBuf, String), String> {
    let template = options
        .template
        .clone()
        .unwrap_or_else(|| "tmp.XXXXXXXXXX".to_string());
    if options.single_component && template.contains(['/', '\\']) {
        return Err(format!(
            "invalid template, '{template}', contains directory separator"
        ));
    }
    let implied_system_dir = options.template.is_none() || options.use_system_tmp;
    let base = match &options.tmpdir {
        Some(dir) => PathBuf::from(dir),
        None if options.single_component || implied_system_dir => std::env::temp_dir(),
        // A relative template is created relative to the working directory.
        None => PathBuf::new(),
    };
    Ok((base, template))
}

/// Creates the file or directory, or (with `-u`) picks a free name. Returns the printed path.
fn make(options: &Options, dir: &Path, template: &str) -> Result<PathBuf, String> {
    let (stem, x_count, suffix) = split_template(template, options.suffix.as_deref())?;
    let mut last_error = None;
    for _ in 0..ATTEMPTS {
        let candidate = dir.join(format!("{stem}{}{suffix}", random_chars(x_count)));
        if options.dry_run {
            return Ok(candidate);
        }
        match create(&candidate, options.directory) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_error = Some(error);
            }
            Err(error) => {
                let kind = if options.directory {
                    "directory"
                } else {
                    "file"
                };
                return Err(format!(
                    "failed to create {kind} via template '{template}': {}",
                    io_error_text(&error)
                ));
            }
        }
    }
    let error = last_error.unwrap_or_else(|| io::Error::from(io::ErrorKind::AlreadyExists));
    Err(format!(
        "failed to create file via template '{template}': {}",
        io_error_text(&error)
    ))
}

/// Splits a template into the text before its last run of `X`s, the run's length and the
/// suffix. `--suffix` is appended to the template, which must then end in `X`.
fn split_template(
    template: &str,
    suffix_option: Option<&str>,
) -> Result<(String, usize, String), String> {
    let (body, suffix) = match suffix_option {
        Some(suffix) => {
            if !template.ends_with('X') {
                return Err(format!(
                    "with --suffix, template '{template}' must end in X"
                ));
            }
            (template, suffix.to_string())
        }
        None => (template, String::new()),
    };
    if suffix.contains('/') {
        return Err(format!(
            "invalid suffix '{suffix}', contains directory separator"
        ));
    }
    let run_end = match body.rfind('X') {
        Some(index) => index + 1,
        None => return Err(format!("too few X's in template '{template}'")),
    };
    let run_start = body[..run_end]
        .rfind(|c: char| c != 'X')
        .map_or(0, |index| index + 1);
    let count = run_end - run_start;
    if count < 3 {
        return Err(format!("too few X's in template '{template}'"));
    }
    let tail = &body[run_end..];
    if tail.contains('/') {
        return Err(format!(
            "invalid suffix '{tail}', contains directory separator"
        ));
    }
    let tail_suffix = format!("{tail}{suffix}");
    Ok((body[..run_start].to_string(), count, tail_suffix))
}

fn create(path: &Path, directory: bool) -> io::Result<()> {
    if directory {
        create_dir(path)
    } else {
        create_file(path)
    }
}

#[cfg(unix)]
fn create_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map(|_| ())
}

#[cfg(not(unix))]
fn create_file(path: &Path) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
}

#[cfg(unix)]
fn create_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_dir(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}

/// `count` pseudo-random characters from [`ALPHABET`]. The generator is seeded from the
/// standard library's per-process random state, which is enough to avoid collisions.
fn random_chars(count: usize) -> String {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    let mut seed = std::process::id() as u64;
    let mut text = String::with_capacity(count);
    for _ in 0..count {
        hasher.write_u64(seed);
        seed = hasher.finish();
        text.push(ALPHABET[(seed % ALPHABET.len() as u64) as usize] as char);
    }
    text
}

fn print_path(path: &Path) -> i32 {
    let mut out = stdout_writer();
    let result = writeln!(out, "{}", display_path(path));
    match result {
        Err(error) if !is_broken_pipe(&error) => {
            eprintln!("mktemp: write error: {}", io_error_text(&error));
            return EXIT_FAIL;
        }
        _ => {}
    }
    finish_output("mktemp", &mut out)
}

#[cfg(test)]
mod tests;
