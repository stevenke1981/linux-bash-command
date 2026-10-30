//! `xxd`: a vim xxd subset: hex dump, binary digits, plain hex, C include output and the
//! reverse conversion. Input is streamed; only `-r` output is written as it is decoded.
//!
//! xxd's long options are single-dash words (`-cols`, `-len`, `-seek`, `-ps`), so the
//! arguments are scanned by hand rather than with the GNU parser.

mod reverse;

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek as _, SeekFrom, Write};

use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_TROUBLE};

use reverse::reverse;
#[cfg(test)]
use reverse::{decode_dump_line, decode_plain, hex_value};

const HELP: &str = "\
Usage:
       xxd [options] [infile [outfile]]
    or
       xxd -r [-s [-]offset] [-c cols] [-ps] [infile [outfile]]
Options:
    -a          toggle autoskip: a single '*' replaces runs of nul-lines.
    -b          binary digit dump (incompatible with -ps, -i, -r). Default hex.
    -C          capitalize variable names in C include style (with -i).
    -c cols     format <cols> octets per line. Default 16 (-i: 12, -ps: 30, -b: 6).
                Max 256. -cols cols is accepted as well.
    -d          show offset in decimal instead of hex.
    -e          little-endian dump (incompatible with -ps, -i, -r).
    -g bytes    number of octets per group in normal output. Default 2 (-e: 4).
    -h          print this summary.
    -i          output in C include file style.
    -l len      stop after <len> octets. -len len is accepted as well.
    -n name     set the variable name used in C include output (-i).
    -o off      add <off> to the displayed file position.
    -p, -ps, -postscript, -plain
                output in postscript plain hexdump style.
    -r          reverse operation: convert a hexdump into binary.
    -r -s off   revert with <off> added to file positions found in the hexdump.
    -s [+][-]seek
                start at <seek> bytes abs. (or +: rel.); -: relative to the end.
                -seek seek is accepted as well.
    -u          use upper case hex letters.
    -v, --version
                show version.
    --help      display this help and exit.
";

/// What the program produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Dump,
    Bits,
    Plain,
    Include,
    Reverse,
}

/// Where `-s` starts reading: `Start(n)` from the beginning, `End(n)` from the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeekPos {
    Start(u64),
    End(u64),
}

/// Parsed options. The output flags are kept apart so that `-r -p` can be combined; see
/// [`Opts::mode`] for the precedence.
#[derive(Debug, Default)]
struct Opts {
    reverse: bool,
    include: bool,
    plain: bool,
    bits: bool,
    cols: Option<usize>,
    group: Option<usize>,
    little: bool,
    upper: bool,
    capitals: bool,
    autoskip: bool,
    decimal: bool,
    len: Option<u64>,
    offset: i64,
    seek: Option<SeekPos>,
    name: Option<String>,
    infile: Option<String>,
    outfile: Option<String>,
}

impl Opts {
    /// The output mode. Reverse wins over include, include over plain, plain over bits.
    fn mode(&self) -> Mode {
        if self.reverse {
            Mode::Reverse
        } else if self.include {
            Mode::Include
        } else if self.plain {
            Mode::Plain
        } else if self.bits {
            Mode::Bits
        } else {
            Mode::Dump
        }
    }
}

/// Entry point for the `xxd` command.
pub fn run(args: &[String]) -> i32 {
    let opts = match parse_args(args) {
        Ok(opts) => opts,
        Err(status) => return status,
    };
    match execute(&opts) {
        Ok(()) => 0,
        Err(Failure::Message(message, status)) => {
            eprintln!("xxd: {message}");
            status
        }
        Err(Failure::Io(name, error)) => {
            eprintln!("xxd: {name}: {}", io_error_text(&error));
            EXIT_TROUBLE
        }
    }
}

/// Runtime failures.
enum Failure {
    Io(String, io::Error),
    /// A complete message, printed as `xxd: MESSAGE`, with its exit status.
    Message(&'static str, i32),
}

fn usage_error() -> i32 {
    eprint!("{HELP}");
    EXIT_FAIL
}

/// Scans the arguments. Help and version are printed here and reported as `Err(0)`.
fn parse_args(args: &[String]) -> Result<Opts, i32> {
    let mut opts = Opts::default();
    let mut operands = Vec::new();
    let mut i = 0;
    let mut only_operands = false;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if only_operands || arg == "-" || !arg.starts_with('-') {
            operands.push(arg.to_string());
            continue;
        }
        if arg == "--" {
            only_operands = true;
            continue;
        }
        match arg {
            "--help" => {
                print!("{HELP}");
                return Err(0);
            }
            "--version" | "-v" => return Err(print_version("xxd")),
            "-cols" | "-len" | "-seek" => {
                let value = take_value(args, &mut i).ok_or_else(usage_error)?;
                let letter = match arg {
                    "-cols" => 'c',
                    "-len" => 'l',
                    _ => 's',
                };
                apply_value(&mut opts, letter, &value)?;
            }
            "-ps" | "-postscript" | "-plain" => opts.plain = true,
            _ if arg.starts_with("--") => return Err(usage_error()),
            _ => parse_cluster(&mut opts, args, &mut i, arg)?,
        }
    }
    if operands.len() > 2 {
        return Err(usage_error());
    }
    if opts.little && opts.group.is_some_and(|g| g > 0 && !g.is_power_of_two()) {
        eprintln!("xxd: number of octets per group must be a power of 2 with -e.");
        return Err(EXIT_FAIL);
    }
    opts.infile = operands.first().cloned();
    opts.outfile = operands.get(1).cloned();
    Ok(opts)
}

/// Handles a cluster of single-letter options such as `-ab` or `-c8`.
fn parse_cluster(opts: &mut Opts, args: &[String], i: &mut usize, arg: &str) -> Result<(), i32> {
    let letters: Vec<char> = arg[1..].chars().collect();
    for (index, &letter) in letters.iter().enumerate() {
        match letter {
            'a' => opts.autoskip = !opts.autoskip,
            'b' => opts.bits = true,
            'C' => opts.capitals = true,
            'd' => opts.decimal = true,
            'e' => opts.little = true,
            'h' => {
                print!("{HELP}");
                return Err(0);
            }
            'i' => opts.include = true,
            'p' => opts.plain = true,
            'r' => opts.reverse = true,
            'u' => opts.upper = true,
            'c' | 'g' | 'l' | 'n' | 'o' | 's' => {
                let rest: String = letters[index + 1..].iter().collect();
                let value = if rest.is_empty() {
                    take_value(args, i).ok_or_else(usage_error)?
                } else {
                    rest
                };
                return apply_value(opts, letter, &value);
            }
            _ => return Err(usage_error()),
        }
    }
    Ok(())
}

fn take_value(args: &[String], i: &mut usize) -> Option<String> {
    let value = args.get(*i)?.clone();
    *i += 1;
    Some(value)
}

/// Applies an option that takes a value.
fn apply_value(opts: &mut Opts, letter: char, value: &str) -> Result<(), i32> {
    match letter {
        'c' => {
            let cols = parse_number(value).ok_or_else(usage_error)?;
            if cols > 256 {
                eprintln!("xxd: invalid number of columns (max. 256).");
                return Err(EXIT_FAIL);
            }
            opts.cols = (cols > 0).then_some(cols as usize);
        }
        'g' => {
            let group = parse_number(value).ok_or_else(usage_error)?;
            opts.group = Some(group as usize);
        }
        'l' => opts.len = Some(parse_number(value).ok_or_else(usage_error)?),
        'o' => opts.offset = parse_signed(value).ok_or_else(usage_error)?,
        'n' => opts.name = Some(value.to_string()),
        's' => opts.seek = Some(parse_seek(value).ok_or_else(usage_error)?),
        _ => {}
    }
    Ok(())
}

/// Parses a decimal or `0x` hexadecimal unsigned number.
fn parse_number(text: &str) -> Option<u64> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn parse_signed(text: &str) -> Option<i64> {
    match text.strip_prefix('-') {
        Some(rest) => parse_number(rest).map(|n| (n as i64).wrapping_neg()),
        None => parse_number(text.strip_prefix('+').unwrap_or(text)).map(|n| n as i64),
    }
}

/// Parses `[+][-]seek`: `+` is accepted and means the start, `-` means from the end.
fn parse_seek(text: &str) -> Option<SeekPos> {
    let text = text.strip_prefix('+').unwrap_or(text);
    match text.strip_prefix('-') {
        Some(rest) => parse_number(rest).map(SeekPos::End),
        None => parse_number(text).map(SeekPos::Start),
    }
}

fn execute(opts: &Opts) -> Result<(), Failure> {
    if opts.mode() == Mode::Reverse {
        return reverse(opts);
    }
    let name = opts.infile.clone().unwrap_or_else(|| "-".to_string());
    let (mut reader, total) = open_input(&name, opts.seek)?;
    let base = match opts.seek {
        Some(SeekPos::Start(n)) => n,
        Some(SeekPos::End(n)) => total.saturating_sub(n),
        None => 0,
    };
    let mut out = open_output(opts.outfile.as_deref())?;
    let limit = opts.len.unwrap_or(u64::MAX);
    let address_base = (base as i64).wrapping_add(opts.offset) as u64;
    let result = match opts.mode() {
        Mode::Include => include(opts, &name, reader.as_mut(), limit, &mut out),
        mode => dump(opts, mode, reader.as_mut(), limit, address_base, &mut out),
    };
    result
        .and_then(|()| out.flush())
        .map_err(|error| Failure::Io(name, error))
}

/// Opens the input and returns it positioned at the seek point, with the total length
/// when it is known (regular files).
fn open_input(name: &str, seek: Option<SeekPos>) -> Result<(Box<dyn Read>, u64), Failure> {
    if name == "-" {
        let mut stdin = io::stdin().lock();
        if let Some(SeekPos::End(_)) = seek {
            // Exit status 4 is what xxd uses for a seek that is not possible.
            return Err(Failure::Message("Sorry, cannot seek.", 4));
        }
        if let Some(SeekPos::Start(n)) = seek {
            io::copy(&mut (&mut stdin).take(n), &mut io::sink())
                .map_err(|e| Failure::Io(name.to_string(), e))?;
        }
        return Ok((Box::new(stdin), 0));
    }
    let mut file = File::open(name).map_err(|e| Failure::Io(name.to_string(), e))?;
    let total = file
        .metadata()
        .map(|m| m.len())
        .map_err(|e| Failure::Io(name.to_string(), e))?;
    match seek {
        Some(SeekPos::Start(n)) => {
            file.seek(SeekFrom::Start(n))
                .map_err(|e| Failure::Io(name.to_string(), e))?;
        }
        Some(SeekPos::End(n)) => {
            file.seek(SeekFrom::Start(total.saturating_sub(n)))
                .map_err(|e| Failure::Io(name.to_string(), e))?;
        }
        None => {}
    }
    Ok((Box::new(BufReader::new(file)), total))
}

fn open_output(name: Option<&str>) -> Result<Box<dyn Write>, Failure> {
    match name {
        None | Some("-") => Ok(Box::new(BufWriter::with_capacity(64 * 1024, io::stdout()))),
        Some(path) => {
            let file = File::create(path).map_err(|e| Failure::Io(path.to_string(), e))?;
            Ok(Box::new(BufWriter::new(file)))
        }
    }
}

/// Fills `buf` from `reader` until it is full or the input ends; returns the count read.
fn read_chunk(reader: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

/// Hex, binary and the dumps built on them, with autoskip.
fn dump(
    opts: &Opts,
    mode: Mode,
    reader: &mut dyn Read,
    limit: u64,
    base: u64,
    out: &mut dyn Write,
) -> io::Result<()> {
    let cols = opts.cols.unwrap_or(match mode {
        Mode::Bits => 6,
        Mode::Plain => 30,
        _ => 16,
    });
    let mut buf = vec![0u8; cols];
    let mut done = 0u64;
    let mut line_index = 0u64;
    let mut last_zero = false;
    let mut star_shown = false;
    while done < limit {
        let want = (limit - done).min(cols as u64) as usize;
        let n = read_chunk(reader, &mut buf[..want])?;
        if n == 0 {
            break;
        }
        let line = &buf[..n];
        done += n as u64;
        let address = base.wrapping_add(line_index * cols as u64);
        line_index += 1;
        if mode == Mode::Plain {
            writeln!(out, "{}", plain_hex(line, opts.upper))?;
            continue;
        }
        let zero = n == cols && line.iter().all(|&b| b == 0);
        if opts.autoskip && zero {
            if last_zero {
                if !star_shown {
                    writeln!(out, "*")?;
                    star_shown = true;
                }
                last_zero = true;
                continue;
            }
        } else {
            star_shown = false;
        }
        last_zero = zero;
        let text = if mode == Mode::Bits {
            bits_line(address, line, cols, opts)
        } else {
            hex_line(address, line, cols, opts)
        };
        out.write_all(text.as_bytes())?;
    }
    Ok(())
}

fn address_text(address: u64, decimal: bool) -> String {
    if decimal {
        format!("{address:08}")
    } else {
        format!("{address:08x}")
    }
}

fn hex_digits(byte: u8, upper: bool) -> String {
    if upper {
        format!("{byte:02X}")
    } else {
        format!("{byte:02x}")
    }
}

fn plain_hex(line: &[u8], upper: bool) -> String {
    line.iter().map(|&b| hex_digits(b, upper)).collect()
}

/// One normal dump line: address, grouped hex, then the printable characters.
fn hex_line(address: u64, line: &[u8], cols: usize, opts: &Opts) -> String {
    let group = opts.group.unwrap_or(if opts.little { 4 } else { 2 });
    let mut text = format!("{}: ", address_text(address, opts.decimal));
    text.push_str(&hex_area(line, cols, group, opts));
    // Little-endian lines leave one more column before the characters.
    text.push_str(if opts.little { "   " } else { "  " });
    text.push_str(&ascii(line));
    text.push('\n');
    text
}

/// The hex columns of a normal line, padded to the full width for a short last line.
fn hex_area(line: &[u8], cols: usize, group: usize, opts: &Opts) -> String {
    let mut out = String::new();
    if opts.little && group > 0 {
        for start in (0..cols).step_by(group) {
            if start > 0 {
                out.push(' ');
            }
            let present: &[u8] = if start < line.len() {
                &line[start..line.len().min(start + group)]
            } else {
                &[]
            };
            for _ in present.len()..group {
                out.push_str("  ");
            }
            for &byte in present.iter().rev() {
                out.push_str(&hex_digits(byte, opts.upper));
            }
        }
        return out;
    }
    for index in 0..cols {
        if group > 0 && index > 0 && index % group == 0 {
            out.push(' ');
        }
        match line.get(index) {
            Some(&byte) => out.push_str(&hex_digits(byte, opts.upper)),
            None => out.push_str("  "),
        }
    }
    out
}

/// One binary dump line: eight digits per byte, separated by `-g` groups (default 1).
fn bits_line(address: u64, line: &[u8], cols: usize, opts: &Opts) -> String {
    let group = opts.group.unwrap_or(1);
    let mut text = format!("{}: ", address_text(address, opts.decimal));
    for index in 0..cols {
        if group > 0 && index > 0 && index % group == 0 {
            text.push(' ');
        }
        match line.get(index) {
            Some(&byte) => text.push_str(&format!("{byte:08b}")),
            None => text.push_str("        "),
        }
    }
    text.push_str("  ");
    text.push_str(&ascii(line));
    text.push('\n');
    text
}

fn ascii(line: &[u8]) -> String {
    line.iter()
        .map(|&b| {
            if (0x20..0x7f).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect()
}

/// C include output: an array of the bytes and, when there is a name, its length.
fn include(
    opts: &Opts,
    input_name: &str,
    reader: &mut dyn Read,
    limit: u64,
    out: &mut dyn Write,
) -> io::Result<()> {
    let name = opts.name.clone().or_else(|| {
        (input_name != "-").then(|| {
            input_name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect()
        })
    });
    let name = name.map(|n| if opts.capitals { n.to_uppercase() } else { n });
    let cols = opts.cols.unwrap_or(12);
    if let Some(name) = &name {
        writeln!(out, "unsigned char {name}[] = {{")?;
    }
    let mut buf = vec![0u8; cols];
    let mut count = 0u64;
    // A line gets its comma only when another line follows, so one line is held back.
    let mut held: Option<String> = None;
    loop {
        let want = (limit - count).min(cols as u64) as usize;
        if want == 0 {
            break;
        }
        let n = read_chunk(reader, &mut buf[..want])?;
        if n == 0 {
            break;
        }
        let mut line = String::from("  ");
        for (index, &byte) in buf[..n].iter().enumerate() {
            if index > 0 {
                line.push_str(", ");
            }
            line.push_str(&format!("0x{}", hex_digits(byte, opts.upper)));
        }
        count += n as u64;
        if let Some(previous) = held.replace(line) {
            writeln!(out, "{previous},")?;
        }
        if n < want {
            break;
        }
    }
    if let Some(last) = held {
        writeln!(out, "{last}")?;
    }
    if let Some(name) = &name {
        writeln!(out, "}};")?;
        let var = if opts.capitals {
            format!("{name}_LEN")
        } else {
            format!("{name}_len")
        };
        writeln!(out, "unsigned int {var} = {count};")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(args: &[&str]) -> Opts {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_args(&args).unwrap()
    }

    #[test]
    fn default_line_matches_xxd_layout() {
        let o = opts(&[]);
        assert_eq!(
            hex_line(0, b"hello\n", 16, &o),
            "00000000: 6865 6c6c 6f0a                           hello.\n"
        );
    }

    #[test]
    fn little_endian_groups_are_reversed() {
        let o = opts(&["-e"]);
        let line = hex_line(0, b"hello\n", 16, &o);
        assert!(line.starts_with("00000000: 6c6c6568     0a6f"), "{line}");
    }

    #[test]
    fn group_zero_has_no_separators() {
        let o = opts(&["-g0"]);
        // Ten characters of address, then 32 hex columns with no separators, then the text.
        let line = hex_line(0, b"hi", 16, &o);
        assert!(line.starts_with("00000000: 6869"));
        assert_eq!(line.find("  hi"), Some(42));
    }

    #[test]
    fn bits_line_shows_eight_digits_per_byte() {
        let o = opts(&["-b"]);
        let line = bits_line(0, b"hi", 6, &o);
        assert!(line.starts_with("00000000: 01101000 01101001 "), "{line}");
        assert!(line.ends_with("  hi\n"), "{line}");
    }

    #[test]
    fn decode_plain_pairs_across_lines() {
        let mut pending = None;
        assert_eq!(decode_plain(b"68 6", &mut pending), vec![0x68]);
        assert_eq!(decode_plain(b"9\n", &mut pending), vec![0x69]);
    }

    #[test]
    fn include_output_has_array_and_length() {
        let o = opts(&["-i", "-c", "4"]);
        let mut out = Vec::new();
        include(&o, "a.bin", &mut &b"hello"[..], u64::MAX, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "unsigned char a_bin[] = {\n  0x68, 0x65, 0x6c, 0x6c,\n  0x6f\n};\nunsigned int a_bin_len = 5;\n"
        );
    }

    #[test]
    fn reverse_parses_dump_and_plain_hex() {
        let (address, bytes) =
            decode_dump_line(b"00000010: 6865 6c6c 6f0a                           hello.\n");
        assert_eq!(address, Some(16));
        assert_eq!(bytes, b"hello\n");
        let (address, bytes) = decode_dump_line(b"ab cd ef\n");
        assert_eq!(address, None);
        assert_eq!(bytes, vec![0xab, 0xcd, 0xef]);
    }

    #[test]
    fn option_values_and_words_are_parsed() {
        let o = opts(&["-cols", "8", "-len", "16", "-s", "-4"]);
        assert_eq!(o.cols, Some(8));
        assert_eq!(o.len, Some(16));
        assert_eq!(o.seek, Some(SeekPos::End(4)));
        let o = opts(&["-ps", "-c8", "--", "-odd-name"]);
        assert_eq!(o.mode(), Mode::Plain);
        assert_eq!(opts(&["-r", "-p"]).mode(), Mode::Reverse);
        assert_eq!(o.cols, Some(8));
        assert_eq!(o.infile.as_deref(), Some("-odd-name"));
    }

    #[test]
    fn bad_options_are_usage_errors() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_args(&args(&["-x"])).err(), Some(EXIT_FAIL));
        assert_eq!(parse_args(&args(&["-c", "300"])).err(), Some(EXIT_FAIL));
        assert_eq!(parse_args(&args(&["--bogus"])).err(), Some(EXIT_FAIL));
    }

    #[test]
    fn plain_hex_and_autoskip_helpers() {
        assert_eq!(plain_hex(b"AB", true), "4142");
        assert_eq!(address_text(255, false), "000000ff");
        assert_eq!(address_text(255, true), "00000255");
        assert_eq!(hex_value(b'g'), None);
    }
}
