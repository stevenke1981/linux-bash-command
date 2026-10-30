//! `sort`: writes sorted lines of text. Byte comparison (C locale), GNU key syntax, the
//! ordering modes (`-n -g -h -M -V -R`), external merging beyond the buffer size, and the
//! `-c`, `-C` and `-m` modes. Specification: docs/spec/coreutils-alignment/03-commands-core.md.

mod compare;
mod extsort;
mod keys;
mod numeric;
mod options;
mod version;

use std::collections::hash_map::RandomState;
use std::fs::{self, File};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, BufRead, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::{finish_output, is_broken_pipe, open_input, read_record, stdout_writer};

use compare::Comparer;
use extsort::{merge_sources, Sorter};
use keys::{Key, KeyDef};
use options::{Action, Check, Config};

const CMD: &str = "sort";
const SEED_BYTES: usize = 16;

/// Entry point for `sort` and the `agentcmd sort` multi-call form.
pub fn run(args: &[String]) -> i32 {
    match options::parse(args) {
        Action::Help => write_help(),
        Action::Version => print_version(CMD),
        Action::Exit(code) => code,
        Action::Sort(config) => execute(*config),
    }
}

fn write_help() -> i32 {
    let mut out = stdout_writer();
    if out.write_all(options::HELP.as_bytes()).is_err() {
        return EXIT_OK;
    }
    finish_output(CMD, &mut out)
}

fn execute(config: Config) -> i32 {
    let comparer = match build_comparer(&config) {
        Ok(comparer) => comparer,
        Err(code) => return code,
    };
    let delim = if config.zero { 0 } else { b'\n' };
    let names = match input_names(&config) {
        Ok(names) => names,
        Err(code) => return code,
    };
    match config.check {
        Check::None if config.merge => merge_mode(&config, &comparer, delim, &names),
        Check::None => sort_mode(&config, &comparer, delim, &names),
        check => match names.first() {
            Some(name) => check_mode(&comparer, delim, name, check == Check::Quiet),
            None => EXIT_OK,
        },
    }
}

/// Builds the comparison plan: explicit keys with inherited modes, or one whole-line key
/// carrying the global modes. Reads the random seed when `-R` needs one.
fn build_comparer(config: &Config) -> Result<Comparer, i32> {
    let keys: Vec<Key> = if config.keys.is_empty() {
        vec![Key {
            def: KeyDef {
                start_field: 0,
                start_char: 0,
                end: None,
                modes: config.global,
            },
            modes: config.global,
        }]
    } else {
        keys::resolve_keys(&config.keys, config.global)
    };
    let seed = match &config.random_source {
        Some(path) => read_seed(path)?,
        None => default_seed(),
    };
    Ok(Comparer {
        keys,
        tab: config.tab,
        global_reverse: config.global.reverse,
        last_resort: !(config.unique || config.stable),
        unique: config.unique,
        seed,
    })
}

fn read_seed(path: &str) -> Result<Vec<u8>, i32> {
    let mut seed = vec![0u8; SEED_BYTES];
    let read = File::open(path).and_then(|mut file| {
        let mut filled = 0;
        while filled < SEED_BYTES {
            let n = file.read(&mut seed[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        Ok(filled)
    });
    match read {
        Ok(filled) => {
            seed.truncate(filled);
            Ok(seed)
        }
        Err(error) => {
            eprintln!("{CMD}: open failed: {path}: {}", io_error_text(&error));
            Err(EXIT_TROUBLE)
        }
    }
}

/// A seed from the process's randomly keyed hasher, used when no `--random-source` is given.
fn default_seed() -> Vec<u8> {
    let state = RandomState::new();
    let mut seed = Vec::with_capacity(SEED_BYTES);
    for salt in 0..2u64 {
        let mut hasher = state.build_hasher();
        hasher.write_u64(salt);
        seed.extend_from_slice(&hasher.finish().to_le_bytes());
    }
    seed
}

/// The operands to read, expanded from `--files0-from` when given. `-` means stdin.
fn input_names(config: &Config) -> Result<Vec<String>, i32> {
    if let Some(source) = &config.files0_from {
        return read_files0(source);
    }
    if config.files.is_empty() {
        return Ok(vec!["-".to_owned()]);
    }
    Ok(config.files.clone())
}

fn read_files0(source: &str) -> Result<Vec<String>, i32> {
    let mut reader = match open_input(source) {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!(
                "{CMD}: cannot open '{source}' for reading: {}",
                io_error_text(&error)
            );
            return Err(EXIT_TROUBLE);
        }
    };
    let mut names = Vec::new();
    let mut buf = Vec::new();
    let mut number = 0;
    loop {
        match read_record(reader.as_mut(), 0, &mut buf) {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => {
                eprintln!("{CMD}: read failed: {source}: {}", io_error_text(&error));
                return Err(EXIT_TROUBLE);
            }
        }
        number += 1;
        if buf.last() == Some(&0) {
            buf.pop();
        }
        if buf.is_empty() {
            eprintln!("{CMD}: {source}:{number}: invalid zero-length file name");
            return Err(EXIT_TROUBLE);
        }
        names.push(String::from_utf8_lossy(&buf).into_owned());
    }
    Ok(names)
}

/// Prints the message for an input that could not be opened.
fn cannot_read(name: &str, error: &io::Error) {
    eprintln!("{CMD}: cannot read: {name}: {}", io_error_text(error));
}

/// Default mode: read everything, sort, write.
fn sort_mode(config: &Config, comparer: &Comparer, delim: u8, names: &[String]) -> i32 {
    let limit = config.buffer;
    let temp_dir = temp_directory(config);
    let mut sorter = Sorter::new(comparer, delim, limit, config.batch_size, temp_dir);
    let mut status = EXIT_OK;
    for name in names {
        match open_input(name) {
            Ok(mut reader) => {
                if let Err(error) = sorter.add(reader.as_mut()) {
                    report_write_or_read(&error);
                    return EXIT_TROUBLE;
                }
            }
            Err(error) => {
                cannot_read(name, &error);
                status = EXIT_TROUBLE;
            }
        }
    }
    let mut out = match open_output(config.output.as_deref()) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let result = sorter.finish(out.as_mut()).and_then(|()| out.flush());
    finish_result(result, status)
}

/// `-m`: merge inputs that are already sorted.
fn merge_mode(config: &Config, comparer: &Comparer, delim: u8, names: &[String]) -> i32 {
    let mut readers: Vec<Box<dyn BufRead>> = Vec::with_capacity(names.len());
    let mut status = EXIT_OK;
    for name in names {
        match open_input(name) {
            Ok(reader) => readers.push(reader),
            Err(error) => {
                cannot_read(name, &error);
                status = EXIT_TROUBLE;
            }
        }
    }
    match &config.output {
        Some(path) => merge_to_file(config, path, readers, comparer, delim, status),
        None => {
            let mut out = stdout_writer();
            let result =
                merge_sources(readers, comparer, delim, &mut out).and_then(|()| out.flush());
            finish_result(result, status)
        }
    }
}

/// Merges into a temporary file first, so that `-o` may name one of the inputs.
fn merge_to_file(
    config: &Config,
    path: &str,
    readers: Vec<Box<dyn BufRead>>,
    comparer: &Comparer,
    delim: u8,
    status: i32,
) -> i32 {
    let mut staged = match staging_file(&temp_directory(config)) {
        Ok(staged) => staged,
        Err(code) => return code,
    };
    let merged = merge_sources(readers, comparer, delim, &mut staged.writer);
    if let Err(error) = merged.and_then(|()| staged.writer.flush()) {
        report_write_or_read(&error);
        return EXIT_TROUBLE;
    }
    let mut out = match open_output(Some(path)) {
        Ok(out) => out,
        Err(code) => return code,
    };
    let copied = File::open(&staged.path).and_then(|mut file| io::copy(&mut file, out.as_mut()));
    let result = copied.and_then(|_| out.flush()).map(|_| ());
    let _ = fs::remove_file(&staged.path);
    finish_result(result, status)
}

struct Staged {
    path: PathBuf,
    writer: BufWriter<File>,
}

fn staging_file(dir: &Path) -> Result<Staged, i32> {
    let path = dir.join(format!("agentcmd-sort-merge-{}", std::process::id()));
    match File::create(&path) {
        Ok(file) => Ok(Staged {
            path,
            writer: BufWriter::new(file),
        }),
        Err(error) => {
            eprintln!(
                "{CMD}: cannot create temporary file in '{}': {}",
                dir.display(),
                io_error_text(&error)
            );
            Err(EXIT_TROUBLE)
        }
    }
}

/// `-c` and `-C`: verify that the single input is sorted.
fn check_mode(comparer: &Comparer, delim: u8, name: &str, quiet: bool) -> i32 {
    let mut reader = match open_input(name) {
        Ok(reader) => reader,
        Err(error) => {
            cannot_read(name, &error);
            return EXIT_TROUBLE;
        }
    };
    match first_disorder(reader.as_mut(), comparer, delim) {
        Ok(None) => EXIT_OK,
        Ok(Some((line_number, line))) => {
            if !quiet {
                let mut message = format!("{CMD}: {name}:{line_number}: disorder: ").into_bytes();
                message.extend_from_slice(&line);
                message.push(b'\n');
                let _ = io::stderr().write_all(&message);
            }
            EXIT_FAIL
        }
        Err(error) => {
            report_write_or_read(&error);
            EXIT_TROUBLE
        }
    }
}

/// Finds the first record that sorts before its predecessor (or equals it under `-u`).
fn first_disorder(
    reader: &mut dyn BufRead,
    comparer: &Comparer,
    delim: u8,
) -> io::Result<Option<(u64, Vec<u8>)>> {
    let mut previous: Option<Vec<u8>> = None;
    let mut current = Vec::new();
    let mut number = 0u64;
    while read_record(reader, delim, &mut current)? {
        number += 1;
        if current.last() == Some(&delim) {
            current.pop();
        }
        if let Some(prev) = &previous {
            let order = comparer.compare(prev, &current);
            if order.is_gt() || (comparer.unique && order.is_eq()) {
                return Ok(Some((number, current)));
            }
        }
        previous = Some(std::mem::take(&mut current));
    }
    Ok(None)
}

/// The directory for temporary files: the first `-T` directory, else `TMPDIR` (or the
/// platform default).
fn temp_directory(config: &Config) -> PathBuf {
    match config.temp_dirs.first() {
        Some(dir) => PathBuf::from(dir),
        None => std::env::temp_dir(),
    }
}

fn open_output(path: Option<&str>) -> Result<Box<dyn Write>, i32> {
    match path {
        None => Ok(Box::new(stdout_writer())),
        Some(path) => match File::create(path) {
            Ok(file) => Ok(Box::new(BufWriter::new(file))),
            Err(error) => {
                eprintln!("{CMD}: open failed: {path}: {}", io_error_text(&error));
                Err(EXIT_TROUBLE)
            }
        },
    }
}

fn report_write_or_read(error: &io::Error) {
    if !is_broken_pipe(error) {
        eprintln!("{CMD}: {}", io_error_text(error));
    }
}

/// Maps the final write result to the exit status: a closed pipe is silent success.
fn finish_result(result: io::Result<()>, status: i32) -> i32 {
    match result {
        Ok(()) => status,
        Err(error) if is_broken_pipe(&error) => status,
        Err(error) => {
            eprintln!("{CMD}: write failed: {}", io_error_text(&error));
            EXIT_TROUBLE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_seed_has_sixteen_bytes() {
        assert_eq!(default_seed().len(), SEED_BYTES);
    }

    #[test]
    fn disorder_reports_first_bad_line() {
        let comparer = Comparer {
            keys: vec![Key {
                def: KeyDef {
                    start_field: 0,
                    start_char: 0,
                    end: None,
                    modes: Default::default(),
                },
                modes: Default::default(),
            }],
            tab: None,
            global_reverse: false,
            last_resort: true,
            unique: false,
            seed: Vec::new(),
        };
        let mut data: &[u8] = b"a\nc\nb\nd\n";
        let found = first_disorder(&mut data, &comparer, b'\n').unwrap();
        assert_eq!(found, Some((3, b"b".to_vec())));
        let mut sorted: &[u8] = b"a\nb\n";
        assert_eq!(first_disorder(&mut sorted, &comparer, b'\n').unwrap(), None);
    }

    #[test]
    fn version_flag_prints_name_and_version() {
        assert_eq!(run(&["--version".to_owned()]), EXIT_OK);
    }
}
