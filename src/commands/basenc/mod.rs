mod codec;
use crate::common::{
    args::{self, Item, OptSpec},
    error::{io_text, usage},
    io::{input, output},
};
use codec::Encoding;
use std::io;

pub fn run_base64(args: &[String]) -> i32 {
    run("base64", Some(Encoding::Base64), args)
}
pub fn run_base32(args: &[String]) -> i32 {
    run("base32", Some(Encoding::Base32), args)
}
pub fn run_basenc(args: &[String]) -> i32 {
    run("basenc", None, args)
}

fn run(command: &str, default: Option<Encoding>, args: &[String]) -> i32 {
    let mut help=format!("Usage: {command} [OPTION]... [FILE]\nEncode or decode byte streams. No FILE or '-' reads stdin.\n\n  -d, --decode          decode instead of encode\n  -i, --ignore-garbage  skip non-alphabet bytes while decoding\n  -w, --wrap=COLS       wrap encoded lines (default 76; 0 disables)\n      --help            display help\n      --version         display version\n\nExit status: 0 on success, 1 on errors or invalid input.\n");
    let mut specs = vec![
        OptSpec::flag("decode", 'd'),
        OptSpec::flag("ignore-garbage", 'i'),
        OptSpec::value("wrap", 'w'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    if default.is_none() {
        help.push_str("Select one encoding:\n  --base64 --base64url --base32 --base32hex --base16 --base2msbf --base2lsbf --z85\n");
        specs.extend(Encoding::NAMES.iter().map(|name| OptSpec::flag(name, '\0')));
    }
    let items = match args::parse(args, &specs, true) {
        Ok(v) => v,
        Err(e) => return usage(command, e, 1),
    };
    let (mut encoding, mut decode, mut garbage, mut wrap, mut file) =
        (default, false, false, 76, None);
    for item in items {
        match item {
            Item::Opt(id @ ("help" | "version"), _) => {
                return args::help_version(command, &[format!("--{id}")], &help, true).unwrap()
            }
            Item::Opt("decode", _) => decode = true,
            Item::Opt("ignore-garbage", _) => garbage = true,
            Item::Opt("wrap", Some(value)) => match value.parse::<usize>() {
                Ok(n) => wrap = n,
                Err(_) => return usage(command, format!("invalid wrap size: '{value}'"), 1),
            },
            Item::Opt(id, _) => {
                let selected = Encoding::named(id).unwrap();
                encoding = Some(selected);
            }
            Item::Operand(name) => {
                if file.is_some() {
                    return usage(command, format!("extra operand '{name}'"), 1);
                }
                file = Some(name);
            }
        }
    }
    let Some(encoding) = encoding else {
        return usage(command, "missing encoding type", 1);
    };
    let file = file.unwrap_or_else(|| "-".into());
    let mut reader = match input(&file) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{command}: {file}: {}", io_text(&e));
            return 1;
        }
    };
    let mut invalid = false;
    let status = output(command, |out| {
        let result = if decode {
            codec::decode(&mut reader, out, encoding, garbage)
        } else {
            codec::encode(&mut reader, out, encoding, wrap)
        };
        match result {
            Err(e) if codec::read_error(&e).is_some() => {
                eprintln!(
                    "{command}: read error: {}",
                    io_text(codec::read_error(&e).unwrap())
                );
                invalid = true;
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                eprintln!("{command}: invalid input");
                invalid = true;
                Ok(())
            }
            result => result,
        }
    });
    status.max(i32::from(invalid))
}
pub(crate) fn encode_bytes(bytes: &[u8]) -> String {
    let mut out = Vec::new();
    codec::encode(&mut &bytes[..], &mut out, Encoding::Base64, 0).unwrap();
    String::from_utf8(out).unwrap()
}
pub(crate) fn decode_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    codec::decode(&mut &bytes[..], &mut out, Encoding::Base64, false).ok()?;
    Some(out)
}
