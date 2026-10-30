//! `-c` verification shared by `md5sum` … `b2sum` and `cksum -c`.
//!
//! Lines are either GNU style (`HASH  NAME`, `HASH *NAME`) or BSD style
//! (`ALGO (NAME) = HASH`). A leading backslash marks an escaped name.

use std::io::Write;

use super::escape_name;
use super::hasher::{digest_reader, from_hex, Algo};
use crate::common::error::{io_error_text, EXIT_FAIL, EXIT_OK};
use crate::common::escape::quote_name;
use crate::common::io::{open_input, read_record};

/// Options that shape a verification run.
pub struct CheckOpts<'a> {
    /// Name used as the message prefix.
    pub cmd: &'a str,
    /// The algorithm every line must use. `None` accepts only BSD-style lines and takes
    /// the algorithm from each tag (`cksum -c` without `-a`).
    pub forced: Option<Algo>,
    /// BLAKE2b: the digest length comes from each line instead of `forced`.
    pub variable_length: bool,
    pub zero: bool,
    pub quiet: bool,
    pub status: bool,
    pub strict: bool,
    pub warn: bool,
    pub ignore_missing: bool,
}

/// One well-formed checksum line.
#[derive(Debug, PartialEq, Eq)]
struct CheckLine {
    /// The name with escapes removed.
    name: String,
    escaped: bool,
    algo: Algo,
    digest: Vec<u8>,
}

#[derive(Default)]
struct Totals {
    improper: u64,
    mismatched: u64,
    unreadable: u64,
    failed: bool,
}

/// Verifies every checksum listed in `files`. Results go to `out`; diagnostics go to
/// stderr. Returns the exit status.
pub fn run(opts: &CheckOpts, files: &[String], out: &mut dyn Write) -> i32 {
    let mut totals = Totals::default();
    for file in files {
        check_list(opts, file, &mut totals, out);
    }
    if !opts.status {
        summarize(opts.cmd, &totals);
    }
    if totals.failed || totals.mismatched > 0 || totals.unreadable > 0 {
        return EXIT_FAIL;
    }
    if opts.strict && totals.improper > 0 {
        return EXIT_FAIL;
    }
    EXIT_OK
}

fn check_list(opts: &CheckOpts, file: &str, totals: &mut Totals, out: &mut dyn Write) {
    let display = display_operand(file);
    let mut reader = match open_input(file) {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("{}: {display}: {}", opts.cmd, io_error_text(&error));
            totals.failed = true;
            return;
        }
    };
    let delim = if opts.zero { b'\0' } else { b'\n' };
    let mut buf = Vec::new();
    let mut lineno = 0u64;
    let mut valid = 0u64;
    let mut verified = 0u64;
    loop {
        buf.clear();
        match read_record(reader.as_mut(), delim, &mut buf) {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => {
                eprintln!("{}: {display}: {}", opts.cmd, io_error_text(&error));
                totals.failed = true;
                break;
            }
        }
        lineno += 1;
        if buf.last() == Some(&delim) {
            buf.pop();
        }
        let text = String::from_utf8_lossy(&buf).into_owned();
        let Some(line) = parse_line(&text, opts) else {
            totals.improper += 1;
            if opts.warn {
                let label = match opts.forced {
                    Some(algo) => format!("{} ", algo.label()),
                    None => String::new(),
                };
                eprintln!(
                    "{}: {display}: {lineno}: improperly formatted {label}checksum line",
                    opts.cmd
                );
            }
            continue;
        };
        valid += 1;
        if check_one(opts, &line, totals, out) {
            verified += 1;
        }
    }
    if valid == 0 {
        eprintln!(
            "{}: {display}: no properly formatted checksum lines found",
            opts.cmd
        );
        totals.failed = true;
    } else if opts.ignore_missing && verified == 0 {
        eprintln!("{}: {display}: no file was verified", opts.cmd);
        totals.failed = true;
    }
}

/// Checks one line against its file. Returns `true` when the file was read and digested
/// (whatever the result), `false` when it was skipped or unreadable.
fn check_one(opts: &CheckOpts, line: &CheckLine, totals: &mut Totals, out: &mut dyn Write) -> bool {
    // GNU shows the name escaped whenever it needs escaping, even if the line was not
    // written with a leading backslash.
    let (prefix, body) = escape_name(&line.name, opts.zero);
    let shown = format!("{}{body}", if prefix { "\\" } else { "" });
    let mut reader = match open_input(&line.name) {
        Ok(reader) => reader,
        Err(error) => return open_failed(opts, line, &shown, &error, totals, out),
    };
    let digest = match digest_reader(line.algo, reader.as_mut()) {
        Ok((digest, _)) => digest,
        Err(error) => return open_failed(opts, line, &shown, &error, totals, out),
    };
    if digest == line.digest {
        if !opts.quiet && !opts.status {
            let _ = writeln!(out, "{shown}: OK");
        }
    } else {
        if !opts.status {
            let _ = writeln!(out, "{shown}: FAILED");
        }
        totals.mismatched += 1;
    }
    true
}

fn open_failed(
    opts: &CheckOpts,
    line: &CheckLine,
    shown: &str,
    error: &std::io::Error,
    totals: &mut Totals,
    out: &mut dyn Write,
) -> bool {
    if opts.ignore_missing && error.kind() == std::io::ErrorKind::NotFound {
        return false;
    }
    eprintln!(
        "{}: {}: {}",
        opts.cmd,
        quote_name(&line.name),
        io_error_text(error)
    );
    if !opts.status {
        let _ = writeln!(out, "{shown}: FAILED open or read");
    }
    totals.unreadable += 1;
    false
}

fn summarize(cmd: &str, totals: &Totals) {
    if totals.improper > 0 {
        let n = totals.improper;
        let text = if n == 1 {
            "1 line is improperly formatted".to_owned()
        } else {
            format!("{n} lines are improperly formatted")
        };
        eprintln!("{cmd}: WARNING: {text}");
    }
    if totals.mismatched > 0 {
        let n = totals.mismatched;
        let text = if n == 1 {
            "1 computed checksum did NOT match".to_owned()
        } else {
            format!("{n} computed checksums did NOT match")
        };
        eprintln!("{cmd}: WARNING: {text}");
    }
    if totals.unreadable > 0 {
        let n = totals.unreadable;
        let text = if n == 1 {
            "1 listed file could not be read".to_owned()
        } else {
            format!("{n} listed files could not be read")
        };
        eprintln!("{cmd}: WARNING: {text}");
    }
}

/// Names in diagnostics follow GNU quoting; standard input is shown as it is named.
fn display_operand(file: &str) -> String {
    if file == "-" {
        quote_name("standard input")
    } else {
        quote_name(file)
    }
}

/// Parses one checksum line. Returns `None` when the line is improperly formatted.
fn parse_line(text: &str, opts: &CheckOpts) -> Option<CheckLine> {
    let (escaped, body) = match text.strip_prefix('\\') {
        Some(rest) if !opts.zero => (true, rest),
        _ => (false, text),
    };
    let (name, algo, digest) = parse_untagged(body, opts).or_else(|| parse_tagged(body, opts))?;
    let name = if escaped {
        unescape(name)?
    } else {
        name.to_owned()
    };
    Some(CheckLine {
        name,
        escaped,
        algo,
        digest,
    })
}

/// `HASH  NAME` or `HASH *NAME`; needs a forced algorithm.
fn parse_untagged<'a>(body: &'a str, opts: &CheckOpts) -> Option<(&'a str, Algo, Vec<u8>)> {
    let forced = opts.forced?;
    let hex_len = body.bytes().take_while(u8::is_ascii_hexdigit).count();
    let rest = &body[hex_len..];
    if !(rest.starts_with("  ") || rest.starts_with(" *")) {
        return None;
    }
    let digest = from_hex(&body[..hex_len])?;
    let algo = resolve_length(forced, digest.len(), opts)?;
    Some((&rest[2..], algo, digest))
}

/// `ALGO (NAME) = HASH`; the name may itself contain `) = `, so the last one splits.
fn parse_tagged<'a>(body: &'a str, opts: &CheckOpts) -> Option<(&'a str, Algo, Vec<u8>)> {
    let open = body.find(" (")?;
    let tag = &body[..open];
    let rest = &body[open + 2..];
    let split = rest.rfind(") = ")?;
    let name = &rest[..split];
    let hash = &rest[split + 4..];
    let algo = Algo::from_tag(tag)?;
    let algo = match opts.forced {
        Some(forced) if opts.variable_length && matches!(forced, Algo::Blake2b(_)) => {
            if !matches!(algo, Algo::Blake2b(_)) {
                return None;
            }
            algo
        }
        Some(forced) if forced != algo => return None,
        _ => algo,
    };
    let digest = from_hex(hash)?;
    if digest.len() != algo.digest_len() {
        return None;
    }
    Some((name, algo, digest))
}

/// The algorithm for a digest of `len` bytes. Fixed algorithms need an exact length;
/// BLAKE2b takes any length from 1 to 64 bytes.
fn resolve_length(forced: Algo, len: usize, opts: &CheckOpts) -> Option<Algo> {
    if opts.variable_length && matches!(forced, Algo::Blake2b(_)) {
        return (1..=64).contains(&len).then_some(Algo::Blake2b(len));
    }
    (len == forced.digest_len()).then_some(forced)
}

/// Undoes the `\\` and `\n` escapes of an escaped line. Any other escape is malformed.
fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(forced: Option<Algo>) -> CheckOpts<'static> {
        CheckOpts {
            cmd: "test",
            forced,
            variable_length: false,
            zero: false,
            quiet: false,
            status: false,
            strict: false,
            warn: false,
            ignore_missing: false,
        }
    }

    const MD5_ABC: &str = "900150983cd24fb0d6963f7d28e17f72";

    #[test]
    fn parses_gnu_text_and_binary_lines() {
        let o = opts(Some(Algo::Md5));
        let text = parse_line(&format!("{MD5_ABC}  a.txt"), &o).expect("text line");
        assert_eq!(text.name, "a.txt");
        let binary = parse_line(&format!("{MD5_ABC} *a.txt"), &o).expect("binary line");
        assert_eq!(binary.name, "a.txt");
        assert!(!binary.escaped);
    }

    #[test]
    fn accepts_bsd_lines_and_uppercase_hex() {
        let o = opts(Some(Algo::Md5));
        let line = parse_line(&format!("MD5 (x) = {}", MD5_ABC.to_uppercase()), &o);
        assert_eq!(line.map(|l| l.name), Some("x".to_owned()));
        assert!(parse_line(&format!("SHA1 (x) = {MD5_ABC}"), &o).is_none());
    }

    #[test]
    fn rejects_bad_lengths_and_separators() {
        let o = opts(Some(Algo::Md5));
        assert!(parse_line(&format!("{MD5_ABC} a"), &o).is_none());
        assert!(parse_line("abcd  a", &o).is_none());
        assert!(parse_line("junk", &o).is_none());
    }

    #[test]
    fn escaped_names_are_unescaped() {
        let o = opts(Some(Algo::Md5));
        let line = parse_line(&format!("\\{MD5_ABC}  a\\nb\\\\c"), &o).expect("escaped");
        assert_eq!(line.name, "a\nb\\c");
        assert!(line.escaped);
        assert!(unescape("bad\\t").is_none());
    }

    #[test]
    fn blake2b_length_comes_from_the_line() {
        let mut o = opts(Some(Algo::Blake2b(64)));
        o.variable_length = true;
        let hex32 = "00".repeat(32);
        let line = parse_line(&format!("{hex32}  f"), &o).expect("32-byte line");
        assert_eq!(line.algo, Algo::Blake2b(32));
        let tagged = parse_line(&format!("BLAKE2b-256 (f) = {hex32}"), &o).expect("tagged");
        assert_eq!(tagged.algo, Algo::Blake2b(32));
    }

    #[test]
    fn without_forced_algorithm_only_tags_are_accepted() {
        let o = opts(None);
        assert!(parse_line(&format!("{MD5_ABC}  a"), &o).is_none());
        let line = parse_line(&format!("MD5 (a) = {MD5_ABC}"), &o).expect("tag");
        assert_eq!(line.algo, Algo::Md5);
    }

    #[test]
    fn tagged_name_may_contain_separator_text() {
        let o = opts(Some(Algo::Md5));
        let line = parse_line(&format!("MD5 (a) = b) = c) = {MD5_ABC}"), &o).expect("tag");
        assert_eq!(line.name, "a) = b) = c");
    }
}
