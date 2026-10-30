//! `cksum` (CRC and the digest algorithms) and `sum` (BSD and System V checksums).

mod legacy;

use std::io::Write;

use legacy::{blocks, checksum, Legacy};

use crate::commands::basenc::codec::base64_text;
use crate::commands::hashsum::check::{self, CheckOpts};
use crate::commands::hashsum::escape_name;
use crate::commands::hashsum::hasher::{digest_reader, to_hex, Algo};
use crate::commands::hashsum::parse_length;
use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::escape::quote_name;
use crate::common::io::{finish_output, open_input, stdout_writer};

const CKSUM: &str = "cksum";
const SUM: &str = "sum";
const MAX_BLAKE2B_BYTES: usize = 64;

const CKSUM_HELP: &str = "\
Usage: cksum [OPTION]... [FILE]...
Print or verify checksums.
By default use the 32 bit CRC algorithm.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -a, --algorithm=TYPE  select the digest type to use.  See DIGEST below.
      --base64          emit base64-encoded digests, not hexadecimal
  -c, --check           read checksums from the FILEs and check them
  -l, --length=BITS     digest length in bits; must not exceed the max for
                          the blake2 algorithm and must be a multiple of 8
      --raw             emit a raw binary digest, not hexadecimal
      --tag             create a BSD-style checksum (the default)
      --untagged        create a reversed style checksum, without digest type
  -z, --zero            end each output line with NUL, not newline,
                          and disable file name escaping

The following five options are useful only when verifying checksums:
      --ignore-missing  don't fail or report status for missing files
      --quiet           don't print OK for each successfully verified file
      --status          don't output anything, status code shows success
      --strict          exit non-zero for improperly formatted checksum lines
  -w, --warn            warn about improperly formatted checksum lines

      --help        display this help and exit
      --version     output version information and exit

DIGEST determines the digest algorithm and default output format:
  sysv      (equivalent to sum -s)
  bsd       (equivalent to sum -r)
  crc       (equivalent to cksum)
  crc32b    CRC-32 as used by gzip and zlib
  md5       (equivalent to md5sum)
  sha1      (equivalent to sha1sum)
  sha224    (equivalent to sha224sum)
  sha256    (equivalent to sha256sum)
  sha384    (equivalent to sha384sum)
  sha512    (equivalent to sha512sum)
  blake2b   (equivalent to b2sum)

When checking, the input should be a former output of this program,
or equivalent standalone program.
";

const SUM_HELP: &str = "\
Usage: sum [OPTION]... [FILE]...
Print or check BSD (16-bit) checksums.

With no FILE, or when FILE is -, read standard input.

  -r              use BSD sum algorithm (the default), use 1K blocks
  -s, --sysv      use System V sum algorithm, use 512 bytes blocks
      --help        display this help and exit
      --version     output version information and exit
";

const CKSUM_OPTIONS: &[OptSpec] = &[
    OptSpec::req("algorithm", Some('a'), Some("algorithm")),
    OptSpec::flag("base64", None, Some("base64")),
    OptSpec::flag("check", Some('c'), Some("check")),
    OptSpec::req("length", Some('l'), Some("length")),
    OptSpec::flag("raw", None, Some("raw")),
    OptSpec::flag("tag", None, Some("tag")),
    OptSpec::flag("untagged", None, Some("untagged")),
    OptSpec::flag("zero", Some('z'), Some("zero")),
    OptSpec::flag("ignore-missing", None, Some("ignore-missing")),
    OptSpec::flag("quiet", None, Some("quiet")),
    OptSpec::flag("status", None, Some("status")),
    OptSpec::flag("strict", None, Some("strict")),
    OptSpec::flag("warn", Some('w'), Some("warn")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const SUM_OPTIONS: &[OptSpec] = &[
    OptSpec::flag("bsd", Some('r'), None),
    OptSpec::flag("sysv", Some('s'), Some("sysv")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const ALGORITHMS: &[&str] = &[
    "bsd", "sysv", "crc", "crc32b", "md5", "sha1", "sha224", "sha256", "sha384", "sha512",
    "blake2b",
];

/// What `-a` selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    Legacy(Legacy),
    Hash(Algo),
}

/// Parsed `cksum` options.
struct Options {
    method: Option<Method>,
    length: Option<usize>,
    base64: bool,
    raw: bool,
    tagged: bool,
    check: bool,
    zero: bool,
    ignore_missing: bool,
    quiet: bool,
    status: bool,
    strict: bool,
    warn: bool,
}

/// Entry point for `cksum`.
pub fn cksum(args: &[String]) -> i32 {
    let items = match Parser::new(CKSUM, CKSUM_OPTIONS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CKSUM, &error, EXIT_FAIL),
    };
    let mut opts = Options {
        method: None,
        length: None,
        base64: false,
        raw: false,
        tagged: true,
        check: false,
        zero: false,
        ignore_missing: false,
        quiet: false,
        status: false,
        strict: false,
        warn: false,
    };
    let mut files = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{CKSUM_HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(CKSUM),
            Item::Opt {
                id: "algorithm",
                value,
            } => match parse_method(value.as_deref().unwrap_or("")) {
                Some(method) => opts.method = Some(method),
                None => return invalid_algorithm(value.as_deref().unwrap_or("")),
            },
            Item::Opt {
                id: "length",
                value,
            } => match parse_length(CKSUM, value.as_deref().unwrap_or("")) {
                Ok(bytes) => opts.length = Some(bytes),
                Err(()) => return EXIT_FAIL,
            },
            Item::Opt { id, .. } => set_flag(&mut opts, id),
            Item::Operand(file) => files.push(file),
        }
    }
    if let Some(code) = validate(&opts) {
        return code;
    }
    let implicit = files.is_empty();
    if implicit {
        files.push("-".to_owned());
    }
    if opts.check {
        return verify(&opts, &files);
    }
    let method = opts.method.unwrap_or(Method::Legacy(Legacy::Crc));
    let method = match (method, opts.length) {
        (Method::Hash(Algo::Blake2b(_)), Some(bytes)) => Method::Hash(Algo::Blake2b(bytes)),
        (other, _) => other,
    };
    generate(&opts, method, &files, implicit)
}

/// Entry point for `sum`.
pub fn sum(args: &[String]) -> i32 {
    let items = match Parser::new(SUM, SUM_OPTIONS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(SUM, &error, EXIT_FAIL),
    };
    let mut sysv = false;
    let mut files = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{SUM_HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(SUM),
            Item::Opt { id: "sysv", .. } => sysv = true,
            Item::Opt { id: "bsd", .. } => sysv = false,
            Item::Opt { .. } => {}
            Item::Operand(file) => files.push(file),
        }
    }
    let implicit = files.is_empty();
    if implicit {
        files.push("-".to_owned());
    }
    let kind = if sysv { Legacy::Sysv } else { Legacy::Bsd };
    let mut out = stdout_writer();
    let mut code = EXIT_OK;
    for file in &files {
        match checksum_file(file, kind) {
            Ok(sum) => {
                let line = legacy_text(kind, sum.value, sum.size, name_of(file, implicit));
                if out.write_all(format!("{line}\n").as_bytes()).is_err() {
                    break;
                }
            }
            Err(error) => {
                eprintln!("{SUM}: {}: {}", quote_name(file), io_error_text(&error));
                code = EXIT_FAIL;
            }
        }
    }
    merge(code, finish_output(SUM, &mut out))
}

fn set_flag(opts: &mut Options, id: &str) {
    match id {
        "base64" => opts.base64 = true,
        "check" => opts.check = true,
        "raw" => opts.raw = true,
        "tag" => opts.tagged = true,
        "untagged" => opts.tagged = false,
        "zero" => opts.zero = true,
        "ignore-missing" => opts.ignore_missing = true,
        "quiet" => opts.quiet = true,
        "status" => opts.status = true,
        "strict" => opts.strict = true,
        "warn" => opts.warn = true,
        _ => {}
    }
}

fn parse_method(text: &str) -> Option<Method> {
    Some(match text {
        "sysv" => Method::Legacy(Legacy::Sysv),
        "bsd" => Method::Legacy(Legacy::Bsd),
        "crc" => Method::Legacy(Legacy::Crc),
        "crc32b" => Method::Legacy(Legacy::Crc32b),
        "md5" => Method::Hash(Algo::Md5),
        "sha1" => Method::Hash(Algo::Sha1),
        "sha224" => Method::Hash(Algo::Sha224),
        "sha256" => Method::Hash(Algo::Sha256),
        "sha384" => Method::Hash(Algo::Sha384),
        "sha512" => Method::Hash(Algo::Sha512),
        "blake2b" => Method::Hash(Algo::Blake2b(MAX_BLAKE2B_BYTES)),
        _ => return None,
    })
}

fn invalid_algorithm(text: &str) -> i32 {
    eprintln!("{CKSUM}: invalid argument '{text}' for '--algorithm'");
    eprintln!("Valid arguments are:");
    for name in ALGORITHMS {
        eprintln!("  - '{name}'");
    }
    eprintln!("Try '{CKSUM} --help' for more information.");
    EXIT_FAIL
}

/// GNU's cross-option rules. Returns the exit status when the combination is refused.
fn validate(opts: &Options) -> Option<i32> {
    let usage = |message: &str| {
        let error = UsageError {
            message: message.to_owned(),
        };
        Some(report_usage(CKSUM, &error, EXIT_FAIL))
    };
    if opts.base64 && opts.raw {
        return usage("--base64 and --raw are mutually exclusive");
    }
    let is_blake2b = matches!(opts.method, Some(Method::Hash(Algo::Blake2b(_))));
    if opts.length.is_some() && !is_blake2b {
        eprintln!("{CKSUM}: --length is only supported with --algorithm=blake2b");
        return Some(EXIT_FAIL);
    }
    if !opts.check {
        let checks = [
            (opts.ignore_missing, "ignore-missing"),
            (opts.quiet, "quiet"),
            (opts.status, "status"),
            (opts.strict, "strict"),
            (opts.warn, "warn"),
        ];
        if let Some((_, name)) = checks.iter().find(|(set, _)| *set) {
            return usage(&format!(
                "the --{name} option is meaningful only when verifying checksums"
            ));
        }
    }
    None
}

fn verify(opts: &Options, files: &[String]) -> i32 {
    let forced = match opts.method {
        None => None,
        Some(Method::Legacy(Legacy::Crc32b)) => {
            eprintln!("{CKSUM}: --check is not supported with --algorithm=crc32b");
            return EXIT_FAIL;
        }
        Some(Method::Legacy(_)) => {
            eprintln!("{CKSUM}: --check is not supported with --algorithm={{bsd,sysv,crc}}");
            return EXIT_FAIL;
        }
        Some(Method::Hash(algo)) => Some(algo),
    };
    let check_opts = CheckOpts {
        cmd: CKSUM,
        forced,
        variable_length: matches!(forced, Some(Algo::Blake2b(_))),
        zero: opts.zero,
        quiet: opts.quiet,
        status: opts.status,
        strict: opts.strict,
        warn: opts.warn,
        ignore_missing: opts.ignore_missing,
    };
    let mut out = stdout_writer();
    let code = check::run(&check_opts, files, &mut out);
    merge(code, finish_output(CKSUM, &mut out))
}

fn generate(opts: &Options, method: Method, files: &[String], implicit: bool) -> i32 {
    let mut out = stdout_writer();
    let mut code = EXIT_OK;
    let terminator = if opts.zero { '\0' } else { '\n' };
    for file in files {
        let bytes = match method {
            Method::Legacy(kind) => legacy_line(opts, kind, file, implicit),
            Method::Hash(algo) => hash_line(opts, algo, file),
        };
        match bytes {
            Ok(mut bytes) => {
                if !opts.raw {
                    bytes.extend(terminator.to_string().as_bytes());
                }
                if out.write_all(&bytes).is_err() {
                    break;
                }
            }
            Err(error) => {
                eprintln!("{CKSUM}: {}: {}", quote_name(file), io_error_text(&error));
                code = EXIT_FAIL;
            }
        }
    }
    merge(code, finish_output(CKSUM, &mut out))
}

fn merge(first: i32, second: i32) -> i32 {
    if first != EXIT_OK {
        first
    } else {
        second
    }
}

fn checksum_file(file: &str, kind: Legacy) -> std::io::Result<legacy::Sum> {
    let mut reader = open_input(file)?;
    checksum(kind, reader.as_mut())
}

/// The bytes for one legacy-algorithm operand, without the terminator.
fn legacy_line(
    opts: &Options,
    kind: Legacy,
    file: &str,
    implicit: bool,
) -> std::io::Result<Vec<u8>> {
    let sum = checksum_file(file, kind)?;
    if opts.raw {
        let raw = match kind {
            Legacy::Crc | Legacy::Crc32b => sum.value.to_be_bytes().to_vec(),
            Legacy::Bsd | Legacy::Sysv => (sum.value as u16).to_be_bytes().to_vec(),
        };
        return Ok(raw);
    }
    let text = match kind {
        Legacy::Crc32b => {
            let (escaped, body) = escape_name(file, opts.zero);
            let prefix = if escaped { "\\" } else { "" };
            if opts.tagged {
                format!("{prefix}CRC32B ({body}) = {:08x}", sum.value)
            } else {
                format!("{prefix}{:08x}  {body}", sum.value)
            }
        }
        _ => legacy_text(kind, sum.value, sum.size, name_of(file, implicit)),
    };
    Ok(text.into_bytes())
}

/// The name shown for an operand. Standard input read without an operand has none in
/// the legacy formats.
fn name_of(file: &str, implicit: bool) -> Option<&str> {
    if implicit {
        None
    } else {
        Some(file)
    }
}

/// `CRC SIZE NAME`, `%05d %5d NAME` (BSD) or `%d %d NAME` (System V). The name part is
/// dropped when `name` is `None`.
fn legacy_text(kind: Legacy, value: u32, size: u64, name: Option<&str>) -> String {
    let suffix = match name {
        Some(name) => format!(" {name}"),
        None => String::new(),
    };
    match kind {
        Legacy::Crc | Legacy::Crc32b => format!("{value} {size}{suffix}"),
        Legacy::Bsd => format!("{value:05} {:>5}{suffix}", blocks(size, 1024)),
        Legacy::Sysv => format!("{value} {}{suffix}", blocks(size, 512)),
    }
}

/// The bytes for one digest operand, without the terminator.
fn hash_line(opts: &Options, algo: Algo, file: &str) -> std::io::Result<Vec<u8>> {
    let mut reader = open_input(file)?;
    let (digest, _) = digest_reader(algo, reader.as_mut())?;
    if opts.raw {
        return Ok(digest);
    }
    let encoded = if opts.base64 {
        base64_text(&digest)
    } else {
        to_hex(&digest)
    };
    let (escaped, body) = escape_name(file, opts.zero);
    let prefix = if escaped { "\\" } else { "" };
    let text = if opts.tagged {
        format!("{prefix}{} ({body}) = {encoded}", algo.tag())
    } else {
        format!("{prefix}{encoded}  {body}")
    };
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algorithm_names_are_recognised() {
        assert_eq!(parse_method("crc"), Some(Method::Legacy(Legacy::Crc)));
        assert_eq!(parse_method("sha256"), Some(Method::Hash(Algo::Sha256)));
        assert_eq!(
            parse_method("blake2b"),
            Some(Method::Hash(Algo::Blake2b(MAX_BLAKE2B_BYTES)))
        );
        assert_eq!(parse_method("sm3"), None);
    }

    #[test]
    fn legacy_formats_match_gnu() {
        assert_eq!(
            legacy_text(Legacy::Crc, 3_015_617_425, 6, Some("a")),
            "3015617425 6 a"
        );
        assert_eq!(
            legacy_text(Legacy::Bsd, 36_979, 6, Some("a")),
            "36979     1 a"
        );
        assert_eq!(legacy_text(Legacy::Sysv, 542, 6, Some("a")), "542 1 a");
        assert_eq!(
            legacy_text(Legacy::Crc, 3_015_617_425, 6, None),
            "3015617425 6"
        );
        assert_eq!(legacy_text(Legacy::Bsd, 36_979, 6, None), "36979     1");
    }

    #[test]
    fn length_is_refused_without_blake2b() {
        let opts = Options {
            method: Some(Method::Hash(Algo::Sha1)),
            length: Some(8),
            base64: false,
            raw: false,
            tagged: true,
            check: false,
            zero: false,
            ignore_missing: false,
            quiet: false,
            status: false,
            strict: false,
            warn: false,
        };
        assert_eq!(validate(&opts), Some(EXIT_FAIL));
    }
}
