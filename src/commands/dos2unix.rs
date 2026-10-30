//! `dos2unix` and `unix2dos`: convert line endings between CRLF and LF and handle the
//! UTF-8 byte order mark. Files are converted in place through a temporary file in the
//! same directory, or into a new file with `-n`; with no operands the tools filter standard
//! input to standard output. Specification: docs/spec/coreutils-alignment/04-commands-extra.md
//! (dos2unix 7.x subset).

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const OPTS: &[OptSpec] = &[
    OptSpec::flag("newfile", Some('n'), Some("newfile")),
    OptSpec::flag("oldfile", Some('o'), Some("oldfile")),
    OptSpec::flag("keepdate", Some('k'), Some("keepdate")),
    OptSpec::flag("quiet", Some('q'), Some("quiet")),
    OptSpec::flag("force", Some('f'), Some("force")),
    OptSpec::flag("keep-bom", Some('b'), Some("keep-bom")),
    OptSpec::flag("remove-bom", Some('r'), Some("remove-bom")),
    OptSpec::flag("add-bom", Some('m'), Some("add-bom")),
    OptSpec::req("convmode", Some('c'), Some("convmode")),
    OptSpec::flag("add-eol", Some('e'), Some("add-eol")),
    OptSpec::flag("safe", Some('s'), Some("safe")),
    OptSpec::flag("help", Some('h'), Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const UTF8_BOM: [u8; 3] = [0xef, 0xbb, 0xbf];
const CHUNK: usize = 64 * 1024;

/// Which way the line endings are converted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Dos2Unix,
    Unix2Dos,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Dos2Unix => "dos2unix",
            Tool::Unix2Dos => "unix2dos",
        }
    }

    fn target(self) -> &'static str {
        match self {
            Tool::Dos2Unix => "Unix",
            Tool::Unix2Dos => "DOS",
        }
    }
}

/// What to do with a byte order mark at the start of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BomMode {
    Keep,
    Remove,
    Add,
}

#[derive(Debug)]
struct Options {
    tool: Tool,
    keepdate: bool,
    quiet: bool,
    force: bool,
    bom: BomMode,
    add_eol: bool,
}

/// Entry point for `dos2unix` and the `agentcmd dos2unix` multi-call form.
pub fn dos2unix(args: &[String]) -> i32 {
    run(Tool::Dos2Unix, args)
}

/// Entry point for `unix2dos` and the `agentcmd unix2dos` multi-call form.
pub fn unix2dos(args: &[String]) -> i32 {
    run(Tool::Unix2Dos, args)
}

fn run(tool: Tool, args: &[String]) -> i32 {
    let cmd = tool.name();
    let items = match Parser::new(cmd, OPTS).parse(args) {
        Ok(items) => items,
        Err(err) => return report_usage(cmd, &err, EXIT_FAIL),
    };
    let mut options = Options {
        tool,
        keepdate: false,
        quiet: false,
        force: false,
        bom: match tool {
            Tool::Dos2Unix => BomMode::Remove,
            Tool::Unix2Dos => BomMode::Keep,
        },
        add_eol: false,
    };
    let mut paired = false;
    let mut files = Vec::new();
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut pending_input: Option<String> = None;
    for item in items {
        match item {
            Item::Operand(text) if paired => match pending_input.take() {
                Some(input) => pairs.push((input, text)),
                None => pending_input = Some(text),
            },
            Item::Operand(text) => files.push(text),
            Item::Opt { id, value } => match id {
                "help" => return write_help(tool),
                "version" => return print_version(cmd),
                "newfile" => paired = true,
                "convmode" => {
                    let mode = value.unwrap_or_default();
                    if mode != "ascii" {
                        eprintln!("{cmd}: invalid {mode} conversion mode specified");
                        return EXIT_FAIL;
                    }
                }
                other => apply_flag(other, &mut options),
            },
        }
    }
    let mut status = EXIT_OK;
    if files.is_empty() && !paired {
        return filter_stdin(&options);
    }
    for name in &files {
        status = status.max(if name == "-" {
            filter_stdin(&options)
        } else {
            convert_in_place(&options, name)
        });
    }
    for (input, output) in &pairs {
        status = status.max(convert_to_new(&options, input, output));
    }
    status
}

fn apply_flag(id: &str, options: &mut Options) {
    match id {
        "keepdate" => options.keepdate = true,
        "quiet" => options.quiet = true,
        "force" => options.force = true,
        "keep-bom" => options.bom = BomMode::Keep,
        "remove-bom" => options.bom = BomMode::Remove,
        "add-bom" => options.bom = BomMode::Add,
        "add-eol" => options.add_eol = true,
        // -o and -s are the defaults.
        _ => {}
    }
}

fn write_help(tool: Tool) -> i32 {
    let mut out = stdout_writer();
    let text = HELP.replace("{cmd}", tool.name()).replace(
        "{direction}",
        match tool {
            Tool::Dos2Unix => {
                "Convert text files from DOS/Windows line endings (CRLF) to Unix (LF)."
            }
            Tool::Unix2Dos => {
                "Convert text files from Unix line endings (LF) to DOS/Windows (CRLF)."
            }
        },
    );
    if out.write_all(text.as_bytes()).is_err() {
        return EXIT_OK;
    }
    finish_output(tool.name(), &mut out)
}

/// Converts standard input to standard output.
fn filter_stdin(options: &Options) -> i32 {
    let cmd = options.tool.name();
    let mut data = Vec::new();
    if let Err(error) = io::stdin().lock().read_to_end(&mut data) {
        eprintln!("{cmd}: standard input: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    if !options.force && has_binary_bytes(&mut &data[..]).unwrap_or(false) {
        if !options.quiet {
            eprintln!("{cmd}: Skipping binary file stdin");
        }
        return EXIT_FAIL;
    }
    let mut out = stdout_writer();
    match convert(&mut &data[..], &mut out, options) {
        Ok(()) => finish_output(cmd, &mut out),
        Err(error) if is_broken_pipe(&error) => EXIT_OK,
        Err(error) => {
            eprintln!("{cmd}: write error: {}", io_error_text(&error));
            EXIT_FAIL
        }
    }
}

/// Outcome of looking at one input operand before converting it.
enum Check {
    Convert(Metadata),
    Skip(i32),
}

/// Checks that `name` is a regular, text file that should be converted. Missing files
/// are errors; symbolic links, other non-regular files and binary files are skipped.
fn check_input(options: &Options, name: &str) -> Check {
    let cmd = options.tool.name();
    let path = Path::new(name);
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) => {
            eprintln!("{cmd}: {name}: {}", io_error_text(&error));
            eprintln!("{cmd}: Skipping {name}, not a regular file.");
            return Check::Skip(EXIT_TROUBLE);
        }
    };
    if meta.file_type().is_symlink() {
        eprintln!("{cmd}: Skipping symbolic link {name}.");
        return Check::Skip(EXIT_OK);
    }
    if !meta.is_file() {
        eprintln!("{cmd}: Skipping {name}, not a regular file.");
        return Check::Skip(EXIT_OK);
    }
    if !options.force {
        let binary = File::open(path).and_then(|mut file| has_binary_bytes(&mut file));
        match binary {
            Ok(false) => {}
            Ok(true) => {
                if !options.quiet {
                    eprintln!("{cmd}: Skipping binary file {name}");
                }
                return Check::Skip(EXIT_OK);
            }
            Err(error) => {
                eprintln!("{cmd}: {name}: {}", io_error_text(&error));
                return Check::Skip(EXIT_FAIL);
            }
        }
    }
    Check::Convert(meta)
}

/// Converts one file in place: the result is written to a temporary file beside it, which
/// then replaces the original.
fn convert_in_place(options: &Options, name: &str) -> i32 {
    let cmd = options.tool.name();
    let meta = match check_input(options, name) {
        Check::Convert(meta) => meta,
        Check::Skip(code) => return code,
    };
    if !options.quiet {
        eprintln!(
            "{cmd}: converting file {name} to {} format...",
            options.tool.target()
        );
    }
    match replace_file(Path::new(name), &meta, options) {
        Ok(()) => EXIT_OK,
        Err(error) => {
            eprintln!(
                "{cmd}: problems converting file {name}: {}",
                io_error_text(&error)
            );
            EXIT_FAIL
        }
    }
}

fn replace_file(path: &Path, meta: &Metadata, options: &Options) -> io::Result<()> {
    let (temp_path, temp) = create_temp(path)?;
    let result = (|| {
        let mut input = File::open(path)?;
        let mut writer = BufWriter::new(temp);
        convert(&mut input, &mut writer, options)?;
        let file = writer.into_inner().map_err(|error| error.into_error())?;
        drop(file);
        fs::set_permissions(&temp_path, meta.permissions())?;
        finish_metadata(&temp_path, meta, options)?;
        fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Converts one operand into a new file (`-n INFILE OUTFILE`).
fn convert_to_new(options: &Options, input: &str, output: &str) -> i32 {
    let cmd = options.tool.name();
    let meta = match check_input(options, input) {
        Check::Convert(meta) => meta,
        Check::Skip(code) => return code,
    };
    if !options.quiet {
        eprintln!(
            "{cmd}: converting file {input} to file {output} in {} format...",
            options.tool.target()
        );
    }
    let result = (|| -> io::Result<()> {
        let mut reader = File::open(input)?;
        let file = File::create(output)?;
        let mut writer = BufWriter::new(file);
        convert(&mut reader, &mut writer, options)?;
        let file = writer.into_inner().map_err(|error| error.into_error())?;
        drop(file);
        fs::set_permissions(output, meta.permissions())?;
        finish_metadata(Path::new(output), &meta, options)
    })();
    match result {
        Ok(()) => EXIT_OK,
        Err(error) => {
            eprintln!("{cmd}: {output}: {}", io_error_text(&error));
            EXIT_FAIL
        }
    }
}

/// Applies `-k`: the output keeps the input's modification time.
fn finish_metadata(path: &Path, meta: &Metadata, options: &Options) -> io::Result<()> {
    if options.keepdate {
        let time = filetime::FileTime::from_last_modification_time(meta);
        filetime::set_file_mtime(path, time)?;
    }
    Ok(())
}

/// Creates an unused temporary file in the directory of `path`.
fn create_temp(path: &Path) -> io::Result<(PathBuf, File)> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pid = std::process::id();
    let mut attempt = 0u32;
    loop {
        let candidate = dir.join(format!(".{base}.{pid}.{attempt}.tmp"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Streams `reader` through the conversion into `out`.
fn convert(reader: &mut dyn Read, out: &mut dyn Write, options: &Options) -> io::Result<()> {
    let mut head = [0u8; 3];
    let filled = read_head(reader, &mut head)?;
    let has_bom = filled == UTF8_BOM.len() && head == UTF8_BOM;
    let write_bom = match options.bom {
        BomMode::Keep => has_bom,
        BomMode::Remove => false,
        BomMode::Add => true,
    };
    if write_bom {
        out.write_all(&UTF8_BOM)?;
    }
    let mut converter = LineConverter::new(options);
    if !has_bom {
        converter.feed(&head[..filled], out)?;
    }
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        converter.feed(&buffer[..count], out)?;
    }
    converter.finish(out)
}

/// Reads up to three bytes, returning how many were read.
fn read_head(reader: &mut dyn Read, head: &mut [u8; 3]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < head.len() {
        match reader.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

/// Converts line endings chunk by chunk. A CR is held back until the next byte is known.
struct LineConverter {
    tool: Tool,
    add_eol: bool,
    pending_cr: bool,
    previous: Option<u8>,
}

impl LineConverter {
    fn new(options: &Options) -> Self {
        Self {
            tool: options.tool,
            add_eol: options.add_eol,
            pending_cr: false,
            previous: None,
        }
    }

    fn feed(&mut self, bytes: &[u8], out: &mut dyn Write) -> io::Result<()> {
        let mut text = Vec::with_capacity(bytes.len() + 16);
        for &byte in bytes {
            match self.tool {
                Tool::Dos2Unix => {
                    if self.pending_cr {
                        self.pending_cr = false;
                        if byte != b'\n' {
                            text.push(b'\r');
                        }
                    }
                    if byte == b'\r' {
                        self.pending_cr = true;
                    } else {
                        text.push(byte);
                    }
                }
                Tool::Unix2Dos => {
                    if byte == b'\n' && self.previous != Some(b'\r') {
                        text.push(b'\r');
                    }
                    text.push(byte);
                }
            }
            self.previous = Some(byte);
        }
        out.write_all(&text)
    }

    fn finish(&mut self, out: &mut dyn Write) -> io::Result<()> {
        let mut tail = Vec::new();
        if self.pending_cr {
            tail.push(b'\r');
        }
        let missing_break = matches!(self.previous, Some(byte) if byte != b'\n');
        if self.add_eol && missing_break {
            match self.tool {
                Tool::Dos2Unix => tail.push(b'\n'),
                Tool::Unix2Dos => tail.extend_from_slice(b"\r\n"),
            }
        }
        out.write_all(&tail)
    }
}

/// True when the data holds a NUL or a control byte that text files do not contain.
/// Tab, newline, form feed and carriage return are text.
fn is_binary_byte(byte: u8) -> bool {
    byte < 0x20 && !matches!(byte, b'\t' | b'\n' | 0x0c | b'\r')
}

fn has_binary_bytes(reader: &mut dyn Read) -> io::Result<bool> {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => return Ok(false),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if buffer[..count].iter().any(|byte| is_binary_byte(*byte)) {
            return Ok(true);
        }
    }
}

const HELP: &str = "\
Usage: {cmd} [options] [file ...] [-n infile outfile ...]
{direction}
With no file, read standard input and write standard output.

  -b, --keep-bom        keep the byte order mark (BOM)
  -r, --remove-bom      remove the byte order mark (BOM)
  -m, --add-bom         add a UTF-8 byte order mark (BOM)
  -c, --convmode ascii  conversion mode (only ascii is supported)
  -e, --add-eol         add a line break to the last line if there isn't one
  -f, --force           convert binary files too
  -k, --keepdate        keep the output file's modification date
  -n, --newfile         write to new files: each INFILE is followed by OUTFILE
  -o, --oldfile         convert the files in place (default)
  -q, --quiet           suppress normal messages
  -s, --safe            skip binary files (default)
  -h, --help            show this help text
      --version         output version information and exit

Exit status is 0 on success, 1 on error, and 2 when an operand does not exist.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn options(tool: Tool) -> Options {
        Options {
            tool,
            keepdate: false,
            quiet: false,
            force: false,
            bom: match tool {
                Tool::Dos2Unix => BomMode::Remove,
                Tool::Unix2Dos => BomMode::Keep,
            },
            add_eol: false,
        }
    }

    fn run_convert(tool: Tool, input: &[u8], edit: impl FnOnce(&mut Options)) -> Vec<u8> {
        let mut opts = options(tool);
        edit(&mut opts);
        let mut out = Vec::new();
        convert(&mut &input[..], &mut out, &opts).unwrap();
        out
    }

    #[test]
    fn dos2unix_turns_crlf_into_lf_and_keeps_lone_cr() {
        assert_eq!(
            run_convert(Tool::Dos2Unix, b"a\r\nb\rc\r\n", |_| {}),
            b"a\nb\rc\n"
        );
    }

    #[test]
    fn dos2unix_keeps_a_trailing_cr_without_lf() {
        assert_eq!(run_convert(Tool::Dos2Unix, b"a\r", |_| {}), b"a\r");
        assert_eq!(run_convert(Tool::Dos2Unix, b"a\r\r\n", |_| {}), b"a\r\n");
    }

    #[test]
    fn unix2dos_adds_cr_except_before_existing_crlf() {
        assert_eq!(
            run_convert(Tool::Unix2Dos, b"a\nb\r\n", |_| {}),
            b"a\r\nb\r\n"
        );
        assert_eq!(run_convert(Tool::Unix2Dos, b"a\r\r\n", |_| {}), b"a\r\r\n");
    }

    #[test]
    fn carriage_return_split_across_chunks_still_pairs_with_lf() {
        let mut converter = LineConverter::new(&options(Tool::Dos2Unix));
        let mut out = Vec::new();
        converter.feed(b"a\r", &mut out).unwrap();
        converter.feed(b"\nb", &mut out).unwrap();
        converter.finish(&mut out).unwrap();
        assert_eq!(out, b"a\nb");
    }

    #[test]
    fn add_eol_terminates_the_last_line_only_when_missing() {
        let add = |opts: &mut Options| opts.add_eol = true;
        assert_eq!(run_convert(Tool::Dos2Unix, b"a\r\nb", add), b"a\nb\n");
        assert_eq!(run_convert(Tool::Unix2Dos, b"a\nb", add), b"a\r\nb\r\n");
        assert_eq!(run_convert(Tool::Unix2Dos, b"a\n", add), b"a\r\n");
        assert_eq!(run_convert(Tool::Unix2Dos, b"", add), b"");
    }

    #[test]
    fn byte_order_mark_is_removed_kept_or_added() {
        let bom = b"\xef\xbb\xbfa\r\n";
        assert_eq!(run_convert(Tool::Dos2Unix, bom, |_| {}), b"a\n");
        assert_eq!(
            run_convert(Tool::Dos2Unix, bom, |o| o.bom = BomMode::Keep),
            b"\xef\xbb\xbfa\n"
        );
        let added = run_convert(Tool::Unix2Dos, b"a\n", |o| o.bom = BomMode::Add);
        assert_eq!(added, b"\xef\xbb\xbfa\r\n");
        let once = run_convert(Tool::Unix2Dos, bom, |o| o.bom = BomMode::Add);
        assert_eq!(once, b"\xef\xbb\xbfa\r\n");
    }

    #[test]
    fn control_bytes_mark_data_as_binary() {
        assert!(is_binary_byte(0x00));
        assert!(is_binary_byte(0x1b));
        assert!(is_binary_byte(0x08));
        assert!(!is_binary_byte(b'\t'));
        assert!(!is_binary_byte(0x0c));
        assert!(!is_binary_byte(0x7f));
        assert!(!is_binary_byte(0xe9));
    }
}
