//! CLI tests for the P2 option gaps: `ls`, `find`, `df` and `touch -d`. Every test works in a
//! `Sandbox` temporary directory. Times are pinned with explicit modification times and
//! `TZ=UTC`, so the output does not depend on the machine's clock or zone.

mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, UNIX_EPOCH};

use common::{run_in, Sandbox};

/// Runs `agentcmd <util> <args>` in `dir` with `TZ=UTC` and `COLUMNS=80`, feeding `stdin`.
fn agentcmd(dir: &Path, util: &str, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentcmd"))
        .current_dir(dir)
        .arg(util)
        .args(args)
        .env("TZ", "UTC")
        .env("COLUMNS", "80")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn agentcmd");
    if let Err(error) = child.stdin.take().expect("stdin").write_all(stdin) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().expect("wait")
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> Option<i32> {
    output.status.code()
}

fn set_mtime(path: &Path, seconds: u64) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

fn mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// The line of a long listing that ends with `name`.
fn long_line<'a>(listing: &'a str, name: &str) -> &'a str {
    listing
        .lines()
        .find(|line| line.ends_with(name))
        .unwrap_or_else(|| panic!("no line for {name} in:\n{listing}"))
}

// ---------------------------------------------------------------- ls

#[test]
fn ls_help_and_version_are_printed_with_status_zero() {
    let sb = Sandbox::new();
    let help = run_in(sb.path(), "ls", &["--help"], b"");
    assert_eq!(code(&help), Some(0));
    let help_text = text(&help);
    assert!(help_text.starts_with("Usage: ls"), "{help_text}");
    for option in [
        "--time-style",
        "--quoting-style",
        "--block-size",
        "--indicator-style",
    ] {
        assert!(help_text.contains(option), "{option} missing from help");
    }
    let version = run_in(sb.path(), "ls", &["--version"], b"");
    assert_eq!(code(&version), Some(0));
    assert!(
        text(&version).starts_with("ls (agentcmd) "),
        "{}",
        text(&version)
    );
}

#[test]
fn ls_unsupported_and_invalid_options_have_gnu_statuses() {
    let sb = Sandbox::new();
    sb.write("a", b"a");
    let dired = run_in(sb.path(), "ls", &["-D", "a"], b"");
    assert_eq!(code(&dired), Some(2));
    assert!(err_text(&dired).contains("'-D' is not supported by agentcmd"));
    // An invalid choice for an argmatch option exits 1 (as GNU does), a bad number exits 2.
    assert_eq!(
        code(&run_in(sb.path(), "ls", &["--time=later"], b"")),
        Some(1)
    );
    assert_eq!(
        code(&run_in(sb.path(), "ls", &["--sort=random"], b"")),
        Some(1)
    );
    assert_eq!(
        code(&run_in(sb.path(), "ls", &["--block-size=0"], b"")),
        Some(2)
    );
    assert_eq!(
        code(&run_in(sb.path(), "ls", &["-w", "wide"], b"")),
        Some(2)
    );
    assert_eq!(code(&run_in(sb.path(), "ls", &["-T", "x"], b"")), Some(2));
}

#[test]
fn ls_time_style_full_and_long_iso_use_the_zone_and_fraction() {
    let sb = Sandbox::new();
    sb.write("old", b"abc");
    set_mtime(&sb.path().join("old"), 1_700_000_000);
    let full = agentcmd(sb.path(), "ls", &["-l", "--full-time", "old"], b"");
    assert!(
        text(&full).ends_with("2023-11-14 22:13:20.000000000 +0000 old\n"),
        "{}",
        text(&full)
    );
    let long = agentcmd(
        sb.path(),
        "ls",
        &["-l", "--time-style=long-iso", "old"],
        b"",
    );
    assert!(
        text(&long).ends_with(" 2023-11-14 22:13 old\n"),
        "{}",
        text(&long)
    );
    let custom = agentcmd(sb.path(), "ls", &["-l", "--time-style=+%Y/%j", "old"], b"");
    assert!(
        text(&custom).ends_with(" 2023/318 old\n"),
        "{}",
        text(&custom)
    );
    let iso = agentcmd(
        sb.path(),
        "ls",
        &["-l", "--time=mtime", "--time-style=iso", "old"],
        b"",
    );
    assert!(text(&iso).ends_with(" 2023-11-14  old\n"), "{}", text(&iso));
}

#[test]
fn ls_long_format_shows_owner_and_group_names_and_numeric_ids_with_n() {
    let sb = Sandbox::new();
    sb.write("f", b"x");
    let names = agentcmd(sb.path(), "ls", &["-l", "--time-style=long-iso", "f"], b"");
    let listing = text(&names);
    let fields: Vec<&str> = long_line(&listing, "f").split_whitespace().collect();
    assert_eq!(fields.len(), 8, "{fields:?}");
    if cfg!(unix) {
        assert_ne!(fields[2], "-", "owner name expected: {fields:?}");
    }
    let numeric = agentcmd(sb.path(), "ls", &["-n", "--time-style=long-iso", "f"], b"");
    let numeric_listing = text(&numeric);
    let numbers: Vec<&str> = long_line(&numeric_listing, "f")
        .split_whitespace()
        .collect();
    if cfg!(unix) {
        assert!(numbers[2].parse::<u32>().is_ok(), "{numbers:?}");
    }
    let no_group = agentcmd(sb.path(), "ls", &["-lG", "--time-style=long-iso", "f"], b"");
    assert_eq!(
        long_line(&text(&no_group), "f").split_whitespace().count(),
        7
    );
}

#[cfg(unix)]
#[test]
fn ls_inode_and_block_columns_match_metadata() {
    use std::os::unix::fs::MetadataExt;
    let sb = Sandbox::new();
    sb.write("f", b"abc");
    let inode = fs::metadata(sb.path().join("f")).unwrap().ino();
    let short = text(&run_in(sb.path(), "ls", &["-i", "f"], b""));
    assert_eq!(short, format!("{inode} f\n"));
    let blocks = text(&run_in(sb.path(), "ls", &["-s", "f"], b""));
    let count: u64 = blocks.split_whitespace().next().unwrap().parse().unwrap();
    assert!(count >= 1, "{blocks}");
    let both = text(&run_in(sb.path(), "ls", &["-is", "f"], b""));
    assert!(both.starts_with(&format!("{inode} ")), "{both}");
    let human = text(&run_in(sb.path(), "ls", &["-s", "-h", "f"], b""));
    assert!(
        human.trim_start().starts_with(&count.to_string()) || human.contains('K'),
        "{human}"
    );
}

#[test]
fn ls_total_line_appears_for_long_and_block_listings_of_directories() {
    let sb = Sandbox::new();
    sb.write("d/one", b"1");
    sb.write("d/two", b"2");
    let long = text(&run_in(sb.path(), "ls", &["-l", "d"], b""));
    assert!(long.starts_with("total "), "{long}");
    let blocks = text(&run_in(sb.path(), "ls", &["-s", "d"], b""));
    assert!(blocks.starts_with("total "), "{blocks}");
    let file_only = text(&run_in(sb.path(), "ls", &["-s", "d/one"], b""));
    assert!(!file_only.contains("total"), "{file_only}");
}

#[test]
fn ls_ignore_hide_and_backup_filters() {
    let sb = Sandbox::new();
    for name in ["keep", "old~", "x.log", "y.log"] {
        sb.write(name, b"");
    }
    let backups = text(&run_in(sb.path(), "ls", &["-B"], b""));
    assert_eq!(backups, "keep\nx.log\ny.log\n");
    let ignored = text(&run_in(
        sb.path(),
        "ls",
        &["-I", "*.log", "--ignore=old*"],
        b"",
    ));
    assert_eq!(ignored, "keep\n");
    let hidden = text(&run_in(sb.path(), "ls", &["--hide=k*"], b""));
    assert_eq!(hidden, "old~\nx.log\ny.log\n");
    let overridden = text(&run_in(sb.path(), "ls", &["-A", "--hide=k*"], b""));
    assert!(overridden.contains("keep\n"), "{overridden}");
}

#[test]
fn ls_sort_options_order_by_extension_size_and_name() {
    let sb = Sandbox::new();
    sb.write("b.txt", b"");
    sb.write("a.md", b"");
    sb.write("c", b"");
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["-X"], b"")),
        "c\na.md\nb.txt\n"
    );
    sb.write("big", b"xxxxxxxxxx");
    sb.write("tiny", b"x");
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["--sort=size"], b""))
            .lines()
            .next(),
        Some("big")
    );
}

#[test]
fn ls_column_and_comma_layouts_wrap_at_the_width() {
    let sb = Sandbox::new();
    for index in 1..=12 {
        sb.write(&format!("file_{index:02}.txt"), b"");
    }
    let commas = text(&run_in(sb.path(), "ls", &["-m"], b""));
    assert!(commas.contains(",\n"), "{commas}");
    assert!(
        commas.lines().all(|line| line.chars().count() <= 80),
        "{commas}"
    );
    let across = text(&run_in(sb.path(), "ls", &["-x", "-w", "40"], b""));
    assert!(across.lines().count() > 1, "{across}");
    let one_line = text(&run_in(sb.path(), "ls", &["-C", "-w", "0"], b""));
    assert_eq!(one_line.lines().count(), 1, "{one_line}");
    let single = text(&run_in(sb.path(), "ls", &["-1"], b""));
    assert_eq!(single.lines().count(), 12);
}

#[test]
fn ls_quoting_styles_quote_names_as_gnu_does() {
    let sb = Sandbox::new();
    sb.write("it's", b"");
    sb.write("sp ace", b"");
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["--quoting-style=shell"], b"")),
        "\"it's\"\n'sp ace'\n"
    );
    assert_eq!(
        text(&run_in(
            sb.path(),
            "ls",
            &["--quoting-style=shell-always"],
            b""
        )),
        "\"it's\"\n'sp ace'\n"
    );
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["-N"], b"")),
        "it's\nsp ace\n"
    );
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["-Q"], b"")),
        "\"it's\"\n\"sp ace\"\n"
    );
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["--quoting-style=c"], b"")),
        "\"it's\"\n\"sp ace\"\n"
    );
}

#[cfg(unix)]
#[test]
fn ls_c_style_escapes_control_characters_and_quotes() {
    let sb = Sandbox::new();
    sb.write("tab\there", b"");
    sb.write("q\"d", b"");
    let quoted = text(&run_in(sb.path(), "ls", &["-Q"], b""));
    assert_eq!(quoted, "\"q\\\"d\"\n\"tab\\there\"\n");
    let escaped = text(&run_in(sb.path(), "ls", &["--quoting-style=escape"], b""));
    assert_eq!(escaped, "q\"d\ntab\\there\n");
}

#[cfg(unix)]
#[test]
fn ls_indicator_styles_mark_directories_links_and_executables() {
    use std::os::unix::fs::PermissionsExt;
    let sb = Sandbox::new();
    sb.mkdir("dir");
    sb.write("run.sh", b"#!/bin/sh\n");
    fs::set_permissions(sb.path().join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    sb.write("plain", b"p");
    sb.symlink("plain", "lnk").unwrap();
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["--file-type"], b"")),
        "dir/\nlnk@\nplain\nrun.sh\n"
    );
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["-F"], b"")),
        "dir/\nlnk@\nplain\nrun.sh*\n"
    );
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["--classify=never"], b"")),
        "dir\nlnk\nplain\nrun.sh\n"
    );
    assert_eq!(
        text(&run_in(sb.path(), "ls", &["--indicator-style=slash"], b"")),
        "dir/\nlnk\nplain\nrun.sh\n"
    );
}

#[cfg(unix)]
#[test]
fn ls_dereference_shows_the_target_of_a_link_operand() {
    let sb = Sandbox::new();
    sb.write("target", b"12345");
    sb.symlink("target", "link").unwrap();
    let plain = text(&agentcmd(
        sb.path(),
        "ls",
        &["-l", "--time-style=long-iso", "link"],
        b"",
    ));
    assert!(plain.starts_with('l'), "{plain}");
    let followed = text(&agentcmd(
        sb.path(),
        "ls",
        &["-lL", "--time-style=long-iso", "link"],
        b"",
    ));
    assert!(
        followed.starts_with('-') && followed.ends_with(" link\n"),
        "{followed}"
    );
    assert!(!followed.contains("->"), "{followed}");
}

#[cfg(unix)]
#[test]
fn ls_recursive_dereference_stops_at_symlink_loops() {
    let sb = Sandbox::new();
    sb.write("sub/f", b"f");
    sb.symlink("..", "sub/up").unwrap();
    let out = run_in(sb.path(), "ls", &["-RL", "sub"], b"");
    assert_eq!(code(&out), Some(1));
    assert!(
        err_text(&out).contains("not listing already-listed directory"),
        "{}",
        err_text(&out)
    );
    assert!(text(&out).contains("sub/up:\nsub\n"), "{}", text(&out));
}

#[cfg(unix)]
#[test]
fn ls_block_counts_scale_with_block_size_and_suffix() {
    let sb = Sandbox::new();
    sb.mkdir("d");
    sb.write("d/inner", b"hi");
    let kib = text(&run_in(
        sb.path(),
        "ls",
        &["-s", "--block-size=K", "d"],
        b"",
    ));
    assert!(kib.starts_with("total 4K\n"), "{kib}");
    let kilo = text(&run_in(sb.path(), "ls", &["-s", "--si", "d"], b""));
    assert!(kilo.starts_with("total 4.1k\n"), "{kilo}");
    let bytes = text(&run_in(
        sb.path(),
        "ls",
        &["-s", "--block-size=1", "d"],
        b"",
    ));
    assert!(bytes.contains("4096 inner\n"), "{bytes}");
}

// ---------------------------------------------------------------- find

#[test]
fn find_owner_tests_report_unknown_names() {
    let sb = Sandbox::new();
    sb.write("f", b"x");
    let unknown = run_in(
        sb.path(),
        "find",
        &[".", "-user", "agentcmd-no-such-user-xyz"],
        b"",
    );
    assert_eq!(code(&unknown), Some(1));
    assert!(err_text(&unknown).contains("is not the name of a known user"));
    let unknown_group = run_in(
        sb.path(),
        "find",
        &[".", "-group", "agentcmd-no-such-group-xyz"],
        b"",
    );
    assert_eq!(code(&unknown_group), Some(1));
    let none = run_in(sb.path(), "find", &[".", "-nouser", "-o", "-nogroup"], b"");
    assert_eq!(code(&none), Some(0));
    assert_eq!(text(&none), "");
}

#[test]
fn find_newermt_and_newerxy_compare_modification_times() {
    let sb = Sandbox::new();
    sb.write("old", b"o");
    sb.write("new", b"n");
    set_mtime(&sb.path().join("old"), 1_600_000_000);
    set_mtime(&sb.path().join("new"), 1_800_000_000);
    let newer = text(&run_in(
        sb.path(),
        "find",
        &[".", "-type", "f", "-newer", "old"],
        b"",
    ));
    assert_eq!(newer, "./new\n");
    let by_date = text(&run_in(
        sb.path(),
        "find",
        &[".", "-type", "f", "-newermt", "2021-01-01"],
        b"",
    ));
    assert_eq!(by_date, "./new\n");
    let xy = text(&run_in(
        sb.path(),
        "find",
        &[".", "-type", "f", "-newermm", "old"],
        b"",
    ));
    assert_eq!(xy, "./new\n");
    let bad = run_in(sb.path(), "find", &[".", "-newerqq", "old"], b"");
    assert_eq!(code(&bad), Some(1));
}

#[test]
fn find_regextype_selects_the_pattern_dialect() {
    let sb = Sandbox::new();
    sb.write("old.txt", b"");
    sb.write("other.txt", b"");
    let basic = text(&run_in(
        sb.path(),
        "find",
        &[
            ".",
            "-regextype",
            "posix-basic",
            "-regex",
            "./\\(old\\|nope\\)\\.txt",
        ],
        b"",
    ));
    assert_eq!(basic, "./old.txt\n");
    let extended = text(&run_in(
        sb.path(),
        "find",
        &[
            ".",
            "-regextype",
            "posix-extended",
            "-regex",
            "./(old|other)\\.txt",
        ],
        b"",
    ));
    assert_eq!(extended, "./old.txt\n./other.txt\n");
    let unknown = run_in(sb.path(), "find", &[".", "-regextype", "bogus"], b"");
    assert_eq!(code(&unknown), Some(1));
}

#[test]
fn find_file_outputs_write_the_named_files() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"");
    let status = run_in(
        sb.path(),
        "find",
        &[".", "-name", "a.txt", "-fprint", "list"],
        b"",
    );
    assert_eq!(code(&status), Some(0));
    assert_eq!(sb.read("list"), b"./a.txt\n");
    run_in(
        sb.path(),
        "find",
        &[".", "-name", "a.txt", "-fprint0", "zero"],
        b"",
    );
    assert_eq!(sb.read("zero"), b"./a.txt\0");
    run_in(
        sb.path(),
        "find",
        &[".", "-name", "a.txt", "-fprintf", "fmt", "%f=%s\\n"],
        b"",
    );
    assert_eq!(sb.read("fmt"), b"a.txt=0\n");
    let unwritable = run_in(sb.path(), "find", &[".", "-fprint", "no-such-dir/out"], b"");
    assert_eq!(code(&unwritable), Some(1));
}

#[test]
fn find_ok_asks_on_stderr_and_runs_only_on_yes() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"");
    let yes = agentcmd(
        sb.path(),
        "find",
        &[".", "-name", "a.txt", "-ok", "echo", "{}", ";"],
        b"y\n",
    );
    assert_eq!(text(&yes), "./a.txt\n");
    assert!(
        err_text(&yes).contains("< echo ./a.txt > ? "),
        "{}",
        err_text(&yes)
    );
    let no = agentcmd(
        sb.path(),
        "find",
        &[".", "-name", "a.txt", "-ok", "echo", "{}", ";"],
        b"n\n",
    );
    assert_eq!(text(&no), "");
    let eof = agentcmd(
        sb.path(),
        "find",
        &[".", "-name", "a.txt", "-okdir", "echo", "{}", ";"],
        b"",
    );
    assert_eq!(text(&eof), "");
    assert_eq!(code(&eof), Some(0));
}

#[cfg(unix)]
#[test]
fn find_links_inode_samefile_and_link_name_tests() {
    let sb = Sandbox::new();
    sb.write("target.txt", b"t");
    sb.symlink("target.txt", "link").unwrap();
    sb.symlink("nowhere", "dangling").unwrap();
    let dangling = text(&run_in(sb.path(), "find", &[".", "-xtype", "l"], b""));
    assert_eq!(dangling, "./dangling\n");
    let via_target = text(&run_in(
        sb.path(),
        "find",
        &[".", "-xtype", "f", "-name", "link"],
        b"",
    ));
    assert_eq!(via_target, "./link\n");
    let name = text(&run_in(sb.path(), "find", &[".", "-lname", "target*"], b""));
    assert_eq!(name, "./link\n");
    let insensitive = text(&run_in(
        sb.path(),
        "find",
        &[".", "-ilname", "TARGET*"],
        b"",
    ));
    assert_eq!(insensitive, "./link\n");
    let same = text(&run_in(
        sb.path(),
        "find",
        &[".", "-samefile", "target.txt"],
        b"",
    ));
    assert_eq!(same, "./target.txt\n");
    let inode_zero = run_in(sb.path(), "find", &[".", "-inum", "0"], b"");
    assert_eq!(code(&inode_zero), Some(0));
    assert_eq!(text(&inode_zero), "");
    let links = text(&run_in(
        sb.path(),
        "find",
        &[".", "-type", "f", "-links", "1"],
        b"",
    ));
    assert_eq!(links, "./target.txt\n");
}

#[test]
fn find_help_lists_the_new_options() {
    let sb = Sandbox::new();
    let help = run_in(sb.path(), "find", &["--help"], b"");
    assert_eq!(code(&help), Some(0));
    let help_text = text(&help);
    for option in [
        "-fprintf",
        "-okdir",
        "-newerXY",
        "-regextype",
        "-samefile",
        "-xtype",
    ] {
        assert!(help_text.contains(option), "{option} missing from help");
    }
}

// ---------------------------------------------------------------- df

#[test]
fn df_output_selects_fields_in_order() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "df", &["--output=target,size", "."], b"");
    assert_eq!(code(&out), Some(0), "{}", err_text(&out));
    let listing = text(&out);
    let mut lines = listing.lines();
    assert!(lines.next().unwrap().starts_with("Mounted on"), "{listing}");
    let row = lines.next().unwrap();
    assert!(!row.is_empty(), "{listing}");
}

#[test]
fn df_output_default_fields_and_total_row() {
    let sb = Sandbox::new();
    let out = text(&run_in(sb.path(), "df", &["--total", "."], b""));
    let header = out.lines().next().unwrap();
    assert!(
        header.starts_with("Filesystem") && header.ends_with("Mounted on"),
        "{out}"
    );
    assert!(out.lines().last().unwrap().starts_with("total"), "{out}");
    let plain = text(&run_in(sb.path(), "df", &["--output=source"], b""));
    assert!(plain.starts_with("Filesystem\n"), "{plain}");
}

#[test]
fn df_output_rejects_bad_field_lists_and_conflicts() {
    let sb = Sandbox::new();
    for args in [
        vec!["--output=itotal", "."],
        vec!["--output=bogus", "."],
        vec!["--output=size,size", "."],
        vec!["--output=size", "-T", "."],
        vec!["--output=size", "-P", "."],
    ] {
        let out = run_in(sb.path(), "df", &args, b"");
        assert_eq!(code(&out), Some(1), "{args:?}");
        assert!(!err_text(&out).is_empty(), "{args:?}");
    }
}

#[test]
fn df_inode_report_is_not_supported() {
    let sb = Sandbox::new();
    for args in [vec!["-i", "."], vec!["--inodes", "."]] {
        let out = run_in(sb.path(), "df", &args, b"");
        assert_eq!(code(&out), Some(1), "{args:?}");
        assert!(
            err_text(&out).contains("is not supported by agentcmd"),
            "{}",
            err_text(&out)
        );
    }
}

#[test]
fn df_help_lists_output_and_total() {
    let sb = Sandbox::new();
    let help = run_in(sb.path(), "df", &["--help"], b"");
    assert_eq!(code(&help), Some(0));
    let help_text = text(&help);
    assert!(
        help_text.contains("--output") && help_text.contains("--total"),
        "{help_text}"
    );
}

// ---------------------------------------------------------------- touch -d

#[test]
fn touch_d_accepts_free_form_dates_and_relative_items() {
    let sb = Sandbox::new();
    let cases = [
        ("1 Jan 2000 10:00", 946_720_800u64),
        ("Jan 1 2000 10:00", 946_720_800),
        ("2000-01-01 10:00:00 UTC", 946_720_800),
        ("2000-01-01 +2 days", 946_857_600),
        ("@1000000", 1_000_000),
    ];
    for (index, (date, expected)) in cases.iter().enumerate() {
        let name = format!("t{index}");
        let out = agentcmd(sb.path(), "touch", &["-d", date, &name], b"");
        assert_eq!(code(&out), Some(0), "{date}: {}", err_text(&out));
        assert_eq!(mtime_secs(&sb.path().join(&name)), *expected, "{date}");
    }
}

#[test]
fn touch_d_rejects_unparseable_text() {
    let sb = Sandbox::new();
    let out = agentcmd(sb.path(), "touch", &["-d", "not a date", "t"], b"");
    assert_eq!(code(&out), Some(1));
    assert!(
        err_text(&out).contains("invalid date format 'not a date'"),
        "{}",
        err_text(&out)
    );
}
