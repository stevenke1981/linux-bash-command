mod check;
pub(crate) mod engine;
use crate::common::{
    args::{self, Item, OptSpec},
    error::{io_text, usage},
    io::{input, output},
    path::display_path,
};
use engine::Algorithm;
use std::{
    io::{self, Write},
    path::Path,
};

#[derive(Clone)]
pub(crate) struct Options {
    pub algorithm: Algorithm,
    pub files: Vec<String>,
    pub binary: bool,
    pub mode_given: bool,
    pub check: bool,
    pub tag: bool,
    pub zero: bool,
    pub ignore_missing: bool,
    pub quiet: bool,
    pub status: bool,
    pub strict: bool,
    pub warn: bool,
    pub length_given: bool,
    pub auto_algorithm: bool,
}
impl Options {
    pub fn new(algorithm: Algorithm) -> Self {
        Self {
            algorithm,
            files: Vec::new(),
            binary: false,
            mode_given: false,
            check: false,
            tag: false,
            zero: false,
            ignore_missing: false,
            quiet: false,
            status: false,
            strict: false,
            warn: false,
            length_given: false,
            auto_algorithm: false,
        }
    }
    pub fn inputs(&self) -> Vec<String> {
        if self.files.is_empty() {
            vec!["-".into()]
        } else {
            self.files.clone()
        }
    }
}
const HELP: &str = "Usage: {cmd} [OPTION]... [FILE]...\nCompute or verify file checksums; no FILE or '-' reads stdin.\n\n  -b, --binary         mark binary input\n  -c, --check          verify checksums from FILEs\n      --tag            produce BSD tagged records\n  -t, --text           mark text input (default)\n  -z, --zero           terminate with NUL; do not escape names\n      --ignore-missing ignore missing checked files\n      --quiet          suppress successful check records\n      --status         report checks only through exit status\n      --strict         reject malformed check records\n  -w, --warn           diagnose malformed check records\n      --help           display help\n      --version        display version\n\nExit status: 0 on success, 1 on errors or checksum differences.\n";
pub fn run_md5sum(a: &[String]) -> i32 {
    run("md5sum", Algorithm::Md5, a)
}
pub fn run_sha1sum(a: &[String]) -> i32 {
    run("sha1sum", Algorithm::Sha1, a)
}
pub fn run_sha224sum(a: &[String]) -> i32 {
    run("sha224sum", Algorithm::Sha224, a)
}
pub fn run_sha256sum(a: &[String]) -> i32 {
    run("sha256sum", Algorithm::Sha256, a)
}
pub fn run_sha384sum(a: &[String]) -> i32 {
    run("sha384sum", Algorithm::Sha384, a)
}
pub fn run_sha512sum(a: &[String]) -> i32 {
    run("sha512sum", Algorithm::Sha512, a)
}
pub fn run_b2sum(a: &[String]) -> i32 {
    run("b2sum", Algorithm::Blake2b(64), a)
}

fn run(command: &str, algorithm: Algorithm, args: &[String]) -> i32 {
    let mut help = HELP.replace("{cmd}", command);
    let mut specs = specs();
    if matches!(algorithm, Algorithm::Blake2b(_)) {
        help.push_str("  -l, --length=BITS    multiple of 8 up to 512; 0 selects 512\n");
        specs.push(OptSpec::value("length", 'l'));
    }
    let items = match args::parse(args, &specs, true) {
        Ok(v) => v,
        Err(e) => return usage(command, e, 1),
    };
    let mut options = Options::new(algorithm);
    let mut text = false;
    for item in items {
        match item {
            Item::Opt(id @ ("help" | "version"), _) => {
                return args::help_version(command, &[format!("--{id}")], &help, true).unwrap()
            }
            Item::Opt("length", Some(n)) => match length(&n) {
                Ok(n) => {
                    options.algorithm = Algorithm::Blake2b(n);
                    options.length_given = true;
                }
                Err(e) => return usage(command, e, 1),
            },
            Item::Opt("text", _) => {
                apply(&mut options, "text");
                text = true;
            }
            Item::Opt(id, _) => {
                apply(&mut options, id);
                if id == "binary" {
                    text = false;
                }
            }
            Item::Operand(file) => options.files.push(file),
        }
    }
    if options.check {
        options.length_given = false;
    }
    if options.tag && text {
        return usage(command, "--tag does not support --text", 1);
    }
    if let Err(e) = validate(&options) {
        return usage(command, e, 1);
    }
    execute(command, &options, false, false)
}
pub(crate) fn specs() -> Vec<OptSpec> {
    vec![
        OptSpec::flag("binary", 'b'),
        OptSpec::flag("check", 'c'),
        OptSpec::flag("tag", '\0'),
        OptSpec::flag("text", 't'),
        OptSpec::flag("zero", 'z'),
        OptSpec::flag("ignore-missing", '\0'),
        OptSpec::flag("quiet", '\0'),
        OptSpec::flag("status", '\0'),
        OptSpec::flag("strict", '\0'),
        OptSpec::flag("warn", 'w'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ]
}
pub(crate) fn apply(o: &mut Options, id: &str) {
    match id {
        "binary" | "text" => {
            o.binary = id == "binary";
            o.mode_given = true;
        }
        "check" => o.check = true,
        "tag" => o.tag = true,
        "zero" => o.zero = true,
        "ignore-missing" => o.ignore_missing = true,
        "quiet" => o.quiet = true,
        "status" => o.status = true,
        "strict" => o.strict = true,
        "warn" => o.warn = true,
        _ => unreachable!(),
    }
}
pub(crate) fn length(n: &str) -> Result<usize, String> {
    let bits = n
        .parse::<usize>()
        .map_err(|_| format!("invalid length: '{n}'"))?;
    if bits > 512 || bits % 8 != 0 {
        return Err(format!("invalid length: '{n}'"));
    }
    Ok(if bits == 0 { 64 } else { bits / 8 })
}
pub(crate) fn validate(o: &Options) -> Result<(), String> {
    if o.check && o.mode_given {
        return Err("--binary and --text options are meaningless when verifying checksums".into());
    }
    if o.check && (o.zero || o.tag) {
        return Err("--zero and --tag are not supported when verifying checksums".into());
    }
    if !o.check && (o.ignore_missing || o.quiet || o.status || o.strict || o.warn) {
        return Err("check options require --check".into());
    }
    Ok(())
}
pub(crate) fn execute(command: &str, o: &Options, raw: bool, base64: bool) -> i32 {
    let mut failed = false;
    let status = output(command, |out| {
        if o.check {
            failed = check::verify(command, o, out)?;
            return Ok(());
        }
        for name in o.inputs() {
            let result = input(&name).and_then(|mut reader| engine::hash(o.algorithm, &mut reader));
            match result {
                Ok(value) => {
                    if raw {
                        out.write_all(&value.bytes)?;
                    } else {
                        write_record(
                            out,
                            &display_path(Path::new(&name)),
                            &value,
                            o,
                            command == "cksum",
                            base64,
                        )?;
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{command}: {}: {}",
                        display_path(Path::new(&name)),
                        io_text(&e)
                    );
                    failed = true;
                }
            }
        }
        Ok(())
    });
    status.max(i32::from(failed))
}
pub(crate) fn write_record(
    out: &mut dyn Write,
    name: &str,
    value: &engine::Value,
    o: &Options,
    cksum: bool,
    base64: bool,
) -> io::Result<()> {
    if !o.algorithm.crypto() {
        if matches!(o.algorithm, Algorithm::Bsd | Algorithm::Sysv) {
            let blocks = value.size.div_ceil(if o.algorithm == Algorithm::Bsd {
                1024
            } else {
                512
            });
            if o.algorithm == Algorithm::Bsd {
                write!(out, "{:05} {blocks:5}", value.number())?;
            } else {
                write!(out, "{} {blocks}", value.number())?;
            }
        } else {
            write!(out, "{} {}", value.number(), value.size)?;
        }
        if !o.files.is_empty() {
            write!(out, " {name}")?;
        }
    } else {
        let digest = if base64 {
            super::basenc::encode_bytes(&value.bytes)
        } else {
            value.hex()
        };
        let escaped = !o.zero && (name.contains('\\') || name.contains('\n'));
        let name = if escaped {
            name.replace('\\', "\\\\").replace('\n', "\\n")
        } else {
            name.into()
        };
        if escaped {
            out.write_all(b"\\")?;
        }
        if o.tag {
            write!(out, "{} ({name}) = {digest}", o.algorithm.tag())?;
        } else {
            write!(
                out,
                "{digest} {}{name}",
                if o.binary && !cksum { '*' } else { ' ' }
            )?;
        }
    }
    out.write_all(if o.zero { b"\0" } else { b"\n" })
}
