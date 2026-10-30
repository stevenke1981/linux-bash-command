//! `base64`, `base32` and `basenc`: encode or decode data in the RFC 4648 alphabets,
//! base2, base16 and z85.

pub mod codec;

use std::io::{BufRead, Write};

use codec::{Decoder, Encoder, Encoding};

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::escape::quote_name;
use crate::common::io::{finish_output, is_broken_pipe, open_input, stdout_writer};

const BASE64_HELP: &str = "\
Usage: base64 [OPTION]... [FILE]
Base64 encode or decode FILE, or standard input, to standard output.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -d, --decode          decode data
  -i, --ignore-garbage  when decoding, ignore non-alphabet characters
  -w, --wrap=COLS       wrap encoded lines after COLS character (default 76).
                          Use 0 to disable line wrapping
      --help        display this help and exit
      --version     output version information and exit

The data are encoded as described for the base64 alphabet in RFC 4648.
When decoding, the input may contain newlines in addition to the bytes of
the formal base64 alphabet.  Use --ignore-garbage to attempt to recover
from any other non-alphabet bytes in the encoded stream.
";

const BASE32_HELP: &str = "\
Usage: base32 [OPTION]... [FILE]
Base32 encode or decode FILE, or standard input, to standard output.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -d, --decode          decode data
  -i, --ignore-garbage  when decoding, ignore non-alphabet characters
  -w, --wrap=COLS       wrap encoded lines after COLS character (default 76).
                          Use 0 to disable line wrapping
      --help        display this help and exit
      --version     output version information and exit

The data are encoded as described for the base32 alphabet in RFC 4648.
When decoding, the input may contain newlines in addition to the bytes of
the formal base32 alphabet.  Use --ignore-garbage to attempt to recover
from any other non-alphabet bytes in the encoded stream.
";

const BASENC_HELP: &str = "\
Usage: basenc [OPTION]... [FILE]
basenc encode or decode FILE, or standard input, to standard output.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
      --base64          same as 'base64' program (RFC4648 section 4)
      --base64url       file- and url-safe base64 (RFC4648 section 5)
      --base32          same as 'base32' program (RFC4648 section 6)
      --base32hex       extended hex alphabet base32 (RFC4648 section 7)
      --base16          hex encoding (RFC4648 section 8)
      --base2msbf       bit string with most significant bit (msb) first
      --base2lsbf       bit string with least significant bit (lsb) first
  -d, --decode          decode data
  -i, --ignore-garbage  when decoding, ignore non-alphabet characters
  -w, --wrap=COLS       wrap encoded lines after COLS character (default 76).
                          Use 0 to disable line wrapping
      --z85             ascii85-like encoding (ZeroMQ spec:32/Z85);
                        when encoding, input length must be a multiple of 4;
                        when decoding, input length must be a multiple of 5
      --help        display this help and exit
      --version     output version information and exit

When decoding, the input may contain newlines in addition to the bytes of
the formal alphabet.  Use --ignore-garbage to attempt to recover
from any other non-alphabet bytes in the encoded stream.
";

const COMMON_OPTIONS: &[OptSpec] = &[
    OptSpec::flag("decode", Some('d'), Some("decode")),
    OptSpec::flag("ignore", Some('i'), Some("ignore-garbage")),
    OptSpec::req("wrap", Some('w'), Some("wrap")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const BASENC_ENCODINGS: &[(&str, Encoding)] = &[
    ("base64", Encoding::Base64),
    ("base64url", Encoding::Base64Url),
    ("base32", Encoding::Base32),
    ("base32hex", Encoding::Base32Hex),
    ("base16", Encoding::Base16),
    ("base2msbf", Encoding::Base2Msbf),
    ("base2lsbf", Encoding::Base2Lsbf),
    ("z85", Encoding::Z85),
];

/// What one invocation does.
#[derive(Clone, Copy)]
struct Config {
    encoding: Encoding,
    decode: bool,
    ignore_garbage: bool,
    wrap: usize,
}

pub fn base64(args: &[String]) -> i32 {
    run("base64", Some(Encoding::Base64), BASE64_HELP, args)
}

pub fn base32(args: &[String]) -> i32 {
    run("base32", Some(Encoding::Base32), BASE32_HELP, args)
}

pub fn basenc(args: &[String]) -> i32 {
    run("basenc", None, BASENC_HELP, args)
}

/// Shared driver. `fixed` is the encoding for `base64` and `base32`; `basenc` passes
/// `None` and selects one with its encoding flags (the last one wins).
fn run(cmd: &str, fixed: Option<Encoding>, help: &str, args: &[String]) -> i32 {
    let mut specs = COMMON_OPTIONS.to_vec();
    if fixed.is_none() {
        for &(name, _) in BASENC_ENCODINGS {
            specs.push(OptSpec::flag(name, None, Some(name)));
        }
    }
    let items = match Parser::new(cmd, &specs).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(cmd, &error, EXIT_FAIL),
    };
    let mut encoding = fixed;
    let mut decode = false;
    let mut ignore_garbage = false;
    let mut wrap = 76usize;
    let mut files = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{help}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(cmd),
            Item::Opt { id: "decode", .. } => decode = true,
            Item::Opt { id: "ignore", .. } => ignore_garbage = true,
            Item::Opt { id: "wrap", value } => {
                let text = value.unwrap_or_default();
                match text.parse::<usize>() {
                    Ok(cols) => wrap = cols,
                    Err(_) => {
                        eprintln!("{cmd}: invalid wrap size: '{text}'");
                        return EXIT_FAIL;
                    }
                }
            }
            Item::Opt { id, .. } => {
                encoding = BASENC_ENCODINGS
                    .iter()
                    .find(|(name, _)| *name == id)
                    .map(|(_, enc)| *enc)
                    .or(encoding);
            }
            Item::Operand(file) => files.push(file),
        }
    }
    let Some(encoding) = encoding else {
        eprintln!("{cmd}: missing encoding type");
        eprintln!("Try '{cmd} --help' for more information.");
        return EXIT_FAIL;
    };
    if files.len() > 1 {
        let error = UsageError {
            message: format!("extra operand '{}'", files[1]),
        };
        return report_usage(cmd, &error, EXIT_FAIL);
    }
    let config = Config {
        encoding,
        decode,
        ignore_garbage,
        wrap,
    };
    let file = files.first().map(String::as_str).unwrap_or("-");
    process(cmd, config, file)
}

/// Streams `file` through the encoder or decoder to stdout.
fn process(cmd: &str, config: Config, file: &str) -> i32 {
    let mut reader = match open_input(file) {
        Ok(reader) => reader,
        Err(error) => {
            eprintln!("{cmd}: {}: {}", quote_name(file), io_error_text(&error));
            return EXIT_FAIL;
        }
    };
    let mut out = stdout_writer();
    let mut buf = Vec::with_capacity(64 * 1024);
    let mut encoder = Encoder::new(config.encoding, config.wrap);
    let mut decoder = Decoder::new(config.encoding, config.ignore_garbage);
    loop {
        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                eprintln!("{cmd}: {}", io_error_text(&error));
                return EXIT_FAIL;
            }
        };
        if chunk.is_empty() {
            break;
        }
        let used = chunk.len();
        let result = if config.decode {
            decoder.feed(chunk, &mut buf)
        } else {
            encoder.feed(chunk, &mut buf);
            Ok(())
        };
        if let Some(code) = write_out(cmd, &mut out, &mut buf) {
            return code;
        }
        reader.consume(used);
        if let Err(message) = result {
            eprintln!("{cmd}: {message}");
            return EXIT_FAIL;
        }
    }
    let result = if config.decode {
        decoder.finish(&mut buf)
    } else {
        encoder.finish(&mut buf)
    };
    if let Some(code) = write_out(cmd, &mut out, &mut buf) {
        return code;
    }
    if let Err(message) = result {
        eprintln!("{cmd}: {message}");
        return EXIT_FAIL;
    }
    finish_output(cmd, &mut out)
}

/// Writes and clears `buf`. Returns an exit status when writing failed.
fn write_out(cmd: &str, out: &mut impl Write, buf: &mut Vec<u8>) -> Option<i32> {
    if buf.is_empty() {
        return None;
    }
    let result = out.write_all(buf);
    buf.clear();
    match result {
        Ok(()) => None,
        Err(error) if is_broken_pipe(&error) => Some(EXIT_OK),
        Err(error) => {
            eprintln!("{cmd}: write error: {}", io_error_text(&error));
            Some(EXIT_FAIL)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_flags_map_to_encodings() {
        let found = BASENC_ENCODINGS
            .iter()
            .find(|(name, _)| *name == "z85")
            .map(|(_, enc)| *enc);
        assert_eq!(found, Some(Encoding::Z85));
        assert_eq!(BASENC_ENCODINGS.len(), 8);
    }

    #[test]
    fn help_texts_mention_their_program() {
        assert!(BASE64_HELP.starts_with("Usage: base64"));
        assert!(BASE32_HELP.starts_with("Usage: base32"));
        assert!(BASENC_HELP.contains("--z85"));
    }
}
