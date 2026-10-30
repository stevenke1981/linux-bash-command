//! `gzip`, `gunzip` and `zcat`: GNU gzip 1.12 behaviour on top of flate2.
//!
//! Compressed bytes are produced by flate2 and therefore differ from GNU's; the decoded
//! data, `-l` figures, exit codes and messages follow GNU.

mod codec;
mod files;
mod list;
mod options;

use std::fs::{self, Metadata};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use crate::common::error::{io_error_text, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, stdout_writer};
use crate::common::path::display_path;
use codec::{DecodeError, Summary};
use options::{Mode, Options, Prog, MSG_NAME};

/// Entry point for `gzip`.
pub fn gzip(args: &[String]) -> i32 {
    drive(Prog::Gzip, args)
}

/// Entry point for `gunzip`.
pub fn gunzip(args: &[String]) -> i32 {
    drive(Prog::Gunzip, args)
}

/// Entry point for `zcat`.
pub fn zcat(args: &[String]) -> i32 {
    drive(Prog::Zcat, args)
}

/// Error and warning counts. GNU exits 1 when any error occurred, otherwise 2 for warnings.
#[derive(Default)]
struct Status {
    errors: u32,
    warnings: u32,
    /// Container overhead of the last file listed successfully (`-l` totals use it).
    last_overhead: u64,
}

impl Status {
    fn exit_code(&self) -> i32 {
        if self.errors > 0 {
            EXIT_FAIL
        } else if self.warnings > 0 {
            2
        } else {
            EXIT_OK
        }
    }
}

/// Running totals for the `-l` summary line.
#[derive(Default)]
struct Totals {
    compressed: u64,
    uncompressed: u64,
    listed: bool,
}

struct Job<'a> {
    opts: &'a Options,
    status: Status,
    totals: Totals,
    header_printed: bool,
    out: BufWriter<std::io::Stdout>,
}

fn drive(prog: Prog, args: &[String]) -> i32 {
    let opts = match options::parse(prog, args) {
        Ok(opts) => opts,
        Err(code) => return code,
    };
    let mut job = Job {
        opts: &opts,
        status: Status::default(),
        totals: Totals::default(),
        header_printed: false,
        out: stdout_writer(),
    };
    if opts.files.is_empty() {
        job.process_stdin();
    } else {
        for operand in &opts.files {
            job.process_operand(operand);
        }
        if opts.mode == Mode::List && opts.files.len() > 1 && job.totals.listed {
            job.print_totals();
        }
    }
    let flush = finish_output(MSG_NAME, &mut job.out);
    if flush != EXIT_OK && job.status.exit_code() == EXIT_OK {
        return flush;
    }
    job.status.exit_code()
}

impl Job<'_> {
    fn error(&mut self, message: &str) {
        eprintln!("{MSG_NAME}: {message}");
        self.status.errors += 1;
    }

    fn warn(&mut self, message: &str) {
        if !self.opts.quiet {
            eprintln!("{MSG_NAME}: {message}");
        }
        self.status.warnings += 1;
    }

    fn io_failure(&mut self, name: &str, error: &io::Error) {
        let text = format!("{name}: {}", io_error_text(error));
        self.error(&text);
    }

    fn decode_failure(&mut self, name: &str, error: &DecodeError) {
        let text = format!("{name}: {}", error.message());
        self.error(&text);
    }

    fn process_stdin(&mut self) {
        match self.opts.mode {
            Mode::Compress => self.compress_stdin(),
            Mode::Decompress => self.decompress_stdin(),
            Mode::List => self.list_stdin(),
            Mode::Test => self.test_stdin(),
        }
    }

    fn process_operand(&mut self, operand: &str) {
        if operand == "-" {
            self.process_stdin();
            return;
        }
        let Some((path, meta)) = self.resolve(operand) else {
            return;
        };
        if meta.is_dir() {
            if self.opts.recursive {
                self.walk(&path);
            } else {
                self.warn(&format!("{path} is a directory -- ignored"));
            }
        } else {
            self.process_file(&path, &meta);
        }
    }

    /// Finds the input for `operand`. For decompression and listing a missing `name`
    /// falls back to `name` plus the suffix, as GNU does.
    fn resolve(&mut self, operand: &str) -> Option<(String, Metadata)> {
        let error = match fs::metadata(operand) {
            Ok(meta) => return Some((operand.to_string(), meta)),
            Err(error) => error,
        };
        let reads_compressed = matches!(self.opts.mode, Mode::Decompress | Mode::List | Mode::Test);
        if reads_compressed && !files::has_suffix(operand, &self.opts.suffix) {
            let suffixed = format!("{operand}{}", self.opts.suffix);
            if let Ok(meta) = fs::metadata(&suffixed) {
                return Some((suffixed, meta));
            }
            self.io_failure(&suffixed, &error);
        } else {
            self.io_failure(operand, &error);
        }
        None
    }

    fn process_file(&mut self, path: &str, meta: &Metadata) {
        match self.opts.mode {
            Mode::Compress => self.compress_file(path, meta),
            Mode::Decompress => self.decompress_file(path, meta),
            Mode::List => self.list_file(path, meta),
            Mode::Test => self.test_file(path),
        }
    }

    /// Recurses into a directory with `-r`, skipping symbolic links. Entries are visited in
    /// name order so the output is deterministic.
    fn walk(&mut self, dir: &str) {
        let mut entries = match fs::read_dir(dir) {
            Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
            Err(error) => return self.io_failure(dir, &error),
        };
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let child = Path::new(dir).join(entry.file_name());
            let shown = display_path(&child);
            let Ok(link_meta) = fs::symlink_metadata(&child) else {
                continue;
            };
            if link_meta.file_type().is_symlink() {
                continue;
            }
            if link_meta.is_dir() {
                self.walk(&shown);
            } else {
                match fs::metadata(&child) {
                    Ok(meta) => self.process_file(&shown, &meta),
                    Err(error) => self.io_failure(&shown, &error),
                }
            }
        }
    }

    fn compress_stdin(&mut self) {
        if !self.opts.force && files::stdout_is_terminal() {
            return self.refuse_terminal();
        }
        let mut input = io::stdin().lock();
        let level = self.opts.level;
        let written = codec::encode(&mut input, &mut self.out, None, 0, level);
        if let Err(error) = written {
            self.io_failure("stdout", &error);
        }
    }

    fn refuse_terminal(&mut self) {
        self.error(
            "compressed data not written to a terminal. Use -f to force compression.\n\
             For help, type: 'gzip -h'",
        );
    }

    fn compress_file(&mut self, path: &str, meta: &Metadata) {
        let suffix = self.opts.suffix.clone();
        if !self.opts.to_stdout && files::has_suffix(path, &suffix) {
            eprintln!("{MSG_NAME}: {path} already has {suffix} suffix -- unchanged");
            return;
        }
        let out_name = format!("{path}{suffix}");
        if !self.opts.to_stdout && files::exists(&out_name) && !self.opts.force {
            let overwrite = files::stdin_is_terminal() && files::confirm_overwrite(&out_name);
            if !overwrite {
                return self.warn(&format!("{out_name} already exists;\tnot overwritten"));
            }
        }
        let mut input = match fs::File::open(path) {
            Ok(file) => BufReader::new(file),
            Err(error) => return self.io_failure(path, &error),
        };
        let (name, mtime) = if self.opts.no_name {
            (None, 0)
        } else {
            (Some(files::base_name_bytes(path)), files::mtime_secs(meta))
        };
        let level = self.opts.level;
        if self.opts.to_stdout {
            if !self.opts.force && files::stdout_is_terminal() {
                return self.refuse_terminal();
            }
            if let Err(error) =
                codec::encode(&mut input, &mut self.out, name.as_deref(), mtime, level)
            {
                self.io_failure(path, &error);
            }
            return;
        }
        self.write_compressed(path, meta, &out_name, &mut input, name, mtime);
    }

    fn write_compressed<R: Read>(
        &mut self,
        path: &str,
        meta: &Metadata,
        out_name: &str,
        input: &mut R,
        name: Option<Vec<u8>>,
        mtime: u32,
    ) {
        let file = match files::create_output(out_name, self.opts.force) {
            Ok(file) => file,
            Err(error) => return self.io_failure(out_name, &error),
        };
        let level = self.opts.level;
        let result = codec::encode(input, BufWriter::new(file), name.as_deref(), mtime, level)
            .and_then(|writer| writer.into_inner().map_err(|e| e.into_error()));
        let written = match result {
            Ok(file) => file,
            Err(error) => {
                let _ = fs::remove_file(out_name);
                return self.io_failure(path, &error);
            }
        };
        drop(written);
        if let Err(error) = files::copy_metadata(meta, out_name, None) {
            self.io_failure(out_name, &error);
        }
        let in_len = meta.len();
        let out_len = fs::metadata(out_name).map(|m| m.len()).unwrap_or(0);
        self.finish_input(path, out_name, in_len, out_len);
    }

    /// Removes the input unless `-k`, and prints the `-v` line.
    fn finish_input(&mut self, path: &str, out_name: &str, in_len: u64, out_len: u64) {
        let pct = saving_percent(in_len, out_len);
        if self.opts.keep {
            if self.opts.verbose {
                eprintln!("{path}:\t {pct:5.1}% -- created {out_name}");
            }
            return;
        }
        if let Err(error) = files::remove_input(path) {
            return self.io_failure(path, &error);
        }
        if self.opts.verbose {
            eprintln!("{path}:\t {pct:5.1}% -- replaced with {out_name}");
        }
    }

    fn decompress_stdin(&mut self) {
        let stdin = io::stdin();
        let mut input = BufReader::new(stdin.lock());
        match codec::decode_stream(&mut input, &mut self.out) {
            Ok(summary) => self.note_trailing(&summary, "stdin"),
            Err(error) => self.decode_failure("stdin", &error),
        }
    }

    fn note_trailing(&mut self, summary: &Summary, name: &str) {
        if summary.trailing_garbage {
            self.warn(&format!(
                "{name}: decompression OK, trailing garbage ignored"
            ));
        }
    }

    fn decompress_file(&mut self, path: &str, meta: &Metadata) {
        let suffix = self.opts.suffix.clone();
        let default_out = if self.opts.to_stdout {
            files::strip_suffix(path, &suffix).unwrap_or_else(|| path.to_string())
        } else {
            match files::strip_suffix(path, &suffix) {
                Some(name) => name,
                None => return self.warn(&format!("{path}: unknown suffix -- ignored")),
            }
        };
        let mut input = match fs::File::open(path) {
            Ok(file) => BufReader::new(file),
            Err(error) => return self.io_failure(path, &error),
        };
        if self.opts.to_stdout {
            return match codec::decode_stream(&mut input, &mut self.out) {
                Ok(summary) => self.note_trailing(&summary, path),
                Err(error) => self.decode_failure(path, &error),
            };
        }
        let out_name = if self.opts.restore_name {
            self.restored_name(path, &default_out)
        } else {
            default_out
        };
        if files::exists(&out_name) && !self.opts.force {
            let overwrite = files::stdin_is_terminal() && files::confirm_overwrite(&out_name);
            if !overwrite {
                return self.warn(&format!("{out_name} already exists;\tnot overwritten"));
            }
        }
        let file = match files::create_output(&out_name, self.opts.force) {
            Ok(file) => file,
            Err(error) => return self.io_failure(&out_name, &error),
        };
        let mut writer = BufWriter::new(file);
        let result = codec::decode_stream(&mut input, &mut writer);
        drop(writer);
        match result {
            Ok(summary) => {
                let mtime = Some(summary.first.mtime);
                if let Err(error) = files::copy_metadata(meta, &out_name, mtime) {
                    self.io_failure(&out_name, &error);
                }
                self.note_trailing(&summary, path);
                let out_len = fs::metadata(&out_name).map(|m| m.len()).unwrap_or(0);
                self.finish_input(path, &out_name, meta.len(), out_len);
            }
            Err(error) => {
                let _ = fs::remove_file(&out_name);
                self.decode_failure(path, &error);
            }
        }
    }

    /// With `-N`, the name stored in the header replaces the default output name.
    fn restored_name(&self, path: &str, default_out: &str) -> String {
        let Ok(file) = fs::File::open(path) else {
            return default_out.to_string();
        };
        let Ok(header) = codec::read_header(&mut BufReader::new(file)) else {
            return default_out.to_string();
        };
        match header.name {
            Some(name) if !name.is_empty() => {
                let dir = match default_out.rfind('/') {
                    Some(index) => &default_out[..=index],
                    None => "",
                };
                format!("{dir}{}", String::from_utf8_lossy(&name))
            }
            _ => default_out.to_string(),
        }
    }

    fn test_stdin(&mut self) {
        let stdin = io::stdin();
        let mut input = BufReader::new(stdin.lock());
        match codec::decode_stream(&mut input, &mut io::sink()) {
            Ok(summary) => self.note_trailing(&summary, "stdin"),
            Err(error) => self.decode_failure("stdin", &error),
        }
    }

    fn test_file(&mut self, path: &str) {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) => return self.io_failure(path, &error),
        };
        match codec::decode_stream(&mut BufReader::new(file), &mut io::sink()) {
            Ok(summary) => {
                self.note_trailing(&summary, path);
                if self.opts.verbose {
                    eprintln!("{path}:\t OK");
                }
            }
            Err(error) => self.decode_failure(path, &error),
        }
    }

    fn list_stdin(&mut self) {
        let mut data = Vec::new();
        if let Err(error) = io::stdin().lock().read_to_end(&mut data) {
            return self.io_failure("stdin", &error);
        }
        let compressed = data.len() as u64;
        let mut cursor = io::Cursor::new(data);
        match codec::decode_stream(&mut cursor, &mut io::sink()) {
            Ok(summary) => self.record_listing(compressed, None, &summary, "stdout".to_string()),
            Err(error) => self.list_failure("stdin", &error),
        }
    }

    fn list_file(&mut self, path: &str, meta: &Metadata) {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) => return self.io_failure(path, &error),
        };
        let name = files::strip_suffix(path, &self.opts.suffix).unwrap_or_else(|| path.to_string());
        match codec::decode_stream(&mut BufReader::new(file), &mut io::sink()) {
            Ok(summary) => self.record_listing(
                meta.len(),
                meta.modified().ok(),
                &summary,
                display_path(Path::new(&name)),
            ),
            Err(error) => self.list_failure(path, &error),
        }
    }

    fn list_failure(&mut self, name: &str, error: &DecodeError) {
        self.status.last_overhead = 0;
        self.decode_failure(name, error);
    }

    fn record_listing(
        &mut self,
        compressed: u64,
        modified: Option<std::time::SystemTime>,
        summary: &Summary,
        name: String,
    ) {
        let overhead = if summary.members == 1 {
            summary.first.len + codec::TRAILER_LEN
        } else {
            0
        };
        if !self.header_printed {
            let _ = writeln!(self.out, "{}", list::header(self.opts.verbose));
            self.header_printed = true;
        }
        let row = list::Row {
            compressed,
            uncompressed: summary.last.out_len,
            crc: summary.last.crc,
            modified,
            overhead,
            name,
        };
        let _ = writeln!(self.out, "{}", list::format_row(self.opts.verbose, &row));
        self.status.last_overhead = overhead;
        self.totals.compressed += compressed;
        self.totals.uncompressed += summary.last.out_len;
        self.totals.listed = true;
    }

    fn print_totals(&mut self) {
        let line = list::format_totals(
            self.opts.verbose,
            self.totals.compressed,
            self.totals.uncompressed,
            self.status.last_overhead,
        );
        let _ = writeln!(self.out, "{line}");
    }
}

/// Percentage by which the output is smaller than the input, for `-v` lines.
fn saving_percent(in_len: u64, out_len: u64) -> f64 {
    if in_len == 0 {
        return 0.0;
    }
    (1.0 - out_len as f64 / in_len as f64) * 100.0
}
