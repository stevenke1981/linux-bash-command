//! `stat`: displays file status. The default layout follows GNU 9.4; `-c`/`--printf` take
//! a format string, `-t` prints a terse line, and `-f` reports file-system status (Unix).

mod format;
mod fs;
pub(crate) mod platform;

use std::fs as stdfs;
use std::io::{self, Write};
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::datetime::zone_from_env;
use crate::common::error::{io_error_text, print_version, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::escape::{unescape, EscapeMode};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};
use crate::common::path::{display_path, strip_verbatim};
use format::FileInfo;

const HELP: &str = r#"Usage: stat [OPTION]... [FILE]...
Display file or file system status.

  -L, --dereference          follow links
  -f, --file-system          display file system status instead of file status
  -c  --format=FORMAT        use the specified FORMAT instead of the default;
                               output a newline after each use of FORMAT
      --printf=FORMAT        like --format, but interpret backslash escapes,
                               and do not output a mandatory trailing newline;
                               if you want a newline, include \n in FORMAT
  -t, --terse                print the information in terse form
      --cached=MODE          accepted for compatibility; has no effect
      --help     display this help and exit
      --version  output version information and exit

The valid format sequences for files (without --file-system):

  %a   access rights in octal
  %A   access rights in human readable form
  %b   number of blocks allocated (see %B)
  %B   the size in bytes of each block reported by %b
  %C   SELinux security context string (always '?' here)
  %d   device number in decimal
  %D   device number in hex
  %f   raw mode in hex
  %F   file type
  %g   group ID of owner
  %G   group name of owner
  %h   number of hard links
  %i   inode number
  %m   mount point
  %n   file name
  %N   quoted file name with dereference if symbolic link
  %o   optimal I/O transfer size hint
  %s   total size, in bytes
  %t   major device type in hex, for character/block device special files
  %T   minor device type in hex, for character/block device special files
  %u   user ID of owner
  %U   user name of owner
  %w   time of file birth, human-readable; - if unknown
  %W   time of file birth, seconds since Epoch; 0 if unknown
  %x   time of last access, human-readable
  %X   time of last access, seconds since Epoch
  %y   time of last data modification, human-readable
  %Y   time of last data modification, seconds since Epoch
  %z   time of last status change, human-readable
  %Z   time of last status change, seconds since Epoch

Valid format sequences for file systems:

  %a   free blocks available to non-superuser
  %b   total data blocks in file system
  %c   total file nodes in file system
  %d   free file nodes in file system
  %f   free blocks in file system
  %i   file system ID in hex
  %l   maximum length of filenames
  %n   file name
  %s   block size (for faster transfers)
  %S   fundamental block size (for block counts)
  %t   file system type in hex
  %T   file system type in human readable form

"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("dereference", Some('L'), Some("dereference")),
    OptSpec::flag("fs", Some('f'), Some("file-system")),
    OptSpec::req("format", Some('c'), Some("format")),
    OptSpec::req("printf", None, Some("printf")),
    OptSpec::flag("terse", Some('t'), Some("terse")),
    OptSpec::req("cached", None, Some("cached")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Parsed command line.
#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    dereference: bool,
    filesystem: bool,
    terse: bool,
    /// The format and whether it came from `--printf` (escapes on, no newline).
    format: Option<(String, bool)>,
    operands: Vec<String>,
}

/// Entry point.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("stat", SPECS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage("stat", &err, EXIT_FAIL),
    };
    let options = match collect(&items) {
        Ok(options) => options,
        Err(code) => return code,
    };
    if options.operands.is_empty() {
        eprintln!("stat: missing operand\nTry 'stat --help' for more information.");
        return EXIT_FAIL;
    }
    let zone = zone_from_env();
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    for operand in &options.operands {
        let result = if options.filesystem {
            show_filesystem(&mut out, operand, &options)
        } else {
            show_file(&mut out, operand, &options, &zone)
        };
        if let Err(message) = result {
            eprintln!("stat: {message}");
            status = EXIT_FAIL;
        }
    }
    let flushed = finish_output("stat", &mut out);
    if status == EXIT_OK {
        flushed
    } else {
        status
    }
}

/// Handles `--help` and `--version`, and gathers the rest.
fn collect(items: &[Item]) -> Result<Options, i32> {
    let mut options = Options::default();
    for item in items {
        match item {
            Item::Operand(text) => options.operands.push(text.clone()),
            Item::Opt { id, value } => match *id {
                "help" => {
                    print!("{HELP}");
                    return Err(EXIT_OK);
                }
                "version" => return Err(print_version("stat")),
                "dereference" => options.dereference = true,
                "fs" => {
                    if cfg!(not(unix)) {
                        return Err(unsupported("stat", "-f"));
                    }
                    options.filesystem = true;
                }
                "terse" => options.terse = true,
                "format" => options.format = Some((value.clone().unwrap_or_default(), false)),
                "printf" => options.format = Some((value.clone().unwrap_or_default(), true)),
                _ => {}
            },
        }
    }
    Ok(options)
}

/// Writes the text for one operand, or describes why it failed.
fn show_file(
    out: &mut impl Write,
    operand: &str,
    options: &Options,
    zone: &jiff::tz::TimeZone,
) -> Result<(), String> {
    let info = gather(operand, options.dereference)
        .map_err(|error| format!("cannot statx '{}': {}", operand, io_error_text(&error)))?;
    let text = match &options.format {
        Some((format, true)) => {
            let unescaped = unescape(format, EscapeMode::Printf).0;
            format::expand(&String::from_utf8_lossy(&unescaped), &info, zone)
        }
        Some((format, false)) => format!("{}\n", format::expand(format, &info, zone)),
        None if options.terse => format::terse_layout(&info),
        None => format::default_layout(&info, zone),
    };
    write_text(out, &text)
}

/// Writes the file-system status for one operand.
fn show_filesystem(out: &mut impl Write, operand: &str, options: &Options) -> Result<(), String> {
    let info = fs::read(operand).map_err(|error| {
        format!(
            "cannot read file system information for '{}': {}",
            operand,
            io_error_text(&error)
        )
    })?;
    let text = match &options.format {
        Some((format, true)) => {
            let unescaped = unescape(format, EscapeMode::Printf).0;
            fs::expand(&String::from_utf8_lossy(&unescaped), &info)
        }
        Some((format, false)) => format!("{}\n", fs::expand(format, &info)),
        None if options.terse => fs::terse_layout(&info),
        None => fs::default_layout(&info),
    };
    write_text(out, &text)
}

fn write_text(out: &mut impl Write, text: &str) -> Result<(), String> {
    match out.write_all(text.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) if is_broken_pipe(&error) => Ok(()),
        Err(error) => Err(format!("write error: {}", io_error_text(&error))),
    }
}

/// Reads everything `stat` shows about `operand`. Links are not followed unless `follow`.
fn gather(operand: &str, follow: bool) -> io::Result<FileInfo> {
    let path = Path::new(operand);
    let meta = if follow {
        stdfs::metadata(path)?
    } else {
        stdfs::symlink_metadata(path)?
    };
    let link = if !follow && meta.file_type().is_symlink() {
        stdfs::read_link(path)
            .ok()
            .map(|target| display_path(&target))
    } else {
        None
    };
    let raw = platform::raw_stat(path, &meta);
    let (user, group) = owner_names(raw.uid, raw.gid);
    Ok(FileInfo {
        name: operand.to_string(),
        link,
        user,
        group,
        mount: mount_point(path),
        stat: raw,
    })
}

#[cfg(unix)]
fn owner_names(uid: u32, gid: u32) -> (String, String) {
    (
        platform::user_name(uid).unwrap_or_else(|| uid.to_string()),
        platform::group_name(gid).unwrap_or_else(|| gid.to_string()),
    )
}

#[cfg(not(unix))]
fn owner_names(_uid: u32, _gid: u32) -> (String, String) {
    ("-".to_string(), "-".to_string())
}

/// The highest ancestor of `path` that is on the same device: the mount point.
fn mount_point(path: &Path) -> String {
    let Ok(absolute) = stdfs::canonicalize(path) else {
        return "?".to_string();
    };
    let Ok(device) = platform::device_of(&absolute) else {
        return "?".to_string();
    };
    let mut current = absolute.as_path();
    while let Some(parent) = current.parent() {
        match platform::device_of(parent) {
            Ok(dev) if dev == device => current = parent,
            _ => break,
        }
    }
    display_path(&strip_verbatim(current))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn options(list: &[&str]) -> Result<Options, i32> {
        let items = Parser::new("stat", SPECS).parse(&args(list)).unwrap();
        collect(&items)
    }

    #[test]
    fn operands_are_collected_in_order() {
        let opts = options(&["a", "-L", "b"]).unwrap();
        assert_eq!(opts.operands, vec!["a", "b"]);
        assert!(opts.dereference);
    }

    #[test]
    fn format_and_printf_record_escape_mode() {
        assert_eq!(
            options(&["-c", "%s"]).unwrap().format,
            Some(("%s".to_string(), false))
        );
        assert_eq!(
            options(&["--printf=%s\\n"]).unwrap().format,
            Some(("%s\\n".to_string(), true))
        );
    }

    #[test]
    fn terse_and_filesystem_flags() {
        assert!(options(&["-t", "x"]).unwrap().terse);
        #[cfg(unix)]
        assert!(options(&["-f", "x"]).unwrap().filesystem);
    }

    #[test]
    fn owner_names_fall_back_to_numbers_or_dash() {
        let (user, group) = owner_names(4_000_000_000, 4_000_000_000);
        assert!(!user.is_empty() && !group.is_empty());
    }

    #[test]
    fn mount_point_of_a_directory_exists() {
        let dir = std::env::temp_dir();
        assert!(!mount_point(&dir).is_empty());
    }

    #[test]
    fn missing_file_reports_statx_wording() {
        let info = gather("/definitely/not/here/agentcmd-stat", true);
        assert!(info.is_err());
    }
}
