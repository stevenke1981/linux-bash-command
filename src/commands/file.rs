//! `file`: identifies file types from their first bytes and content (a libmagic subset).
//!
//! Detection order follows file(1): missing, directory, symbolic link, empty, magic
//! signatures, text, and finally `data`. Only the first 1 MiB of a file is examined.
//! Golden tests cover each branch; there is no GNU differential case because libmagic's
//! database is much larger than this subset.

mod magic;
mod text;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, report, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, stdout_writer};
use crate::common::path::display_path;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("brief", Some('b'), Some("brief")),
    OptSpec::flag("mime", Some('i'), Some("mime")),
    OptSpec::flag("mime_type", None, Some("mime-type")),
    OptSpec::flag("mime_encoding", None, Some("mime-encoding")),
    OptSpec::flag("dereference", Some('L'), Some("dereference")),
    OptSpec::flag("no_dereference", Some('h'), Some("no-dereference")),
    OptSpec::req("files_from", Some('f'), Some("files-from")),
    OptSpec::flag("print0", Some('0'), Some("print0")),
    OptSpec::flag("zero", Some('z'), None),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "\
Usage: file [OPTION...] [FILE...]
Determine type of FILEs.

  -b, --brief                do not prepend filenames to output lines
  -i, --mime                 output MIME type strings (MIME type and encoding)
      --mime-type            output the MIME type only
      --mime-encoding        output the MIME encoding only
  -L, --dereference          follow symlinks
  -h, --no-dereference       do not follow symlinks (default)
  -f, --files-from=FILE      read the names of the files to examine from FILE
  -0, --print0               terminate filenames with ASCII NUL
      --help                 display this help and exit
      --version              output version information and exit

Not supported by agentcmd: -z (uncompress archives).

Detection uses a built-in subset of the libmagic database; the first 1 MiB of each
file is examined.

Exit status is 0 on success, even when a file cannot be examined (its line says so).
";

/// What a file has been identified as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Human-readable description, as printed by default.
    pub description: String,
    /// MIME type without parameters.
    pub mime: String,
    /// MIME charset parameter, such as `us-ascii`, `utf-8` or `binary`.
    pub charset: String,
}

/// How much of each file is examined.
const SAMPLE_BYTES: u64 = 1024 * 1024;

#[derive(Default)]
struct Cli {
    brief: bool,
    mime: bool,
    mime_type: bool,
    mime_encoding: bool,
    dereference: bool,
    files_from: Option<String>,
    print0: bool,
    operands: Vec<String>,
}

/// Entry point for the `file` command.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("file", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("file", &error, EXIT_FAIL),
    };
    let cli = match collect(items) {
        Ok(cli) => cli,
        Err(status) => return status,
    };
    let mut names = cli.operands.clone();
    if let Some(list) = &cli.files_from {
        match read_name_list(list) {
            Ok(more) => names.extend(more),
            Err(error) => {
                report("file", list, &error);
                return EXIT_FAIL;
            }
        }
    }
    if names.is_empty() {
        eprintln!("Usage: file [OPTION...] [FILE...]\nTry 'file --help' for more information.");
        return EXIT_FAIL;
    }
    let mut out = stdout_writer();
    for name in &names {
        let line = examine(&cli, name);
        let _ = out.write_all(line.as_bytes());
    }
    finish_output("file", &mut out)
}

fn collect(items: Vec<Item>) -> Result<Cli, i32> {
    let mut cli = Cli::default();
    for item in items {
        match item {
            Item::Operand(text) => cli.operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => {
                    print!("{HELP}");
                    return Err(EXIT_OK);
                }
                "version" => return Err(print_version("file")),
                "zero" => return Err(unsupported("file", "-z")),
                "brief" => cli.brief = true,
                "mime" => cli.mime = true,
                "mime_type" => cli.mime_type = true,
                "mime_encoding" => cli.mime_encoding = true,
                "dereference" => cli.dereference = true,
                "no_dereference" => cli.dereference = false,
                "files_from" => cli.files_from = value,
                "print0" => cli.print0 = true,
                _ => {}
            },
        }
    }
    Ok(cli)
}

/// Reads one name per line from `path` (`-` for standard input).
fn read_name_list(path: &str) -> io::Result<Vec<String>> {
    let mut text = String::new();
    if path == "-" {
        io::stdin().read_to_string(&mut text)?;
    } else {
        text = fs::read_to_string(path)?;
    }
    Ok(text
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

/// The output line for one operand.
fn examine(cli: &Cli, name: &str) -> String {
    let shown = if name == "-" {
        "/dev/stdin".to_string()
    } else {
        display_path(Path::new(name))
    };
    let body = match identify(cli, name) {
        Ok(verdict) => render(cli, &verdict),
        Err(error) => format!("cannot open `{shown}' ({})", io_error_text(&error)),
    };
    if cli.brief {
        format!("{body}\n")
    } else if cli.print0 {
        format!("{shown}\0: {body}\n")
    } else {
        format!("{shown}: {body}\n")
    }
}

/// The text printed after the name for a verdict, honouring the MIME options.
fn render(cli: &Cli, verdict: &Verdict) -> String {
    if cli.mime_type {
        verdict.mime.clone()
    } else if cli.mime_encoding {
        verdict.charset.clone()
    } else if cli.mime {
        format!("{}; charset={}", verdict.mime, verdict.charset)
    } else {
        verdict.description.clone()
    }
}

/// Identifies one operand, or reports why it cannot be examined.
fn identify(cli: &Cli, name: &str) -> io::Result<Verdict> {
    if name == "-" {
        let mut sample = Vec::new();
        io::stdin().take(SAMPLE_BYTES).read_to_end(&mut sample)?;
        return Ok(classify(&sample));
    }
    let path = Path::new(name);
    let link = fs::symlink_metadata(path)?;
    if link.file_type().is_symlink() && !cli.dereference {
        let target = fs::read_link(path)?;
        return Ok(special(
            format!("symbolic link to {}", display_path(&target)),
            "inode/symlink",
        ));
    }
    let meta = fs::metadata(path)?;
    if meta.is_dir() {
        return Ok(special("directory".to_string(), "inode/directory"));
    }
    if let Some(verdict) = special_file(&meta) {
        return Ok(verdict);
    }
    let mut sample = Vec::new();
    File::open(path)?
        .take(SAMPLE_BYTES)
        .read_to_end(&mut sample)?;
    Ok(classify(&sample))
}

/// A verdict with no charset: the description and MIME type are all there is.
fn special(description: String, mime: &str) -> Verdict {
    Verdict {
        description,
        mime: mime.to_string(),
        charset: "binary".to_string(),
    }
}

#[cfg(unix)]
fn special_file(meta: &fs::Metadata) -> Option<Verdict> {
    use std::os::unix::fs::FileTypeExt;
    let file_type = meta.file_type();
    let (description, mime) = if file_type.is_fifo() {
        ("fifo (named pipe)", "inode/fifo")
    } else if file_type.is_socket() {
        ("socket", "inode/socket")
    } else if file_type.is_char_device() {
        ("character special", "inode/chardevice")
    } else if file_type.is_block_device() {
        ("block special", "inode/blockdevice")
    } else {
        return None;
    };
    Some(special(description.to_string(), mime))
}

#[cfg(not(unix))]
fn special_file(_meta: &fs::Metadata) -> Option<Verdict> {
    None
}

/// Classifies file contents: empty, magic signatures, text, then `data`.
pub fn classify(sample: &[u8]) -> Verdict {
    if sample.is_empty() {
        return special("empty".to_string(), "inode/x-empty");
    }
    if let Some(found) = magic::detect(sample) {
        return found;
    }
    if let Some(found) = text::describe(sample) {
        return found;
    }
    special("data".to_string(), "application/octet-stream")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_data_and_text_fallbacks() {
        assert_eq!(classify(b"").description, "empty");
        assert_eq!(classify(b"a\x00b").description, "data");
        assert_eq!(classify(b"hello\n").description, "ASCII text");
        assert_eq!(classify(b"hello\n").charset, "us-ascii");
    }

    #[test]
    fn render_honours_mime_options() {
        let verdict = classify(b"hi\n");
        let mut cli = Cli::default();
        assert_eq!(render(&cli, &verdict), "ASCII text");
        cli.mime = true;
        assert_eq!(render(&cli, &verdict), "text/plain; charset=us-ascii");
        cli.mime = false;
        cli.mime_type = true;
        assert_eq!(render(&cli, &verdict), "text/plain");
        cli.mime_type = false;
        cli.mime_encoding = true;
        assert_eq!(render(&cli, &verdict), "us-ascii");
    }

    #[test]
    fn missing_file_is_reported_in_the_line() {
        let cli = Cli::default();
        let line = examine(&cli, "/nonexistent-agentcmd-file-xyz");
        assert!(
            line.contains("cannot open `/nonexistent-agentcmd-file-xyz'"),
            "{line}"
        );
        assert!(line.ends_with("(No such file or directory)\n"), "{line}");
    }
}
