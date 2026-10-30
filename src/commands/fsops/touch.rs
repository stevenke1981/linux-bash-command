//! `touch`: change file access and modification times, creating empty files as needed.

use std::fs::{self, OpenOptions};
use std::path::Path;

use filetime::{self, FileTime};

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

use super::helpers::{argmatch, os_text, print_help, ArgGroup};
use super::touch_time;

const HELP: &str = r#"Usage: touch [OPTION]... FILE...
Update the access and modification times of each FILE to the current time.

A FILE argument that does not exist is created empty, unless -c or -h
is supplied.

A FILE argument string of - is handled specially and causes touch to
change the times of the file associated with standard output.

Mandatory arguments to long options are mandatory for short options too.
  -a                     change only the access time
  -c, --no-create        do not create any files
  -d, --date=STRING      parse STRING and use it instead of current time
                         (accepts @EPOCH, now, and YYYY-MM-DD[ HH:MM[:SS]])
  -f                     (ignored)
  -h, --no-dereference   affect each symbolic link instead of any referenced
                         file
  -m                     change only the modification time
  -r, --reference=FILE   use this file's times instead of current time
  -t STAMP               use [[CC]YY]MMDDhhmm[.ss] instead of current time
      --time=WORD        change the specified time: WORD is access, atime, or
                         use: equivalent to -a; WORD is modify or mtime:
                         equivalent to -m
      --help             display this help and exit
      --version          output version information and exit
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("access", Some('a'), None),
    OptSpec::flag("no-create", Some('c'), Some("no-create")),
    OptSpec::req("date", Some('d'), Some("date")),
    OptSpec::flag("ignored", Some('f'), None),
    OptSpec::flag("no-deref", Some('h'), Some("no-dereference")),
    OptSpec::flag("modify", Some('m'), None),
    OptSpec::req("reference", Some('r'), Some("reference")),
    OptSpec::req("stamp", Some('t'), None),
    OptSpec::req("time", None, Some("time")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const TIME_GROUPS: &[ArgGroup<'_, Field>] = &[
    (&["access", "atime", "use"], Field::Access),
    (&["modify", "mtime"], Field::Modify),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Access,
    Modify,
}

/// Where the new times come from.
enum Source {
    Now,
    Date(String),
    Stamp(String),
    Reference(String),
}

/// Parsed options.
struct Options {
    access: bool,
    modify: bool,
    no_create: bool,
    no_deref: bool,
    source: Source,
    source_set: bool,
}

/// Times to apply to one file: `None` keeps the file's current value.
#[derive(Clone, Copy)]
struct Times {
    access: FileTime,
    modify: FileTime,
}

/// Turns the chosen source into concrete times.
fn base_times(source: &Source) -> Result<Times, String> {
    match source {
        Source::Now => {
            let now = FileTime::now();
            Ok(Times {
                access: now,
                modify: now,
            })
        }
        Source::Date(text) => {
            let time = touch_time::parse_date(text)
                .ok_or_else(|| format!("invalid date format '{text}'"))?;
            Ok(Times {
                access: time,
                modify: time,
            })
        }
        Source::Stamp(text) => {
            let time = touch_time::parse_stamp(text)
                .ok_or_else(|| format!("invalid date format '{text}'"))?;
            Ok(Times {
                access: time,
                modify: time,
            })
        }
        Source::Reference(path) => {
            let meta = fs::metadata(path).map_err(|error| {
                format!("failed to get attributes of '{path}': {}", os_text(&error))
            })?;
            Ok(Times {
                access: FileTime::from_last_access_time(&meta),
                modify: FileTime::from_last_modification_time(&meta),
            })
        }
    }
}

/// Current times of an existing file, used for the field that is not being changed.
fn current_times(path: &Path, no_deref: bool) -> Option<Times> {
    let meta = if no_deref {
        fs::symlink_metadata(path)
    } else {
        fs::metadata(path)
    }
    .ok()?;
    Some(Times {
        access: FileTime::from_last_access_time(&meta),
        modify: FileTime::from_last_modification_time(&meta),
    })
}

/// Sets the chosen fields of an existing path.
fn set_times(path: &Path, opts: &Options, times: Times) -> Result<(), std::io::Error> {
    let current = current_times(path, opts.no_deref);
    let access = if opts.access || !opts.modify {
        times.access
    } else {
        current.map_or(times.access, |c| c.access)
    };
    let modify = if opts.modify || !opts.access {
        times.modify
    } else {
        current.map_or(times.modify, |c| c.modify)
    };
    if opts.no_deref {
        filetime::set_symlink_file_times(path, access, modify)
    } else {
        filetime::set_file_times(path, access, modify)
    }
}

/// Touches one file. Returns false on failure (already reported).
fn touch_one(name: &str, opts: &Options, times: Times) -> bool {
    let path = Path::new(name);
    let exists = if opts.no_deref {
        fs::symlink_metadata(path).is_ok()
    } else {
        path.exists()
    };
    if !exists {
        if opts.no_create {
            return true;
        }
        if let Err(error) = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
        {
            eprintln!("touch: cannot touch '{name}': {}", os_text(&error));
            return false;
        }
    }
    match set_times(path, opts, times) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("touch: setting times of '{name}': {}", os_text(&error));
            false
        }
    }
}

/// Collects options. `Err` carries the exit status for an immediate return.
fn collect(items: Vec<Item>) -> Result<(Options, Vec<String>), i32> {
    let mut opts = Options {
        access: false,
        modify: false,
        no_create: false,
        no_deref: false,
        source: Source::Now,
        source_set: false,
    };
    let mut files = Vec::new();
    for item in items {
        let (id, value) = match item {
            Item::Operand(text) => {
                files.push(text);
                continue;
            }
            Item::Opt { id, value } => (id, value),
        };
        match id {
            "help" => return Err(print_help("touch", HELP)),
            "version" => return Err(print_version("touch")),
            "access" => opts.access = true,
            "modify" => opts.modify = true,
            "no-create" => opts.no_create = true,
            "no-deref" => opts.no_deref = true,
            "ignored" => {}
            "time" => {
                let field = argmatch("--time", value.as_deref().unwrap_or(""), TIME_GROUPS)
                    .map_err(|message| report_usage("touch", &UsageError { message }, EXIT_FAIL))?;
                match field {
                    Field::Access => opts.access = true,
                    Field::Modify => opts.modify = true,
                }
            }
            "date" => set_source(&mut opts, Source::Date(value.unwrap_or_default()))?,
            "stamp" => set_source(&mut opts, Source::Stamp(value.unwrap_or_default()))?,
            "reference" => set_source(&mut opts, Source::Reference(value.unwrap_or_default()))?,
            _ => {}
        }
    }
    Ok((opts, files))
}

/// Only one of `-d`, `-t` and `-r` may choose the times.
fn set_source(opts: &mut Options, source: Source) -> Result<(), i32> {
    if opts.source_set {
        eprintln!("touch: cannot specify times from more than one source");
        eprintln!("Try 'touch --help' for more information.");
        return Err(EXIT_FAIL);
    }
    opts.source_set = true;
    opts.source = source;
    Ok(())
}

/// Entry point for `touch`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("touch", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("touch", &error, EXIT_FAIL),
    };
    let (opts, files) = match collect(items) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    if files.is_empty() {
        return report_usage(
            "touch",
            &UsageError {
                message: "missing file operand".to_string(),
            },
            EXIT_FAIL,
        );
    }
    let times = match base_times(&opts.source) {
        Ok(times) => times,
        Err(message) => {
            eprintln!("touch: {message}");
            return EXIT_FAIL;
        }
    };
    let mut status = EXIT_OK;
    for file in &files {
        if !touch_one(file, &opts, times) {
            status = EXIT_FAIL;
        }
    }
    status
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;

    fn options(access: bool, modify: bool, no_create: bool) -> Options {
        Options {
            access,
            modify,
            no_create,
            no_deref: false,
            source: Source::Now,
            source_set: false,
        }
    }

    fn times_at(secs: i64) -> Times {
        let time = FileTime::from_unix_time(secs, 0);
        Times {
            access: time,
            modify: time,
        }
    }

    #[test]
    fn creates_a_missing_file() {
        let t = TempDir::new("touch-create");
        assert!(touch_one(
            t.at("new").to_str().unwrap(),
            &options(false, false, false),
            times_at(5)
        ));
        assert!(t.at("new").is_file());
    }

    #[test]
    fn no_create_leaves_missing_files_alone() {
        let t = TempDir::new("touch-nocreate");
        assert!(touch_one(
            t.at("nope").to_str().unwrap(),
            &options(false, false, true),
            times_at(5)
        ));
        assert!(!t.at("nope").exists());
    }

    #[test]
    fn sets_both_times_by_default() {
        let t = TempDir::new("touch-both");
        let file = t.write("a", "x");
        touch_one(
            file.to_str().unwrap(),
            &options(false, false, false),
            times_at(1_000_000),
        );
        let meta = fs::metadata(&file).unwrap();
        assert_eq!(
            FileTime::from_last_modification_time(&meta).unix_seconds(),
            1_000_000
        );
        assert_eq!(
            FileTime::from_last_access_time(&meta).unix_seconds(),
            1_000_000
        );
    }

    #[test]
    fn access_only_keeps_the_modification_time() {
        let t = TempDir::new("touch-access");
        let file = t.write("a", "x");
        touch_one(
            file.to_str().unwrap(),
            &options(false, false, false),
            times_at(1_000_000),
        );
        touch_one(
            file.to_str().unwrap(),
            &options(true, false, false),
            times_at(2_000_000),
        );
        let meta = fs::metadata(&file).unwrap();
        assert_eq!(
            FileTime::from_last_access_time(&meta).unix_seconds(),
            2_000_000
        );
        assert_eq!(
            FileTime::from_last_modification_time(&meta).unix_seconds(),
            1_000_000
        );
    }

    #[test]
    fn invalid_date_text_is_reported_by_base_times() {
        let error = base_times(&Source::Date("bogus".to_string()))
            .err()
            .unwrap();
        assert_eq!(error, "invalid date format 'bogus'");
        let stamp = base_times(&Source::Stamp("12".to_string())).err().unwrap();
        assert_eq!(stamp, "invalid date format '12'");
        assert!(base_times(&Source::Reference("/nonexistent-agentcmd-ref".to_string())).is_err());
    }
}
