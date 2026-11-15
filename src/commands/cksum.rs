use super::hashsum::{self, engine::Algorithm, Options};
use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error::usage,
};

const HELP:&str="Usage: cksum [OPTION]... [FILE]...\nCompute checksums, or verify tagged digest records. The default is POSIX CRC.\n\n  -a, --algorithm=TYPE select crc, crc32b, bsd, sysv, md5, sha1, sha224, sha256, sha384, sha512, blake2b\n      --base64        encode digests as base64\n  -c, --check         verify checksum records\n  -l, --length=BITS   BLAKE2b digest size (multiple of 8 up to 512; 0 means 512)\n      --raw           output one raw binary digest\n      --tag           select tagged digest records\n      --untagged      select untagged digest records\n  -z, --zero          terminate records with NUL\n      --ignore-missing skip missing checked files\n      --quiet         suppress successful check records\n      --status        report checks only by status\n      --strict        fail on malformed check records\n  -w, --warn          diagnose malformed check records\n      --help          display help\n      --version       display version\n\nExit status: 0 on success, 1 on errors.\n";

pub fn run(args: &[String]) -> i32 {
    let mut specs: Vec<_> = hashsum::specs()
        .into_iter()
        .filter(|s| !matches!(s.id, "binary" | "text"))
        .collect();
    specs.extend([
        OptSpec::value("algorithm", 'a'),
        OptSpec::flag("base64", '\0'),
        OptSpec::value("length", 'l'),
        OptSpec::flag("raw", '\0'),
        OptSpec::flag("untagged", '\0'),
        OptSpec::flag("debug", '\0'),
    ]);
    let items = match args::parse(args, &specs, true) {
        Ok(v) => v,
        Err(e) => return usage("cksum", e, 1),
    };
    let mut options = Options::new(Algorithm::Crc);
    let (mut raw, mut base64, mut tag, mut length, mut algorithm_given) =
        (false, false, None, None, false);
    for item in items {
        match item {
            Item::Opt(id @ ("help" | "version"), _) => {
                return args::help_version("cksum", &[format!("--{id}")], HELP, true).unwrap()
            }
            Item::Opt("algorithm", Some(a)) => match algorithm_argument(&a) {
                Some(a) => {
                    options.algorithm = a;
                    algorithm_given = true;
                }
                None => {
                    return usage(
                        "cksum",
                        format!("algorithm '{a}' is not supported by agentcmd"),
                        1,
                    )
                }
            },
            Item::Opt("length", Some(n)) => match hashsum::length(&n) {
                Ok(n) => length = Some(n),
                Err(e) => return usage("cksum", e, 1),
            },
            Item::Opt("raw", _) => raw = true,
            Item::Opt("base64", _) => base64 = true,
            Item::Opt("tag", _) => tag = Some(true),
            Item::Opt("untagged", _) => tag = Some(false),
            Item::Opt("debug", _) => {
                return usage("cksum", "option '--debug' is not supported by agentcmd", 1)
            }
            Item::Opt(id, _) => hashsum::apply(&mut options, id),
            Item::Operand(file) => options.files.push(file),
        }
    }
    if let Some(length) = length {
        if !matches!(options.algorithm, Algorithm::Blake2b(_)) {
            return usage("cksum", "--length requires --algorithm=blake2b", 1);
        }
        options.algorithm = Algorithm::Blake2b(length);
    }
    options.tag = tag.unwrap_or(options.algorithm.crypto() && !options.check);
    options.auto_algorithm = true;
    if raw && (options.files.len() > 1 || options.check || options.zero) {
        return usage(
            "cksum",
            "--raw requires a single input without --check or --zero",
            1,
        );
    }
    if options.check && algorithm_given && !options.algorithm.crypto() {
        return usage("cksum", "--check is not supported with this algorithm", 1);
    }
    if let Err(e) = hashsum::validate(&options) {
        return usage("cksum", e, 1);
    }
    hashsum::execute("cksum", &options, raw, base64)
}

fn algorithm_argument(name: &str) -> Option<Algorithm> {
    const NAMES: &[&str] = &[
        "sysv", "bsd", "crc", "crc32b", "md5", "sha1", "sha224", "sha256", "sha384", "sha512",
        "blake2b",
    ];
    if NAMES.contains(&name) {
        return Algorithm::named(name);
    }
    let mut matches = NAMES.iter().filter(|n| n.starts_with(name));
    let name = *matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Algorithm::named(name)
    }
}

pub fn run_sum(args: &[String]) -> i32 {
    const HELP:&str="Usage: sum [OPTION]... [FILE]...\nCompute BSD or System V checksums and block counts. No FILE or '-' reads stdin.\n\n  -r           use the BSD algorithm and 1024-byte blocks (default)\n  -s, --sysv   use the System V algorithm and 512-byte blocks\n      --help   display help\n      --version display version\n\nExit status: 0 on success, 1 on errors.\n";
    let specs = [
        OptSpec {
            id: "bsd",
            short: 'r',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec::flag("sysv", 's'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = match args::parse(args, &specs, true) {
        Ok(v) => v,
        Err(e) => return usage("sum", e, 1),
    };
    let mut options = Options::new(Algorithm::Bsd);
    for item in items {
        match item {
            Item::Opt(id @ ("help" | "version"), _) => {
                return args::help_version("sum", &[format!("--{id}")], HELP, true).unwrap()
            }
            Item::Opt("bsd", _) => options.algorithm = Algorithm::Bsd,
            Item::Opt("sysv", _) => options.algorithm = Algorithm::Sysv,
            Item::Operand(file) => options.files.push(file),
            _ => unreachable!(),
        }
    }
    hashsum::execute("sum", &options, false, false)
}

#[cfg(test)]
mod tests {
    use super::super::hashsum::engine::hash;
    use super::*;
    #[test]
    fn bsd_empty() {
        assert_eq!(hash(Algorithm::Bsd, &mut &b""[..]).unwrap().number(), 0);
    }
    #[test]
    fn bsd_rotation() {
        assert_eq!(
            hash(Algorithm::Bsd, &mut &b"ab"[..]).unwrap().number(),
            32914
        );
    }
    #[test]
    fn bsd_count() {
        assert_eq!(
            hash(Algorithm::Bsd, &mut &vec![0; 1025][..]).unwrap().size,
            1025
        );
    }
    #[test]
    fn sysv_empty() {
        assert_eq!(hash(Algorithm::Sysv, &mut &b""[..]).unwrap().number(), 0);
    }
    #[test]
    fn sysv_bytes() {
        assert_eq!(
            hash(Algorithm::Sysv, &mut &[255, 255][..])
                .unwrap()
                .number(),
            510
        );
    }
    #[test]
    fn sysv_carry() {
        assert_eq!(
            hash(Algorithm::Sysv, &mut &vec![255; 1000][..])
                .unwrap()
                .number(),
            58395
        );
    }
    #[test]
    fn crc_vector() {
        assert_eq!(
            hash(Algorithm::Crc, &mut &b"123456789"[..])
                .unwrap()
                .number(),
            930766865
        );
    }
    #[test]
    fn reflected_vector() {
        assert_eq!(
            hash(Algorithm::Crc32b, &mut &b"123456789"[..])
                .unwrap()
                .number(),
            0xcbf43926
        );
    }
    #[test]
    fn lengths() {
        assert_eq!(hashsum::length("256"), Ok(32));
        assert_eq!(hashsum::length("0"), Ok(64));
        for n in ["7", "513", "-1"] {
            assert!(hashsum::length(n).is_err());
        }
    }
    #[test]
    fn tags() {
        assert_eq!(Algorithm::Blake2b(32).tag(), "BLAKE2b-256");
    }
}
