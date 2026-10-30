//! CLI tests for the Phase 5 archive and compression commands: gzip, gunzip, zcat and tar.
//! Every test works inside a `Sandbox`; nothing is extracted outside it.

mod common;

use std::fs;
use std::process::Output;

use common::{run, run_in, Sandbox};
use tar::{Builder, EntryType, Header};

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

/// Deterministic, moderately compressible data.
fn sample(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i % 251) as u8 ^ ((i / 1000) as u8))
        .collect()
}

fn gz_bytes(sb: &Sandbox, name: &str) -> Vec<u8> {
    sb.read(name)
}

// ---------------------------------------------------------------- gzip

#[test]
fn gzip_help_and_version_exit_zero() {
    let help = run("gzip", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help).starts_with("Usage: gzip [OPTION]"));
    let version = run("gzip", &["--version"], b"");
    assert_eq!(code(&version), 0);
    assert!(text(&version).starts_with("gzip (agentcmd) "));
    let gunzip_help = run("gunzip", &["--help"], b"");
    assert!(text(&gunzip_help).starts_with("Usage: gunzip "));
}

#[test]
fn gzip_unknown_option_is_usage_error() {
    let output = run("gzip", &["--bogus"], b"");
    assert_eq!(code(&output), 1);
    assert!(err_text(&output).contains("unrecognized option '--bogus'"));
    assert!(err_text(&output).contains("Try 'gzip --help' for more information."));
}

#[test]
fn gzip_file_round_trip_removes_then_restores_the_original() {
    let sb = Sandbox::new();
    let data = sample(200_000);
    sb.write("data.bin", &data);
    let packed = run_in(sb.path(), "gzip", &["data.bin"], b"");
    assert_eq!(code(&packed), 0, "{}", err_text(&packed));
    assert!(!sb.path().join("data.bin").exists());
    assert!(sb.path().join("data.bin.gz").exists());
    let unpacked = run_in(sb.path(), "gunzip", &["data.bin.gz"], b"");
    assert_eq!(code(&unpacked), 0);
    assert_eq!(sb.read("data.bin"), data);
    assert!(!sb.path().join("data.bin.gz").exists());
}

#[test]
fn gzip_keep_and_stdout_leave_the_input_alone() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"keep me\n");
    let kept = run_in(sb.path(), "gzip", &["-k", "a.txt"], b"");
    assert_eq!(code(&kept), 0);
    assert_eq!(sb.read("a.txt"), b"keep me\n");
    let to_stdout = run_in(sb.path(), "gzip", &["-c", "a.txt"], b"");
    assert_eq!(code(&to_stdout), 0);
    assert_eq!(&to_stdout.stdout[..2], &[0x1f, 0x8b]);
    assert_eq!(sb.read("a.txt"), b"keep me\n");
    let back = run("gzip", &["-dc"], &to_stdout.stdout);
    assert_eq!(back.stdout, b"keep me\n");
}

#[test]
fn stdin_to_stdout_round_trip_and_all_levels() {
    let data = sample(50_000);
    for level in ["-1", "-6", "-9", "--best", "--fast"] {
        let packed = run("gzip", &["-c", level], &data);
        assert_eq!(code(&packed), 0, "level {level}");
        let unpacked = run("gzip", &["-d"], &packed.stdout);
        assert_eq!(unpacked.stdout, data, "level {level}");
    }
}

#[test]
fn concatenated_members_decode_as_one_stream() {
    let first = run("gzip", &["-c"], b"first half, ");
    let second = run("gzip", &["-c"], b"second half\n");
    let mut joined = first.stdout.clone();
    joined.extend_from_slice(&second.stdout);
    let output = run("zcat", &[], &joined);
    assert_eq!(code(&output), 0);
    assert_eq!(output.stdout, b"first half, second half\n");
}

#[test]
fn name_is_stored_and_restored_with_capital_n() {
    let sb = Sandbox::new();
    sb.write("original.txt", b"payload\n");
    run_in(sb.path(), "gzip", &["-k", "original.txt"], b"");
    fs::rename(
        sb.path().join("original.txt.gz"),
        sb.path().join("renamed.gz"),
    )
    .unwrap();
    fs::remove_file(sb.path().join("original.txt")).unwrap();
    let output = run_in(sb.path(), "gzip", &["-dN", "renamed.gz"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(sb.read("original.txt"), b"payload\n");
}

#[test]
fn no_name_flag_omits_the_stored_name() {
    let sb = Sandbox::new();
    sb.write("named.txt", b"x\n");
    run_in(sb.path(), "gzip", &["-n", "named.txt"], b"");
    let packed = gz_bytes(&sb, "named.txt.gz");
    assert_eq!(packed[3] & 0x08, 0, "FNAME flag must be clear");
    assert_eq!(&packed[4..8], &[0, 0, 0, 0], "mtime must be zero");
}

#[test]
fn existing_output_is_not_overwritten_without_force() {
    let sb = Sandbox::new();
    sb.write("f.txt", b"new data\n");
    sb.write("f.txt.gz", b"old\n");
    let refused = run_in(sb.path(), "gzip", &["f.txt"], b"");
    assert_eq!(code(&refused), 2);
    assert!(err_text(&refused).contains("already exists;\tnot overwritten"));
    assert_eq!(sb.read("f.txt"), b"new data\n");
    assert_eq!(sb.read("f.txt.gz"), b"old\n");
    let forced = run_in(sb.path(), "gzip", &["-f", "f.txt"], b"");
    assert_eq!(code(&forced), 0);
    assert_eq!(&sb.read("f.txt.gz")[..2], &[0x1f, 0x8b]);
}

#[test]
fn directories_need_recursion_and_missing_files_fail() {
    let sb = Sandbox::new();
    sb.write("dir/x.txt", b"x\n");
    let skipped = run_in(sb.path(), "gzip", &["dir"], b"");
    assert_eq!(code(&skipped), 2);
    assert!(err_text(&skipped).contains("dir is a directory -- ignored"));
    assert!(sb.path().join("dir/x.txt").exists());
    let recursive = run_in(sb.path(), "gzip", &["-r", "dir"], b"");
    assert_eq!(code(&recursive), 0);
    assert!(sb.path().join("dir/x.txt.gz").exists());
    let missing = run_in(sb.path(), "gzip", &["nosuch.txt"], b"");
    assert_eq!(code(&missing), 1);
    assert!(err_text(&missing).contains("nosuch.txt: No such file or directory"));
}

#[test]
fn gz_suffix_inputs_are_left_unchanged_and_unknown_suffixes_warn() {
    let sb = Sandbox::new();
    sb.write("done.gz", b"already packed");
    let unchanged = run_in(sb.path(), "gzip", &["done.gz"], b"");
    assert_eq!(code(&unchanged), 0);
    assert_eq!(sb.read("done.gz"), b"already packed");
    sb.write("plain.txt", b"not packed\n");
    let unknown = run_in(sb.path(), "gunzip", &["plain.txt"], b"");
    assert_eq!(code(&unknown), 2);
    assert!(err_text(&unknown).contains("unknown suffix -- ignored"));
    assert_eq!(sb.read("plain.txt"), b"not packed\n");
}

#[test]
fn gunzip_finds_the_suffixed_name_when_only_that_exists() {
    let sb = Sandbox::new();
    sb.write("note.txt", b"fallback\n");
    run_in(sb.path(), "gzip", &["note.txt"], b"");
    let output = run_in(sb.path(), "gunzip", &["note.txt"], b"");
    assert_eq!(code(&output), 0, "{}", err_text(&output));
    assert_eq!(sb.read("note.txt"), b"fallback\n");
}

#[test]
fn list_reports_sizes_and_a_header_row() {
    let sb = Sandbox::new();
    let data = sample(12_345);
    sb.write("list.bin", &data);
    run_in(sb.path(), "gzip", &["-k", "list.bin"], b"");
    let output = run_in(sb.path(), "gzip", &["-l", "list.bin.gz"], b"");
    assert_eq!(code(&output), 0);
    let shown = text(&output);
    let lines: Vec<&str> = shown.lines().collect();
    assert!(lines[0].contains("compressed") && lines[0].ends_with("uncompressed_name"));
    assert!(lines[1].contains("12345"));
    assert!(lines[1].ends_with("list.bin"));
}

#[test]
fn test_mode_flags_a_damaged_trailer() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"integrity check\n");
    run_in(sb.path(), "gzip", &["-k", "t.txt"], b"");
    let mut bytes = gz_bytes(&sb, "t.txt.gz");
    let last = bytes.len() - 8;
    bytes[last] ^= 0xff;
    sb.write("t.txt.gz", &bytes);
    let output = run_in(sb.path(), "gzip", &["-t", "t.txt.gz"], b"");
    assert_eq!(code(&output), 1);
    assert!(err_text(&output).contains("crc error"));
}

#[test]
fn truncated_input_fails_and_leaves_no_partial_output() {
    let sb = Sandbox::new();
    sb.write("cut.txt", &sample(5000));
    run_in(sb.path(), "gzip", &["-k", "cut.txt"], b"");
    fs::remove_file(sb.path().join("cut.txt")).unwrap();
    let bytes = gz_bytes(&sb, "cut.txt.gz");
    sb.write("cut.txt.gz", &bytes[..bytes.len() / 2]);
    let output = run_in(sb.path(), "gunzip", &["cut.txt.gz"], b"");
    assert_eq!(code(&output), 1);
    assert!(err_text(&output).contains("unexpected end of file"));
    assert!(!sb.path().join("cut.txt").exists());
}

#[test]
fn zcat_writes_to_stdout_and_keeps_the_file() {
    let sb = Sandbox::new();
    sb.write("z.txt", b"zcat body\n");
    run_in(sb.path(), "gzip", &["z.txt"], b"");
    let output = run_in(sb.path(), "zcat", &["z.txt.gz"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(output.stdout, b"zcat body\n");
    assert!(sb.path().join("z.txt.gz").exists());
}

#[test]
fn custom_suffix_is_used_for_output_and_input() {
    let sb = Sandbox::new();
    sb.write("s.txt", b"suffix\n");
    let packed = run_in(sb.path(), "gzip", &["-S", ".z", "s.txt"], b"");
    assert_eq!(code(&packed), 0);
    assert!(sb.path().join("s.txt.z").exists());
    let unpacked = run_in(sb.path(), "gzip", &["-dS", ".z", "s.txt.z"], b"");
    assert_eq!(code(&unpacked), 0);
    assert_eq!(sb.read("s.txt"), b"suffix\n");
}

#[test]
fn stdin_garbage_is_not_gzip() {
    let output = run("gzip", &["-d"], b"plain words\n");
    assert_eq!(code(&output), 1);
    assert!(err_text(&output).contains("stdin: not in gzip format"));
}

// ---------------------------------------------------------------- tar

/// Builds a small tree `src/a.txt`, `src/sub/b.txt` in the sandbox.
fn tree(sb: &Sandbox) {
    sb.write("src/a.txt", b"alpha\n");
    sb.write("src/sub/b.txt", b"beta\n");
}

fn sorted_lines(output: &Output) -> Vec<String> {
    let mut lines: Vec<String> = text(output).lines().map(str::to_owned).collect();
    lines.sort();
    lines
}

#[test]
fn tar_help_version_and_usage() {
    let help = run("tar", &["--help"], b"");
    assert_eq!(code(&help), 0);
    assert!(text(&help).starts_with("Usage: tar [OPTION...]"));
    let version = run("tar", &["--version"], b"");
    assert!(text(&version).starts_with("tar (agentcmd) "));
    let none = run("tar", &["-f", "x.tar"], b"");
    assert_eq!(code(&none), 1);
    assert!(err_text(&none).contains("You must specify one of the '-Acdtrux'"));
    let unknown = run("tar", &["--frobnicate"], b"");
    assert_eq!(code(&unknown), 1);
    assert!(err_text(&unknown).contains("unrecognized option '--frobnicate'"));
}

#[test]
fn create_then_list_then_extract_round_trip() {
    let sb = Sandbox::new();
    tree(&sb);
    let created = run_in(sb.path(), "tar", &["-cf", "a.tar", "src"], b"");
    assert_eq!(code(&created), 0, "{}", err_text(&created));
    let listed = run_in(sb.path(), "tar", &["-tf", "a.tar"], b"");
    assert_eq!(
        sorted_lines(&listed),
        vec!["src/", "src/a.txt", "src/sub/", "src/sub/b.txt"]
    );
    sb.mkdir("out");
    let extracted = run_in(sb.path(), "tar", &["-xf", "a.tar", "-C", "out"], b"");
    assert_eq!(code(&extracted), 0);
    assert_eq!(sb.read("out/src/a.txt"), b"alpha\n");
    assert_eq!(sb.read("out/src/sub/b.txt"), b"beta\n");
}

#[test]
fn bundled_old_style_options_and_gzip() {
    let sb = Sandbox::new();
    tree(&sb);
    let created = run_in(sb.path(), "tar", &["czf", "a.tgz", "src"], b"");
    assert_eq!(code(&created), 0, "{}", err_text(&created));
    assert_eq!(&sb.read("a.tgz")[..2], &[0x1f, 0x8b]);
    let listed = run_in(sb.path(), "tar", &["tf", "a.tgz"], b"");
    assert!(text(&listed).contains("src/a.txt"));
    sb.mkdir("out");
    let extracted = run_in(sb.path(), "tar", &["xzf", "a.tgz", "-C", "out"], b"");
    assert_eq!(code(&extracted), 0);
    assert_eq!(sb.read("out/src/sub/b.txt"), b"beta\n");
}

#[test]
fn archive_on_stdout_and_read_from_stdin() {
    let sb = Sandbox::new();
    tree(&sb);
    let written = run_in(sb.path(), "tar", &["-cf", "-", "src"], b"");
    assert_eq!(code(&written), 0);
    assert_eq!(
        written.stdout.len() % 10240,
        0,
        "archive is padded to whole records"
    );
    let listed = run("tar", &["-tf", "-"], &written.stdout);
    assert!(text(&listed).contains("src/sub/b.txt"));
}

#[test]
fn verbose_listing_uses_the_gnu_columns() {
    let sb = Sandbox::new();
    tree(&sb);
    run_in(sb.path(), "tar", &["-cf", "v.tar", "src"], b"");
    let listed = run_in(sb.path(), "tar", &["-tvf", "v.tar"], b"");
    let line = text(&listed)
        .lines()
        .find(|l| l.ends_with("src/a.txt"))
        .unwrap()
        .to_string();
    assert!(line.starts_with("-rw-r--r-- "), "{line}");
    assert!(line.contains(" 6 ") || line.contains("      6 "), "{line}");
}

#[test]
fn extract_to_stdout_prints_only_the_member() {
    let sb = Sandbox::new();
    tree(&sb);
    run_in(sb.path(), "tar", &["-cf", "o.tar", "src"], b"");
    let output = run_in(sb.path(), "tar", &["-xOf", "o.tar", "src/sub/b.txt"], b"");
    assert_eq!(code(&output), 0);
    assert_eq!(output.stdout, b"beta\n");
}

#[test]
fn strip_components_exclude_and_keep_old() {
    let sb = Sandbox::new();
    tree(&sb);
    run_in(sb.path(), "tar", &["-cf", "s.tar", "src"], b"");
    sb.mkdir("flat");
    let stripped = run_in(
        sb.path(),
        "tar",
        &[
            "-xf",
            "s.tar",
            "--strip-components=1",
            "-C",
            "flat",
            "--exclude=*.txt",
        ],
        b"",
    );
    assert_eq!(code(&stripped), 0);
    assert!(sb.path().join("flat/sub").is_dir());
    assert!(!sb.path().join("flat/a.txt").exists());
    sb.write("keep/src/a.txt", b"mine\n");
    sb.mkdir("keep");
    let kept = run_in(sb.path(), "tar", &["-xkf", "s.tar", "-C", "keep"], b"");
    assert_eq!(code(&kept), 2);
    assert_eq!(sb.read("keep/src/a.txt"), b"mine\n");
}

#[test]
fn create_with_chdir_stores_names_relative_to_it() {
    let sb = Sandbox::new();
    tree(&sb);
    let created = run_in(
        sb.path(),
        "tar",
        &["-cf", "c.tar", "-C", "src", "a.txt", "sub"],
        b"",
    );
    assert_eq!(code(&created), 0);
    let listed = run_in(sb.path(), "tar", &["-tf", "c.tar"], b"");
    assert_eq!(sorted_lines(&listed), vec!["a.txt", "sub/", "sub/b.txt"]);
}

#[test]
fn files_from_list_with_newlines_or_nuls() {
    let sb = Sandbox::new();
    tree(&sb);
    sb.write("list.txt", b"src/a.txt\nsrc/sub\n");
    let created = run_in(sb.path(), "tar", &["-cf", "f.tar", "-T", "list.txt"], b"");
    assert_eq!(code(&created), 0);
    assert_eq!(
        sorted_lines(&run_in(sb.path(), "tar", &["-tf", "f.tar"], b"")),
        vec!["src/a.txt", "src/sub/", "src/sub/b.txt"]
    );
    sb.write("nul.txt", b"src/a.txt\0");
    let nul = run_in(
        sb.path(),
        "tar",
        &["-cf", "n.tar", "--null", "-T", "nul.txt"],
        b"",
    );
    assert_eq!(code(&nul), 0);
    assert_eq!(
        sorted_lines(&run_in(sb.path(), "tar", &["-tf", "n.tar"], b"")),
        vec!["src/a.txt"]
    );
}

#[test]
fn missing_inputs_and_archives_exit_with_status_two() {
    let sb = Sandbox::new();
    tree(&sb);
    let create = run_in(sb.path(), "tar", &["-cf", "m.tar", "src", "nosuch"], b"");
    assert_eq!(code(&create), 2);
    assert!(err_text(&create).contains("nosuch: Cannot stat"));
    assert!(sb.path().join("m.tar").exists(), "archive is still written");
    let member = run_in(sb.path(), "tar", &["-tf", "m.tar", "nosuch-member"], b"");
    assert_eq!(code(&member), 2);
    assert!(err_text(&member).contains("nosuch-member: Not found in archive"));
    let missing = run_in(sb.path(), "tar", &["-xf", "absent.tar"], b"");
    assert_eq!(code(&missing), 2);
    sb.write("text.tar", b"this is definitely not a tar archive\n");
    let not_tar = run_in(sb.path(), "tar", &["-tf", "text.tar"], b"");
    assert_eq!(code(&not_tar), 2);
    assert!(err_text(&not_tar).contains("does not look like a tar archive"));
}

#[test]
fn unsupported_compression_options_are_refused() {
    let output = run("tar", &["-cjf", "x.tbz", "src"], b"");
    assert_eq!(code(&output), 1);
    assert!(err_text(&output).contains("option '-j' is not supported by agentcmd"));
}

#[test]
fn empty_create_is_refused() {
    let output = run("tar", &["-cf", "-"], b"");
    assert_eq!(code(&output), 2);
    assert!(err_text(&output).contains("Cowardly refusing to create an empty archive"));
}

// Archives with hostile member names, built in memory.

fn raw_header(name: &str, size: u64, kind: EntryType) -> Header {
    let mut header = Header::new_gnu();
    let bytes = name.as_bytes();
    header.as_old_mut().name[..bytes.len()].copy_from_slice(bytes);
    header.set_size(size);
    header.set_mode(0o644);
    header.set_mtime(1_700_000_000);
    header.set_entry_type(kind);
    header.set_cksum();
    header
}

fn archive_with(members: &[(&str, &[u8], EntryType)]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    for (name, data, kind) in members {
        let mut header = raw_header(name, data.len() as u64, *kind);
        if *kind == EntryType::Symlink {
            header.set_size(0);
            header
                .set_link_name(std::str::from_utf8(data).unwrap())
                .unwrap();
            header.set_cksum();
            builder.append(&header, std::io::empty()).unwrap();
        } else {
            builder.append(&header, *data).unwrap();
        }
    }
    builder.into_inner().unwrap()
}

#[test]
fn dot_dot_members_are_refused_and_nothing_is_written_outside() {
    let sb = Sandbox::new();
    sb.mkdir("dest");
    let bytes = archive_with(&[
        ("../escaped.txt", b"nope\n", EntryType::Regular),
        ("ok.txt", b"ok\n", EntryType::Regular),
    ]);
    sb.write("evil.tar", &bytes);
    let output = run_in(sb.path(), "tar", &["-xf", "evil.tar", "-C", "dest"], b"");
    assert_eq!(code(&output), 2);
    assert!(err_text(&output).contains("Member name contains '..'"));
    assert!(!sb.path().join("escaped.txt").exists());
    assert_eq!(sb.read("dest/ok.txt"), b"ok\n");
}

#[cfg(unix)]
#[test]
fn symlink_then_member_through_it_is_refused() {
    let sb = Sandbox::new();
    sb.mkdir("dest");
    sb.mkdir("outside");
    let outside = sb.path().join("outside").to_string_lossy().into_owned();
    let bytes = archive_with(&[
        ("lnk", outside.as_bytes(), EntryType::Symlink),
        ("lnk/planted.txt", b"planted\n", EntryType::Regular),
        ("fine.txt", b"fine\n", EntryType::Regular),
    ]);
    sb.write("escape.tar", &bytes);
    let output = run_in(sb.path(), "tar", &["-xf", "escape.tar", "-C", "dest"], b"");
    assert_eq!(code(&output), 2);
    assert!(err_text(&output).contains("Symbolic link escapes the destination"));
    assert!(!sb.path().join("outside/planted.txt").exists());
    assert_eq!(sb.read("dest/fine.txt"), b"fine\n");
}

#[test]
fn absolute_member_names_are_made_relative_with_a_warning() {
    let sb = Sandbox::new();
    sb.mkdir("dest");
    let bytes = archive_with(&[("/rooted.txt", b"rooted\n", EntryType::Regular)]);
    sb.write("abs.tar", &bytes);
    let output = run_in(sb.path(), "tar", &["-xf", "abs.tar", "-C", "dest"], b"");
    assert_eq!(code(&output), 0);
    assert!(err_text(&output).contains("Removing leading '/' from member names"));
    assert_eq!(sb.read("dest/rooted.txt"), b"rooted\n");
}

#[test]
fn gzip_and_tar_agree_on_a_large_round_trip() {
    let sb = Sandbox::new();
    sb.mkdir("big");
    sb.write("big/data.bin", &sample(300_000));
    let created = run_in(sb.path(), "tar", &["-czf", "big.tgz", "big"], b"");
    assert_eq!(code(&created), 0);
    sb.mkdir("out");
    let extracted = run_in(sb.path(), "tar", &["-xzf", "big.tgz", "-C", "out"], b"");
    assert_eq!(code(&extracted), 0);
    assert_eq!(sb.read("out/big/data.bin"), sample(300_000));
}

#[test]
fn gzip_long_option_aliases_work() {
    let sb = Sandbox::new();
    sb.write("l.txt", b"long options\n");
    let packed = run_in(
        sb.path(),
        "gzip",
        &[
            "--force",
            "--keep",
            "--rsyncable",
            "--synchronous",
            "--fast",
            "--name",
            "l.txt",
        ],
        b"",
    );
    assert_eq!(code(&packed), 0, "{}", err_text(&packed));
    assert!(sb.path().join("l.txt").exists());
    let to_stdout = run_in(
        sb.path(),
        "gzip",
        &["--to-stdout", "--best", "--quiet", "l.txt"],
        b"",
    );
    assert_eq!(code(&to_stdout), 0);
    assert_eq!(&to_stdout.stdout[..2], &[0x1f, 0x8b]);
    let verbose_test = run_in(sb.path(), "gzip", &["--test", "--verbose", "l.txt.gz"], b"");
    assert_eq!(code(&verbose_test), 0);
    assert!(err_text(&verbose_test).contains("l.txt.gz:\t OK"));
    let listed = run_in(sb.path(), "gzip", &["--list", "l.txt.gz"], b"");
    assert_eq!(code(&listed), 0);
    assert!(text(&listed).contains("l.txt"));
    fs::remove_file(sb.path().join("l.txt")).unwrap();
    let unpacked = run_in(
        sb.path(),
        "gzip",
        &["--uncompress", "--recursive", "--suffix=.gz", "l.txt.gz"],
        b"",
    );
    assert_eq!(code(&unpacked), 0, "{}", err_text(&unpacked));
    assert_eq!(sb.read("l.txt"), b"long options\n");
}

#[test]
fn tar_long_option_aliases_work() {
    let sb = Sandbox::new();
    tree(&sb);
    let created = run_in(
        sb.path(),
        "tar",
        &[
            "--create",
            "--auto-compress",
            "--file=auto.tgz",
            "--dereference",
            "--verbose",
            "src",
        ],
        b"",
    );
    assert_eq!(code(&created), 0, "{}", err_text(&created));
    assert_eq!(&sb.read("auto.tgz")[..2], &[0x1f, 0x8b]);
    let listed = run_in(
        sb.path(),
        "tar",
        &[
            "--list",
            "--file=auto.tgz",
            "--wildcards",
            "--exclude=*.nothing",
        ],
        b"",
    );
    assert!(text(&listed).contains("src/a.txt"));
    sb.mkdir("out");
    let extracted = run_in(
        sb.path(),
        "tar",
        &[
            "--get",
            "--gunzip",
            "--file",
            "auto.tgz",
            "--directory=out",
            "--overwrite",
            "--same-permissions",
            "--no-same-owner",
            "--preserve-permissions",
        ],
        b"",
    );
    assert_eq!(code(&extracted), 0, "{}", err_text(&extracted));
    assert_eq!(sb.read("out/src/a.txt"), b"alpha\n");
    let streamed = run_in(
        sb.path(),
        "tar",
        &[
            "--extract",
            "--ungzip",
            "--to-stdout",
            "--file=auto.tgz",
            "src/a.txt",
        ],
        b"",
    );
    assert_eq!(streamed.stdout, b"alpha\n");
    let kept = run_in(
        sb.path(),
        "tar",
        &[
            "--extract",
            "--keep-old-files",
            "--file=auto.tgz",
            "--directory=out",
        ],
        b"",
    );
    assert_eq!(code(&kept), 2);
}
