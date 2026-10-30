//! CLI tests for the checksum, base-encoding and system-information utilities:
//! md5sum … b2sum, cksum, sum, base64, base32, basenc, nproc, hostname, arch, uname,
//! uptime, whoami and free. Files live in a `Sandbox`; system values are checked by shape.

mod common;

use std::process::Output;

use common::{run, run_in, Sandbox};
use regex::Regex;

const HELLO: &[u8] = b"hello\n";

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn status(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn version_line(util: &str) -> String {
    format!("{util} (agentcmd) {}\n", env!("CARGO_PKG_VERSION"))
}

/// Digests of `hello\n` for each hash tool, as printed by GNU.
const DIGESTS: &[(&str, &str, &str)] = &[
    ("md5sum", "MD5", "b1946ac92492d2347c6235b4d2611184"),
    ("sha1sum", "SHA1", "f572d396fae9206628714fb2ce00f72e94f2258f"),
    (
        "sha224sum",
        "SHA224",
        "2d6d67d91d0badcdd06cbbba1fe11538a68a37ec9c2e26457ceff12b",
    ),
    (
        "sha256sum",
        "SHA256",
        "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03",
    ),
    (
        "sha384sum",
        "SHA384",
        "1d0f284efe3edea4b9ca3bd514fa134b17eae361ccc7a1eefeff801b9bd6604e01f21f6bf249ef030599f0c218f2ba8c",
    ),
    (
        "sha512sum",
        "SHA512",
        "e7c22b994c59d9cf2b48e549b1e24666636045930d3da7c1acb299d1c3b7f931f94aae41edda2c2b207a36e10f8bcb8d45223e54878f5b316e7ce3b6bc019629",
    ),
    (
        "b2sum",
        "BLAKE2b",
        "f60ce482e5cc1229f39d71313171a8d9f4ca3a87d066bf4b205effb528192a75f14f3271e2c1a90e1de53f275b4d4793eef2f5e31ea90d2ce29d2e481c36435f",
    ),
];

fn digest_of(tool: &str) -> &'static str {
    DIGESTS
        .iter()
        .find(|(name, _, _)| *name == tool)
        .expect("tool")
        .2
}

// ---------- md5sum … b2sum ----------

#[test]
fn hash_tools_digest_stdin_and_files() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    for (tool, _, hex) in DIGESTS {
        let from_stdin = run(tool, &[], HELLO);
        assert_eq!(status(&from_stdin), 0, "{tool}");
        assert_eq!(stdout(&from_stdin), format!("{hex}  -\n"), "{tool}");
        let from_file = run_in(sandbox.path(), tool, &["a"], b"");
        assert_eq!(stdout(&from_file), format!("{hex}  a\n"), "{tool}");
    }
}

#[test]
fn hash_tools_binary_marker_and_tag_format() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    for (tool, tag, hex) in DIGESTS {
        let binary = run_in(sandbox.path(), tool, &["-b", "a"], b"");
        assert_eq!(stdout(&binary), format!("{hex} *a\n"), "{tool}");
        let tagged = run_in(sandbox.path(), tool, &["--tag", "a"], b"");
        assert_eq!(stdout(&tagged), format!("{tag} (a) = {hex}\n"), "{tool}");
    }
}

#[test]
fn hash_tools_check_ok_mismatch_and_status() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let hex = digest_of("sha256sum");
    sandbox.write("good", format!("{hex}  a\n").as_bytes());
    sandbox.write("bad", format!("{}  a\n", "0".repeat(64)).as_bytes());
    let ok = run_in(sandbox.path(), "sha256sum", &["-c", "good"], b"");
    assert_eq!(status(&ok), 0);
    assert_eq!(stdout(&ok), "a: OK\n");
    let mismatch = run_in(sandbox.path(), "sha256sum", &["-c", "bad"], b"");
    assert_eq!(status(&mismatch), 1);
    assert_eq!(stdout(&mismatch), "a: FAILED\n");
    assert!(stderr(&mismatch).contains("WARNING: 1 computed checksum did NOT match"));
    let silent = run_in(sandbox.path(), "sha256sum", &["-c", "--status", "bad"], b"");
    assert_eq!(status(&silent), 1);
    assert_eq!(stdout(&silent), "");
}

#[test]
fn hash_tools_check_bsd_lines_and_quiet() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let hex = digest_of("md5sum");
    sandbox.write("sums", format!("MD5 (a) = {hex}\n").as_bytes());
    let output = run_in(sandbox.path(), "md5sum", &["-c", "--quiet", "sums"], b"");
    assert_eq!(status(&output), 0);
    assert_eq!(stdout(&output), "");
}

#[test]
fn hash_tools_check_ignore_missing_and_warn_and_strict() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let hex = digest_of("md5sum");
    sandbox.write(
        "sums",
        format!("{hex}  gone\n{hex}  a\njunk line\n").as_bytes(),
    );
    let lenient = run_in(
        sandbox.path(),
        "md5sum",
        &["-c", "--ignore-missing", "sums"],
        b"",
    );
    assert_eq!(status(&lenient), 0);
    assert_eq!(stdout(&lenient), "a: OK\n");
    assert!(stderr(&lenient).contains("improperly formatted"));
    let strict = run_in(sandbox.path(), "md5sum", &["-c", "--strict", "sums"], b"");
    assert_eq!(status(&strict), 1);
    let warned = run_in(sandbox.path(), "md5sum", &["-c", "-w", "sums"], b"");
    assert!(stderr(&warned).contains("sums: 3: improperly formatted MD5 checksum line"));
}

// Windows forbids newlines in file names, so the escaping of such names is Unix-only.
#[cfg(unix)]
#[test]
fn hash_tools_escape_backslash_and_newline_names() {
    let sandbox = Sandbox::new();
    sandbox.write("n\nl", HELLO);
    let hex = digest_of("md5sum");
    let output = run_in(sandbox.path(), "md5sum", &["n\nl"], b"");
    assert_eq!(stdout(&output), format!("\\{hex}  n\\nl\n"));
    let zero = run_in(sandbox.path(), "md5sum", &["-z", "n\nl"], b"");
    assert_eq!(stdout(&zero), format!("{hex}  n\nl\0"));
}

#[test]
fn hash_tools_reject_bad_usage_like_gnu() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let unknown = run_in(sandbox.path(), "md5sum", &["-x", "a"], b"");
    assert_eq!(status(&unknown), 1);
    assert!(stderr(&unknown).contains("invalid option -- 'x'"));
    assert!(stderr(&unknown).contains("Try 'md5sum --help' for more information."));
    let missing = run_in(sandbox.path(), "md5sum", &["nosuch"], b"");
    assert_eq!(status(&missing), 1);
    assert_eq!(
        stderr(&missing),
        "md5sum: nosuch: No such file or directory\n"
    );
    let length = run_in(sandbox.path(), "md5sum", &["-l", "128", "a"], b"");
    assert_eq!(status(&length), 1);
    let status_only = run_in(sandbox.path(), "md5sum", &["--status", "a"], b"");
    assert!(stderr(&status_only).contains("only when verifying checksums"));
    let tag_check = run_in(sandbox.path(), "md5sum", &["-c", "--tag", "a"], b"");
    assert!(stderr(&tag_check).contains("meaningless when verifying checksums"));
    let directory = run_in(sandbox.path(), "md5sum", &["."], b"");
    assert_eq!(status(&directory), 1);
}

#[test]
fn hash_tools_help_and_version() {
    for (tool, _, _) in DIGESTS {
        let help = run(tool, &["--help"], b"");
        assert_eq!(status(&help), 0, "{tool}");
        assert!(stdout(&help).starts_with(&format!("Usage: {tool} [OPTION]... [FILE]...")));
        let version = run(tool, &["--version"], b"");
        assert_eq!(stdout(&version), version_line(tool), "{tool}");
    }
}

#[test]
fn b2sum_length_option_and_messages() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let short = run_in(sandbox.path(), "b2sum", &["-l", "256", "a"], b"");
    assert_eq!(
        stdout(&short),
        "93becc6e9882211c3ec3708c95bcd69baab7bb59c7f4bc84ce637b88a534b783  a\n"
    );
    let zero = run_in(sandbox.path(), "b2sum", &["-l", "0", "a"], b"");
    assert_eq!(stdout(&zero), format!("{}  a\n", digest_of("b2sum")));
    let tagged = run_in(sandbox.path(), "b2sum", &["--tag", "-l", "256", "a"], b"");
    assert!(stdout(&tagged).starts_with("BLAKE2b-256 (a) = "));
    let not_multiple = run_in(sandbox.path(), "b2sum", &["-l", "7", "a"], b"");
    assert_eq!(status(&not_multiple), 1);
    assert!(stderr(&not_multiple).contains("length is not a multiple of 8"));
    let too_big = run_in(sandbox.path(), "b2sum", &["-l", "1024", "a"], b"");
    assert!(stderr(&too_big).contains("maximum digest length for 'BLAKE2b' is 512 bits"));
    let not_number = run_in(sandbox.path(), "b2sum", &["-l", "x", "a"], b"");
    assert!(stderr(&not_number).contains("invalid length: 'x'"));
}

// ---------- cksum / sum ----------

#[test]
fn cksum_default_crc_stdin_and_file() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    assert_eq!(stdout(&run("cksum", &[], HELLO)), "3015617425 6\n");
    assert_eq!(
        stdout(&run_in(sandbox.path(), "cksum", &["a"], b"")),
        "3015617425 6 a\n"
    );
    assert_eq!(stdout(&run("cksum", &["-"], HELLO)), "3015617425 6 -\n");
}

#[test]
fn cksum_algorithms_and_tag_styles() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let sha256 = digest_of("sha256sum");
    assert_eq!(
        stdout(&run_in(
            sandbox.path(),
            "cksum",
            &["-a", "sha256", "a"],
            b""
        )),
        format!("SHA256 (a) = {sha256}\n")
    );
    assert_eq!(
        stdout(&run_in(
            sandbox.path(),
            "cksum",
            &["-a", "sha256", "--untagged", "a"],
            b""
        )),
        format!("{sha256}  a\n")
    );
    assert_eq!(
        stdout(&run_in(
            sandbox.path(),
            "cksum",
            &["-a", "crc32b", "a"],
            b""
        )),
        "CRC32B (a) = 363a3020\n"
    );
    assert_eq!(
        stdout(&run_in(sandbox.path(), "cksum", &["-a", "bsd", "a"], b"")),
        "36979     1 a\n"
    );
    assert_eq!(
        stdout(&run_in(sandbox.path(), "cksum", &["-a", "sysv", "a"], b"")),
        "542 1 a\n"
    );
    let blake = stdout(&run_in(
        sandbox.path(),
        "cksum",
        &["-a", "blake2b", "-l", "256", "a"],
        b"",
    ));
    assert_eq!(
        blake,
        format!(
            "BLAKE2b-256 (a) = {}\n",
            "93becc6e9882211c3ec3708c95bcd69baab7bb59c7f4bc84ce637b88a534b783"
        )
    );
}

#[test]
fn cksum_base64_raw_and_zero() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let b64 = run_in(
        sandbox.path(),
        "cksum",
        &["-a", "sha256", "--base64", "a"],
        b"",
    );
    assert_eq!(
        stdout(&b64),
        "SHA256 (a) = WJG1tSLV3whtD/CxEPvZ0hu0/HFjrzTQgoai6Eb2vgM=\n"
    );
    let raw = run_in(sandbox.path(), "cksum", &["-a", "sha1", "--raw", "a"], b"");
    assert_eq!(raw.stdout.len(), 20);
    let crc_raw = run_in(sandbox.path(), "cksum", &["--raw", "a"], b"");
    assert_eq!(crc_raw.stdout, 3_015_617_425u32.to_be_bytes());
    let zero = run_in(sandbox.path(), "cksum", &["-a", "md5", "-z", "a"], b"");
    assert!(zero.stdout.ends_with(b"\0"));
    assert!(!zero.stdout.contains(&b'\n'));
    let conflict = run_in(sandbox.path(), "cksum", &["--base64", "--raw", "a"], b"");
    assert_eq!(status(&conflict), 1);
    assert!(stderr(&conflict).contains("mutually exclusive"));
}

#[test]
fn cksum_check_round_trip_and_refusals() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let listing = run_in(sandbox.path(), "cksum", &["-a", "sha512", "a"], b"");
    sandbox.write("sums", &listing.stdout);
    let ok = run_in(sandbox.path(), "cksum", &["-c", "sums"], b"");
    assert_eq!(status(&ok), 0);
    assert_eq!(stdout(&ok), "a: OK\n");
    let legacy = run_in(sandbox.path(), "cksum", &["-a", "crc", "-c", "sums"], b"");
    assert_eq!(status(&legacy), 1);
    assert!(stderr(&legacy).contains("--check is not supported with --algorithm={bsd,sysv,crc}"));
    // Without -a only tagged lines are accepted; an untagged digest line is improper.
    sandbox.write(
        "plain",
        format!("{}  a\n", digest_of("sha512sum")).as_bytes(),
    );
    let untagged = run_in(sandbox.path(), "cksum", &["-c", "plain"], b"");
    assert_eq!(status(&untagged), 1);
    assert!(stderr(&untagged).contains("no properly formatted checksum lines found"));
}

#[test]
fn cksum_length_and_algorithm_errors() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    let length = run_in(
        sandbox.path(),
        "cksum",
        &["-a", "sha1", "-l", "8", "a"],
        b"",
    );
    assert_eq!(status(&length), 1);
    assert!(stderr(&length).contains("--length is only supported with --algorithm=blake2b"));
    let bogus = run_in(sandbox.path(), "cksum", &["-a", "bogus", "a"], b"");
    assert_eq!(status(&bogus), 1);
    assert!(stderr(&bogus).contains("invalid argument 'bogus' for '--algorithm'"));
    assert!(stderr(&bogus).contains("Valid arguments are:"));
    let missing = run_in(sandbox.path(), "cksum", &["nosuch"], b"");
    assert_eq!(
        stderr(&missing),
        "cksum: nosuch: No such file or directory\n"
    );
    assert_eq!(status(&missing), 1);
    assert_eq!(
        stdout(&run("cksum", &["--version"], b"")),
        version_line("cksum")
    );
}

#[test]
fn sum_bsd_sysv_and_last_option_wins() {
    let sandbox = Sandbox::new();
    sandbox.write("a", HELLO);
    assert_eq!(
        stdout(&run_in(sandbox.path(), "sum", &["a"], b"")),
        "36979     1 a\n"
    );
    assert_eq!(
        stdout(&run_in(sandbox.path(), "sum", &["-s", "a"], b"")),
        "542 1 a\n"
    );
    assert_eq!(
        stdout(&run_in(sandbox.path(), "sum", &["-s", "-r", "a"], b"")),
        "36979     1 a\n"
    );
    assert_eq!(stdout(&run("sum", &[], HELLO)), "36979     1\n");
    let missing = run_in(sandbox.path(), "sum", &["a", "nosuch"], b"");
    assert_eq!(stdout(&missing), "36979     1 a\n");
    assert_eq!(status(&missing), 1);
    let unknown = run("sum", &["-z"], b"");
    assert_eq!(status(&unknown), 1);
    assert_eq!(
        stdout(&run("sum", &["--help"], b"")).lines().next(),
        Some("Usage: sum [OPTION]... [FILE]...")
    );
    assert_eq!(
        stdout(&run("sum", &["--version"], b"")),
        version_line("sum")
    );
}

// ---------- base64 / base32 / basenc ----------

#[test]
fn base64_encodes_with_default_and_zero_wrap() {
    assert_eq!(
        stdout(&run("base64", &[], b"hello world")),
        "aGVsbG8gd29ybGQ=\n"
    );
    assert_eq!(
        stdout(&run("base64", &["-w", "0"], b"hello world")),
        "aGVsbG8gd29ybGQ="
    );
    assert_eq!(
        stdout(&run("base64", &["-w4"], b"hello world")),
        "aGVs\nbG8g\nd29y\nbGQ=\n"
    );
    assert_eq!(stdout(&run("base64", &[], b"")), "");
    let long = vec![b'x'; 80];
    let wrapped = stdout(&run("base64", &[], &long));
    assert_eq!(wrapped.lines().count(), 2);
    assert_eq!(wrapped.lines().next().map(str::len), Some(76));
}

#[test]
fn base64_decodes_ignoring_newlines_and_reports_garbage() {
    assert_eq!(
        stdout(&run("base64", &["-d"], b"aGVs\nbG8g\nd29y\nbGQ=\n")),
        "hello world"
    );
    let garbage = run("base64", &["-d"], b"aGVs bG8=\n");
    assert_eq!(stdout(&garbage), "hel");
    assert_eq!(status(&garbage), 1);
    assert_eq!(stderr(&garbage), "base64: invalid input\n");
    let ignored = run("base64", &["-d", "-i"], b"aGVs bG8=\n");
    assert_eq!(stdout(&ignored), "hello");
    assert_eq!(status(&ignored), 0);
    let truncated = run("base64", &["--decode"], b"aGVsbG8");
    assert_eq!(stdout(&truncated), "hello");
    assert_eq!(status(&truncated), 1);
}

#[test]
fn base64_usage_errors_and_files() {
    let sandbox = Sandbox::new();
    sandbox.write("data", b"hi");
    assert_eq!(
        stdout(&run_in(sandbox.path(), "base64", &["data"], b"")),
        "aGk=\n"
    );
    let wrap = run("base64", &["-w", "x"], b"");
    assert_eq!(status(&wrap), 1);
    assert_eq!(stderr(&wrap), "base64: invalid wrap size: 'x'\n");
    let extra = run_in(sandbox.path(), "base64", &["data", "data"], b"");
    assert_eq!(status(&extra), 1);
    assert!(stderr(&extra).contains("extra operand 'data'"));
    let missing = run_in(sandbox.path(), "base64", &["nosuch"], b"");
    assert_eq!(status(&missing), 1);
    assert_eq!(
        stdout(&run("base64", &["--help"], b"")).lines().next(),
        Some("Usage: base64 [OPTION]... [FILE]")
    );
    assert_eq!(
        stdout(&run("base64", &["--version"], b"")),
        version_line("base64")
    );
}

#[test]
fn base32_round_trip_and_bad_padding() {
    assert_eq!(stdout(&run("base32", &[], b"hi")), "NBUQ====\n");
    assert_eq!(
        stdout(&run("base32", &["-d"], b"MZXW6YTBOI======\n")),
        "foobar"
    );
    assert_eq!(stdout(&run("base32", &["-w", "0"], b"hi")), "NBUQ====");
    let bad = run("base32", &["-d"], b"NBUQ=");
    assert_eq!(status(&bad), 1);
    assert_eq!(stdout(&bad), "");
    assert_eq!(stderr(&bad), "base32: invalid input\n");
    let ignored = run("base32", &["-d", "-i"], b"NBUQ 1111====\n");
    assert_eq!(status(&ignored), 0);
    assert_eq!(
        stdout(&run("base32", &["--version"], b"")),
        version_line("base32")
    );
}

#[test]
fn basenc_encodings_round_trip() {
    let cases: &[(&str, &[u8], &str)] = &[
        ("--base64url", &[0xfb, 0xff], "-_8=\n"),
        ("--base32hex", b"hi", "D1KG====\n"),
        ("--base16", b"hi", "6869\n"),
        ("--base2msbf", b"h", "01101000\n"),
        ("--base2lsbf", b"h", "00010110\n"),
    ];
    for (flag, input, expected) in cases {
        let encoded = run("basenc", &[flag], input);
        assert_eq!(stdout(&encoded), *expected, "{flag}");
        let decoded = run("basenc", &[flag, "-d"], expected.trim_end().as_bytes());
        assert_eq!(decoded.stdout, *input, "{flag} decode");
    }
}

#[test]
fn basenc_z85_and_length_rules() {
    let encoded = run(
        "basenc",
        &["--z85"],
        &[0x86, 0x4f, 0xd2, 0x6f, 0xb5, 0x59, 0xf7, 0x5b],
    );
    assert_eq!(stdout(&encoded), "HelloWorld\n");
    let decoded = run("basenc", &["--z85", "-d"], b"HelloWorld\n");
    assert_eq!(
        decoded.stdout,
        [0x86, 0x4f, 0xd2, 0x6f, 0xb5, 0x59, 0xf7, 0x5b]
    );
    let bad_encode = run("basenc", &["--z85"], b"hi");
    assert_eq!(status(&bad_encode), 1);
    assert!(stderr(&bad_encode).contains("length must be multiple of 4 characters"));
    let bad_decode = run("basenc", &["--z85", "-d"], b"Hello0");
    assert_eq!(status(&bad_decode), 1);
}

#[test]
fn basenc_requires_encoding_and_last_one_wins() {
    let missing = run("basenc", &[], b"x");
    assert_eq!(status(&missing), 1);
    assert_eq!(
        stderr(&missing).lines().next(),
        Some("basenc: missing encoding type")
    );
    assert_eq!(
        stdout(&run("basenc", &["--base16", "--base32"], b"hi")),
        "NBUQ====\n"
    );
    assert_eq!(
        stdout(&run("basenc", &["--base2msbf", "-w", "5"], b"hi")),
        "01101\n00001\n10100\n1\n"
    );
    assert!(stdout(&run("basenc", &["--help"], b"")).contains("--z85"));
}

// ---------- nproc / arch / whoami / hostname ----------

#[test]
fn nproc_counts_and_honours_environment() {
    let total: usize = stdout(&run("nproc", &[], b""))
        .trim()
        .parse()
        .expect("number");
    assert!(total >= 1);
    let all: usize = stdout(&run("nproc", &["--all"], b""))
        .trim()
        .parse()
        .expect("number");
    assert!(all >= total);
    let minus_one: usize = stdout(&run("nproc", &["--ignore=1"], b""))
        .trim()
        .parse()
        .expect("number");
    assert_eq!(minus_one, total.saturating_sub(1).max(1));
    let bad = run("nproc", &["--ignore=abc"], b"");
    assert_eq!(status(&bad), 1);
    assert_eq!(stderr(&bad), "nproc: invalid number: 'abc'\n");
    let extra = run("nproc", &["x"], b"");
    assert_eq!(status(&extra), 1);
    assert_eq!(
        stdout(&run("nproc", &["--version"], b"")),
        version_line("nproc")
    );
}

#[test]
fn nproc_omp_variables() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .args(["nproc"])
        .env("OMP_NUM_THREADS", "2,3")
        .output()
        .expect("run nproc");
    assert_eq!(stdout(&output), "2\n");
    let limited = std::process::Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .args(["nproc"])
        .env("OMP_THREAD_LIMIT", "1")
        .output()
        .expect("run nproc");
    assert_eq!(stdout(&limited), "1\n");
}

#[test]
fn arch_prints_a_machine_name() {
    let text = stdout(&run("arch", &[], b""));
    assert!(
        !text.trim().is_empty() && !text.trim().contains(' '),
        "{text:?}"
    );
    assert!(text.ends_with('\n'));
    let extra = run("arch", &["x"], b"");
    assert_eq!(status(&extra), 1);
    assert_eq!(
        stdout(&run("arch", &["--version"], b"")),
        version_line("arch")
    );
    assert!(stdout(&run("arch", &["--help"], b"")).starts_with("Usage: arch"));
}

#[test]
fn whoami_prints_the_effective_user() {
    let name = stdout(&run("whoami", &[], b""));
    assert!(!name.trim().is_empty());
    assert_eq!(name.matches('\n').count(), 1);
    let extra = run("whoami", &["x"], b"");
    assert_eq!(status(&extra), 1);
    assert_eq!(
        stdout(&run("whoami", &["--version"], b"")),
        version_line("whoami")
    );
}

#[test]
fn hostname_short_and_refused_operations() {
    let full = stdout(&run("hostname", &[], b"")).trim().to_owned();
    assert!(!full.is_empty());
    let short = stdout(&run("hostname", &["-s"], b"")).trim().to_owned();
    assert_eq!(short, full.split('.').next().unwrap_or(&full));
    let fqdn = stdout(&run("hostname", &["-f"], b"")).trim().to_owned();
    assert!(fqdn.starts_with(&short));
    let set = run("hostname", &["new-name"], b"");
    assert_eq!(status(&set), 1);
    assert!(stderr(&set).contains("not supported by agentcmd"));
    let domain = run("hostname", &["-d"], b"");
    assert_eq!(status(&domain), 1);
    assert!(stderr(&domain).contains("option '-d' is not supported by agentcmd"));
    let unknown = run("hostname", &["-q"], b"");
    assert_eq!(status(&unknown), 1);
    assert_eq!(
        stdout(&run("hostname", &["--version"], b"")),
        version_line("hostname")
    );
}

// ---------- uname / uptime / free ----------

#[test]
fn uname_fields_have_expected_shapes() {
    let kernel = stdout(&run("uname", &["-s"], b"")).trim().to_owned();
    if cfg!(target_os = "linux") {
        assert_eq!(kernel, "Linux");
        assert_eq!(stdout(&run("uname", &["-o"], b"")), "GNU/Linux\n");
    }
    assert_eq!(stdout(&run("uname", &[], b"")), format!("{kernel}\n"));
    let machine = stdout(&run("uname", &["-m"], b"")).trim().to_owned();
    assert!(!machine.is_empty());
    let all = stdout(&run("uname", &["-a"], b""));
    assert!(all.starts_with(&format!("{kernel} ")));
    assert!(!all.ends_with(' '));
    let clustered = stdout(&run("uname", &["-sm"], b""));
    assert_eq!(clustered, format!("{kernel} {machine}\n"));
    let release = stdout(&run("uname", &["-r"], b""));
    assert!(release.len() > 1 && release.ends_with('\n'));
    let unknown = run("uname", &["-x"], b"");
    assert_eq!(status(&unknown), 1);
    assert_eq!(
        stdout(&run("uname", &["--version"], b"")),
        version_line("uname")
    );
}

#[test]
fn uptime_status_line_and_since_shapes() {
    let line = stdout(&run("uptime", &[], b""));
    let re = Regex::new(
        r"^ \d{2}:\d{2}:\d{2} up .+ load average: \d+\.\d{2}, \d+\.\d{2}, \d+\.\d{2}\n$",
    );
    let re = re.expect("pattern");
    if cfg!(target_os = "linux") {
        assert!(re.is_match(&line), "{line:?}");
    } else {
        assert!(line.contains(" up "), "{line:?}");
    }
    let since = stdout(&run("uptime", &["-s"], b""));
    assert!(
        Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\n$")
            .expect("pattern")
            .is_match(&since),
        "{since:?}"
    );
    let pretty = stdout(&run("uptime", &["-p"], b""));
    assert!(pretty.starts_with("up "), "{pretty:?}");
    let unknown = run("uptime", &["-x"], b"");
    assert_eq!(status(&unknown), 1);
    let extra = run("uptime", &["x"], b"");
    assert_eq!(status(&extra), 1);
}

#[test]
fn free_table_layout_and_options() {
    let table = stdout(&run("free", &[], b""));
    let mut lines = table.lines();
    assert_eq!(
        lines.next(),
        Some("               total        used        free      shared  buff/cache   available")
    );
    assert!(lines.next().is_some_and(|l| l.starts_with("Mem:")));
    assert!(lines.next().is_some_and(|l| l.starts_with("Swap:")));
    let bytes = stdout(&run("free", &["-b"], b""));
    assert!(bytes.lines().nth(1).is_some_and(|l| l
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .parse::<u64>()
        .is_ok()));
    let human = stdout(&run("free", &["-h"], b""));
    assert!(human
        .lines()
        .nth(1)
        .is_some_and(|l| l.contains('i') || l.contains('B')));
    let totals = stdout(&run("free", &["-t"], b""));
    assert!(totals.lines().any(|l| l.starts_with("Total:")));
    let wide = stdout(&run("free", &["-w"], b""));
    assert!(wide
        .lines()
        .next()
        .is_some_and(|l| l.contains("buffers") && l.contains("cache")));
    let once = run("free", &["-s", "1", "-c", "1"], b"");
    assert_eq!(status(&once), 0);
    let zero = run("free", &["-c", "0"], b"");
    assert_eq!(status(&zero), 1);
    let bad_seconds = run("free", &["-s", "x"], b"");
    assert_eq!(status(&bad_seconds), 1);
    let committed = run("free", &["-v"], b"");
    assert_eq!(status(&committed), 1);
    assert!(stderr(&committed).contains("option '-v' is not supported by agentcmd"));
    assert_eq!(
        stdout(&run("free", &["--version"], b"")),
        version_line("free")
    );
}

#[test]
fn checksum_commands_accept_files_relative_to_sandbox() {
    let sandbox = Sandbox::new();
    sandbox.mkdir("dir");
    sandbox.write("dir/inner", HELLO);
    let output = run_in(sandbox.path(), "sha256sum", &["dir/inner"], b"");
    assert!(stdout(&output).ends_with("  dir/inner\n"));
}
