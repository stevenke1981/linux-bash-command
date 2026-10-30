mod files;
mod parse;
mod pattern;

use crate::common::args::prepare_legacy;
use crate::common::error::EXIT_TROUBLE;
use files::{io_message, is_device_type, offset_width, stdin_size};
use parse::{parse, LONG_OPTIONS, VALUE_LONGS, VALUE_SHORTS};
use pattern::{build_regex, spans};
use regex::bytes::Regex as ByteRegex;
use regex::Regex as NameRegex;
use std::collections::{HashSet, VecDeque};
use std::ffi::OsStr;
use std::fs::{self, DirEntry, File};
use std::io::{self, BufRead, BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};

const USAGE: &str = "Usage: grep [OPTIONS] PATTERN [FILE...]\n\nSearch lines in files or standard input (GNU grep compatible subset).\n\nPattern syntax:\n  -G, --basic-regexp        PATTERN is a basic regular expression (default)\n  -E, --extended-regexp     PATTERN is an extended regular expression\n  -F, --fixed-strings       PATTERN is a literal string\n  -P, --perl-regexp         PATTERN is a Perl-style regular expression (no lookaround)\n  -e, --regexp=PATTERN      Use PATTERN (repeatable; newlines separate patterns)\n  -f, --file=FILE           Read patterns from FILE, one per line\n  -i, --ignore-case         Ignore case distinctions (also -y)\n      --no-ignore-case      Do not ignore case (overrides -i)\n  -w, --word-regexp         Match whole words only\n  -x, --line-regexp         Match whole lines only\n  -v, --invert-match        Select non-matching lines\n\nOutput control:\n  -c, --count               Print a count of selected lines per file\n  -l, --files-with-matches  Print only the names of files with matches\n  -L, --files-without-match Print only the names of files without matches\n  -o, --only-matching       Print only the matched parts, one per line\n  -q, --quiet, --silent     Suppress output; exit 0 on the first match\n  -s, --no-messages         Suppress error messages about files\n  -m, --max-count=NUM       Stop after NUM selected lines per file\n  -n, --line-number         Prefix lines with their line number\n  -b, --byte-offset         Prefix lines with their byte offset (match offset with -o)\n  -T, --initial-tab         Align prefixes and put a tab before the line content\n  -Z, --null                Output a NUL byte after each file name\n  -z, --null-data           Treat input and output lines as NUL-terminated\n  -H, --with-filename       Always print the file name\n  -h, --no-filename         Never print the file name\n      --label=LABEL         Use LABEL as the name of standard input\n  -A, --after-context=NUM   Print NUM lines of trailing context\n  -B, --before-context=NUM  Print NUM lines of leading context\n  -C, --context=NUM        Print NUM lines of context (also -NUM)\n      --line-buffered      Flush output after each line\n      --color[=WHEN]        Accepted for compatibility; output is never colored\n\nFile selection:\n  -r, --recursive           Recurse into directories (symlinks are not followed)\n  -R, --dereference-recursive  Recurse into directories, following symlinks\n  -d, --directories=ACTION  read (default), skip or recurse into directories\n  -D, --devices=ACTION      read or skip devices, FIFOs and sockets\n      --include=GLOB        Search only files whose base name matches GLOB\n      --exclude=GLOB        Skip files whose base name matches GLOB\n      --exclude-from=FILE   Skip files whose base name matches a GLOB listed in FILE\n      --exclude-dir=GLOB    Skip directories whose base name matches GLOB\n  -a, --text               Treat binary files as text\n  -I                        Ignore binary files\n      --binary-files=TYPE   TYPE is binary, text or without-match\n  -U, --binary              Accepted; no effect on this platform\n      --                    End option parsing\n      --help                Show this help\n\nGlobs support *, ?, [...] and {a,b}. Binary matches print 'Binary file NAME matches'.\nExit status: 0 when any line is selected (with -L, when a file is listed), 1 when none are,\n2 on error. With -q, a match gives 0 even if an error occurred.\n";

const BINARY_PROBE_BYTES: u64 = 8192;
const STDIN_LABEL: &str = "(standard input)";
const STDOUT_LABEL: &str = "(standard output)";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Syntax {
    #[default]
    Basic,
    Extended,
    Perl,
    Fixed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Recurse {
    #[default]
    Off,
    /// `-r`: symbolic links met during recursion are skipped.
    Command,
    /// `-R`: symbolic links are followed.
    All,
}

/// `-D ACTION`. By default devices named on the command line are read, and devices met
/// while recursing are skipped; `-D read` reads them everywhere and `-D skip` skips them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Devices {
    #[default]
    Default,
    Read,
    Skip,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BinaryMode {
    #[default]
    Match,
    Text,
    Skip,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Listing {
    #[default]
    None,
    Matching,
    NonMatching,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Lines,
    Count,
    Quiet,
    FilesWith,
    FilesWithout,
}

struct Filter {
    include: bool,
    glob: NameRegex,
}

#[derive(Default)]
struct Options {
    patterns: Vec<String>,
    files: Vec<String>,
    syntax: Syntax,
    ignore_case: bool,
    invert: bool,
    word: bool,
    line: bool,
    only: bool,
    quiet: bool,
    no_messages: bool,
    count: bool,
    listing: Listing,
    line_number: bool,
    line_buffered: bool,
    with_filename: Option<bool>,
    max_count: Option<u64>,
    before: usize,
    after: usize,
    recurse: Recurse,
    /// `-d skip`: operand directories are passed over without a message.
    skip_dirs: bool,
    devices: Devices,
    binary: BinaryMode,
    filters: Vec<Filter>,
    exclude_dirs: Vec<NameRegex>,
    /// `-z`: records end in NUL instead of newline.
    zero: bool,
    /// `-Z`: a NUL follows file names instead of the usual separator.
    null: bool,
    /// `-b`: prefix each output line (or match with `-o`) with its byte offset.
    byte_offset: bool,
    /// `-T`: align the prefixes and put a tab before the line content.
    initial_tab: bool,
    /// `--label`: name shown for standard input.
    label: Option<String>,
}

impl Options {
    fn mode(&self) -> Mode {
        if self.quiet {
            Mode::Quiet
        } else if self.listing == Listing::Matching {
            Mode::FilesWith
        } else if self.listing == Listing::NonMatching {
            Mode::FilesWithout
        } else if self.count {
            Mode::Count
        } else {
            Mode::Lines
        }
    }

    /// The record terminator: NUL with `-z`, newline otherwise.
    fn eol(&self) -> u8 {
        if self.zero {
            0
        } else {
            b'\n'
        }
    }
}

pub fn run(args: &[String]) -> i32 {
    let expanded = match prepare_legacy(
        "grep",
        args,
        LONG_OPTIONS,
        VALUE_LONGS,
        VALUE_SHORTS,
        EXIT_TROUBLE,
    ) {
        Ok(expanded) => expanded,
        Err(code) => return code,
    };
    let Some(options) = (match parse(&expanded) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("grep: {error}");
            return 2;
        }
    }) else {
        return 0;
    };
    let regex = match build_regex(&options) {
        Ok(regex) => regex,
        Err(error) => {
            eprintln!("grep: {error}");
            return 2;
        }
    };
    let names = show_names(&options);
    let stdout = io::stdout();
    let mut grep = Grep::new(&options, &regex, BufWriter::new(stdout.lock()), names);
    grep.search_all();
    grep.exit_code()
}

// ---------------------------------------------------------------- searching

/// Where a record starts: its 1-based number and its byte offset within the input.
#[derive(Clone, Copy)]
struct Pos {
    line: u64,
    offset: u64,
}

#[derive(Default)]
struct ScanState {
    lineno: u64,
    /// Byte offset of the next record.
    offset: u64,
    selected: u64,
    last_printed: Option<u64>,
    before: VecDeque<(Pos, Vec<u8>)>,
    after_left: usize,
    limit_reached: bool,
}

enum EntryKind {
    Directory,
    File,
    Device,
}

struct Grep<'a, W: Write> {
    opts: &'a Options,
    regex: &'a ByteRegex,
    mode: Mode,
    show_names: bool,
    out: W,
    group_printed: bool,
    any_selected: bool,
    any_listed: bool,
    had_error: bool,
    halted: bool,
    visited: HashSet<PathBuf>,
    /// Field width for `-T` numbers in the input being searched (0 when not aligning).
    width: usize,
}

impl<'a, W: Write> Grep<'a, W> {
    fn new(opts: &'a Options, regex: &'a ByteRegex, out: W, show_names: bool) -> Self {
        Self {
            opts,
            regex,
            mode: opts.mode(),
            show_names,
            out,
            group_printed: false,
            any_selected: false,
            any_listed: false,
            had_error: false,
            halted: false,
            visited: HashSet::new(),
            width: 0,
        }
    }

    fn search_all(&mut self) {
        let opts = self.opts;
        if opts.files.is_empty() {
            if opts.recurse == Recurse::Off {
                self.search_stdin();
            } else {
                // Without operands, names are shown relative to "." (no "./" prefix).
                self.walk(Path::new("."), Path::new(""));
            }
        } else {
            for operand in &opts.files {
                if self.halted {
                    break;
                }
                self.search_operand(operand);
            }
        }
        if let Err(error) = self.out.flush() {
            self.report(STDOUT_LABEL, &error);
        }
    }

    fn exit_code(&self) -> i32 {
        if self.opts.quiet && self.any_selected {
            return 0;
        }
        if self.had_error {
            return 2;
        }
        let success = if self.opts.listing == Listing::NonMatching {
            self.any_listed
        } else {
            self.any_selected
        };
        if success {
            0
        } else {
            1
        }
    }

    fn search_operand(&mut self, operand: &str) {
        if operand == "-" {
            self.search_stdin();
            return;
        }
        let path = Path::new(operand);
        let label = crate::display_path(path);
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                if self.opts.recurse != Recurse::Off {
                    if !self.dir_excluded(path) {
                        self.walk(path, path);
                    }
                } else if !self.opts.skip_dirs {
                    self.report_message(&label, "Is a directory");
                }
            }
            Ok(meta) if is_device_type(meta.file_type()) && self.opts.devices == Devices::Skip => {}
            Ok(_) => {
                if self.operand_allowed(path) {
                    self.search_file(path, &label);
                }
            }
            Err(error) => self.report(&label, &error),
        }
    }

    fn walk(&mut self, fs_dir: &Path, shown: &Path) {
        let label = crate::display_path(shown);
        if self.opts.recurse == Recurse::All && !self.visited.insert(canonical(fs_dir)) {
            return;
        }
        let mut entries = Vec::new();
        match fs::read_dir(fs_dir) {
            Ok(read) => {
                for item in read {
                    match item {
                        Ok(entry) => entries.push(entry),
                        Err(error) => self.report(&label, &error),
                    }
                }
            }
            Err(error) => {
                self.report(&label, &error);
                return;
            }
        }
        entries.sort_by_key(DirEntry::file_name);
        for entry in entries {
            if self.halted {
                return;
            }
            let name = entry.file_name();
            let child_shown = shown.join(&name);
            let child_label = crate::display_path(&child_shown);
            match self.entry_kind(&entry, &child_label) {
                Some(EntryKind::Directory) => {
                    if !glob_hit(&self.opts.exclude_dirs, &name) {
                        self.walk(&entry.path(), &child_shown);
                    }
                }
                Some(EntryKind::File) if self.name_allowed(&name) => {
                    self.search_file(&entry.path(), &child_label);
                }
                // Devices are skipped while recursing unless `-D read` asks for them.
                Some(EntryKind::Device)
                    if self.opts.devices == Devices::Read && self.name_allowed(&name) =>
                {
                    self.search_file(&entry.path(), &child_label);
                }
                Some(EntryKind::File | EntryKind::Device) | None => {}
            }
        }
    }

    fn entry_kind(&mut self, entry: &DirEntry, label: &str) -> Option<EntryKind> {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                self.report(label, &error);
                return None;
            }
        };
        if !file_type.is_symlink() {
            return classify(file_type);
        }
        if self.opts.recurse != Recurse::All {
            return None;
        }
        match fs::metadata(entry.path()) {
            Ok(meta) => classify(meta.file_type()),
            Err(error) => {
                self.report(label, &error);
                None
            }
        }
    }

    fn dir_excluded(&self, path: &Path) -> bool {
        match path.file_name() {
            Some(name) => glob_hit(&self.opts.exclude_dirs, name),
            None => false,
        }
    }

    fn operand_allowed(&self, path: &Path) -> bool {
        match path.file_name() {
            Some(name) => self.name_allowed(name),
            None => true,
        }
    }

    /// `--include`/`--exclude`: the last matching option wins; when none
    /// matches, a file is included unless the first option is `--include`.
    fn name_allowed(&self, name: &OsStr) -> bool {
        let text = name.to_string_lossy();
        let mut decision = None;
        for filter in &self.opts.filters {
            if filter.glob.is_match(&text) {
                decision = Some(filter.include);
            }
        }
        decision.unwrap_or(!matches!(
            self.opts.filters.first(),
            Some(filter) if filter.include
        ))
    }

    fn search_stdin(&mut self) {
        let stdin = io::stdin();
        let label = self
            .opts
            .label
            .as_deref()
            .unwrap_or(STDIN_LABEL)
            .to_string();
        let result = self.search_reader(stdin.lock(), &label, stdin_size());
        if let Err(error) = result {
            self.report(&label, &error);
        }
    }

    fn search_file(&mut self, path: &Path, label: &str) {
        let result = match File::open(path) {
            Ok(file) => {
                let size = file
                    .metadata()
                    .ok()
                    .filter(fs::Metadata::is_file)
                    .map(|m| m.len());
                self.search_reader(file, label, size)
            }
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            self.report(label, &error);
        }
    }

    /// `size` is the input's length when known; it sets the `-T` field width.
    fn search_reader<R: Read>(
        &mut self,
        mut source: R,
        label: &str,
        size: Option<u64>,
    ) -> io::Result<()> {
        let mut probe = Vec::new();
        (&mut source)
            .take(BINARY_PROBE_BYTES)
            .read_to_end(&mut probe)?;
        // With -z, NUL separates records, so it no longer marks a file as binary.
        let binary = !self.opts.zero && probe.contains(&0);
        if binary && self.opts.binary == BinaryMode::Skip {
            return self.finish(label, 0);
        }
        let treat_binary = binary && self.opts.binary != BinaryMode::Text;
        self.width = if self.opts.initial_tab {
            offset_width(size)
        } else {
            0
        };
        let reader = BufReader::new(Cursor::new(probe).chain(source));
        let selected = self.scan(reader, label, treat_binary)?;
        self.finish(label, selected)
    }

    fn scan<R: BufRead>(&mut self, mut reader: R, label: &str, binary: bool) -> io::Result<u64> {
        let mut state = ScanState::default();
        if self.opts.max_count == Some(0) {
            return Ok(0);
        }
        let eol = self.opts.eol();
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(eol, &mut line)?;
            if read == 0 {
                break;
            }
            let pos = Pos {
                line: state.lineno + 1,
                offset: state.offset,
            };
            state.offset += read as u64;
            if line.last() == Some(&eol) {
                line.pop();
            }
            state.lineno += 1;
            if state.limit_reached {
                // After -m is reached, trailing context stops at the next selected line.
                if self.is_selected(&line) || state.after_left == 0 {
                    break;
                }
                state.after_left -= 1;
                self.emit(label, pos, b'-', &line, &mut state.last_printed)?;
                continue;
            }
            if !self.is_selected(&line) {
                self.keep_context(label, &mut state, &line, pos)?;
                continue;
            }
            state.selected += 1;
            if !self.on_selected(label, &line, pos, binary, &mut state)? {
                break;
            }
            state.limit_reached = self.opts.max_count == Some(state.selected);
            if state.limit_reached && state.after_left == 0 {
                break;
            }
        }
        Ok(state.selected)
    }

    fn is_selected(&self, line: &[u8]) -> bool {
        self.regex.is_match(line) != self.opts.invert
    }

    fn on_selected(
        &mut self,
        label: &str,
        line: &[u8],
        pos: Pos,
        binary: bool,
        state: &mut ScanState,
    ) -> io::Result<bool> {
        match self.mode {
            Mode::Count => Ok(true),
            Mode::Lines if binary => {
                writeln!(self.out, "Binary file {label} matches")?;
                Ok(false)
            }
            Mode::Lines => {
                self.print_match(label, line, pos, state)?;
                Ok(true)
            }
            Mode::Quiet | Mode::FilesWith | Mode::FilesWithout => Ok(false),
        }
    }

    fn keep_context(
        &mut self,
        label: &str,
        state: &mut ScanState,
        line: &[u8],
        pos: Pos,
    ) -> io::Result<()> {
        if self.mode != Mode::Lines || !self.context_on() {
            return Ok(());
        }
        if state.after_left > 0 {
            state.after_left -= 1;
            return self.emit(label, pos, b'-', line, &mut state.last_printed);
        }
        if self.opts.before > 0 {
            if state.before.len() == self.opts.before {
                state.before.pop_front();
            }
            state.before.push_back((pos, line.to_vec()));
        }
        Ok(())
    }

    fn print_match(
        &mut self,
        label: &str,
        line: &[u8],
        pos: Pos,
        state: &mut ScanState,
    ) -> io::Result<()> {
        if self.opts.only {
            let eol = self.opts.eol();
            for (start, end) in spans(self.regex, line) {
                let at = Pos {
                    line: pos.line,
                    offset: pos.offset + start as u64,
                };
                self.prefix(label, at, b':')?;
                self.out.write_all(&line[start..end])?;
                self.end_line(eol)?;
            }
            return Ok(());
        }
        let pending: Vec<(Pos, Vec<u8>)> = state.before.drain(..).collect();
        for (before, text) in pending {
            self.emit(label, before, b'-', &text, &mut state.last_printed)?;
        }
        self.emit(label, pos, b':', line, &mut state.last_printed)?;
        state.after_left = self.opts.after;
        Ok(())
    }

    fn context_on(&self) -> bool {
        !self.opts.only && (self.opts.before > 0 || self.opts.after > 0)
    }

    fn emit(
        &mut self,
        label: &str,
        pos: Pos,
        sep: u8,
        text: &[u8],
        last: &mut Option<u64>,
    ) -> io::Result<()> {
        if self.context_on() && self.group_printed {
            let gap = match last {
                None => true,
                Some(previous) => pos.line > *previous + 1,
            };
            if gap {
                self.out.write_all(b"--\n")?;
            }
        }
        self.prefix(label, pos, sep)?;
        self.out.write_all(text)?;
        let eol = self.opts.eol();
        self.end_line(eol)?;
        *last = Some(pos.line);
        self.group_printed = true;
        Ok(())
    }

    /// Writes the `file:line:offset:` prefix. Each field is followed by `sep`, except that
    /// `-Z` puts a NUL after the file name. With `-T`, numbers are padded to the field
    /// width and a tab separates the prefix from the line.
    fn prefix(&mut self, label: &str, pos: Pos, sep: u8) -> io::Result<()> {
        let mut wrote = false;
        if self.show_names {
            write!(self.out, "{label}")?;
            self.out
                .write_all(&[if self.opts.null { 0 } else { sep }])?;
            wrote = true;
        }
        if self.opts.line_number {
            self.number(pos.line)?;
            self.out.write_all(&[sep])?;
            wrote = true;
        }
        if self.opts.byte_offset {
            self.number(pos.offset)?;
            self.out.write_all(&[sep])?;
            wrote = true;
        }
        if self.opts.initial_tab && wrote {
            self.out.write_all(b"\t")?;
        }
        Ok(())
    }

    fn number(&mut self, value: u64) -> io::Result<()> {
        if self.width == 0 {
            write!(self.out, "{value}")
        } else {
            write!(self.out, "{value:>width$}", width = self.width)
        }
    }

    /// Ends an output record with `eol` (NUL with `-z`), flushing per line if asked.
    fn end_line(&mut self, eol: u8) -> io::Result<()> {
        self.out.write_all(&[eol])?;
        if self.opts.line_buffered {
            self.out.flush()?;
        }
        Ok(())
    }

    /// A file name ends a `-l`/`-L`/`-c` line: NUL with `-Z`, otherwise `terminator`.
    fn name_end(&mut self, terminator: u8) -> io::Result<()> {
        let byte = if self.opts.null { 0 } else { terminator };
        self.out.write_all(&[byte])
    }

    fn finish(&mut self, label: &str, selected: u64) -> io::Result<()> {
        self.any_selected |= selected > 0;
        match self.mode {
            Mode::Count => {
                if self.show_names {
                    write!(self.out, "{label}")?;
                    self.name_end(b':')?;
                }
                writeln!(self.out, "{selected}")?;
            }
            Mode::FilesWith if selected > 0 => {
                write!(self.out, "{label}")?;
                self.name_end(b'\n')?;
            }
            Mode::FilesWithout if selected == 0 => {
                write!(self.out, "{label}")?;
                self.name_end(b'\n')?;
                self.any_listed = true;
            }
            Mode::Quiet if selected > 0 => self.halted = true,
            _ => {}
        }
        Ok(())
    }

    fn report(&mut self, label: &str, error: &io::Error) {
        if error.kind() == io::ErrorKind::BrokenPipe {
            // The reader went away; stop quietly.
            self.had_error = true;
            self.halted = true;
            return;
        }
        self.report_message(label, &io_message(error));
    }

    fn report_message(&mut self, label: &str, message: &str) {
        self.had_error = true;
        if !self.opts.no_messages {
            eprintln!("grep: {label}: {message}");
        }
    }
}

fn classify(file_type: fs::FileType) -> Option<EntryKind> {
    if file_type.is_dir() {
        Some(EntryKind::Directory)
    } else if file_type.is_file() {
        Some(EntryKind::File)
    } else if is_device_type(file_type) {
        Some(EntryKind::Device)
    } else {
        None
    }
}

fn glob_hit(globs: &[NameRegex], name: &OsStr) -> bool {
    let text = name.to_string_lossy();
    globs.iter().any(|glob| glob.is_match(&text))
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn show_names(options: &Options) -> bool {
    options.with_filename.unwrap_or(
        options.files.len() > 1
            || (options.recurse != Recurse::Off
                && (options.files.is_empty()
                    || options.files.iter().any(|file| Path::new(file).is_dir()))),
    )
}

#[cfg(test)]
mod tests;
