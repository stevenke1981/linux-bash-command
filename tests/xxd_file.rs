//! CLI tests for `xxd` and `file`. `file` has no GNU differential cases (libmagic is far
//! larger than the subset here), so its detection branches are covered by golden output
//! over samples written into a sandbox.

mod common;

use common::{run, run_in, Sandbox};

fn stdout(out: &std::process::Output) -> Vec<u8> {
    out.stdout.clone()
}

fn text(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn code(out: &std::process::Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

// ---------------------------------------------------------------- xxd

#[test]
fn xxd_help_and_version() {
    let out = run("xxd", &["--help"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out).starts_with("Usage:"));
    let out = run("xxd", &["--version"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out).starts_with("xxd (agentcmd) "));
}

#[test]
fn xxd_default_dump_from_stdin() {
    let out = run("xxd", &[], b"hello\n");
    assert_eq!(code(&out), 0);
    assert_eq!(
        text(&out),
        "00000000: 6865 6c6c 6f0a                           hello.\n"
    );
}

#[test]
fn xxd_reverse_round_trips_a_dump() {
    let data: Vec<u8> = (0u8..=40).collect();
    let dump = run("xxd", &[], &data);
    let back = run("xxd", &["-r"], &dump.stdout);
    assert_eq!(code(&back), 0);
    assert_eq!(stdout(&back), data);
}

#[test]
fn xxd_plain_hex_and_reverse_plain() {
    let out = run("xxd", &["-p", "-c", "4"], b"hello\n");
    assert_eq!(text(&out), "68656c6c\n6f0a\n");
    let back = run("xxd", &["-r", "-p"], text(&out).as_bytes());
    assert_eq!(stdout(&back), b"hello\n");
}

#[test]
fn xxd_include_from_a_file_names_the_array_after_it() {
    let sb = Sandbox::new();
    sb.write("hello.txt", b"hello\n");
    let out = run_in(sb.path(), "xxd", &["-i", "hello.txt"], b"");
    assert_eq!(
        text(&out),
        "unsigned char hello_txt[] = {\n  0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x0a\n};\nunsigned int hello_txt_len = 6;\n"
    );
}

#[test]
fn xxd_include_from_stdin_has_no_header_without_name() {
    let out = run("xxd", &["-i"], b"AB");
    assert_eq!(text(&out), "  0x41, 0x42\n");
    let out = run("xxd", &["-i", "-n", "buf"], b"AB");
    assert!(text(&out).starts_with("unsigned char buf[] = {\n"));
}

#[test]
fn xxd_length_seek_and_offset_options() {
    let out = run("xxd", &["-l", "3", "-s", "1"], b"hello\n");
    assert!(text(&out).starts_with("00000001: 656c 6c ") && text(&out).ends_with("  ell\n"));
    let sb = Sandbox::new();
    sb.write("h.txt", b"hello\n");
    let out = run_in(sb.path(), "xxd", &["-s", "-3", "h.txt"], b"");
    assert!(text(&out).starts_with("00000003: 6c6f 0a ") && text(&out).ends_with("  lo.\n"));
    let out = run("xxd", &["-s", "-3"], b"hello\n");
    assert_eq!(code(&out), 4, "a stream cannot seek from the end");
    let out = run("xxd", &["-o", "256", "-cols", "4"], b"abcde");
    let expected = format!(
        "00000100: 6162 6364  abcd\n00000104: 65{}  e\n",
        " ".repeat(7)
    );
    assert_eq!(text(&out), expected);
}

#[test]
fn xxd_autoskip_replaces_zero_runs_with_a_star() {
    let mut data = vec![0u8; 80];
    data.push(b'x');
    let out = run("xxd", &["-a"], &data);
    let lines: Vec<String> = text(&out)
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect();
    assert!(lines[0].starts_with("00000000:"), "{lines:?}");
    assert_eq!(lines[1], "*");
    assert!(lines[2].starts_with("00000050:"), "{lines:?}");
}

#[test]
fn xxd_output_file_operand_receives_the_dump() {
    let sb = Sandbox::new();
    sb.write("in.txt", b"hello\n");
    let out = run_in(sb.path(), "xxd", &["-u", "in.txt", "out.txt"], b"");
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).is_empty());
    assert_eq!(
        sb.read("out.txt"),
        b"00000000: 6865 6C6C 6F0A                           hello.\n"
    );
}

#[test]
fn xxd_invalid_columns_and_unknown_option_exit_one() {
    let out = run("xxd", &["-c", "300"], b"x");
    assert_eq!(code(&out), 1);
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid number of columns (max. 256)."));
    let out = run("xxd", &["-x"], b"x");
    assert_eq!(code(&out), 1);
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage:"));
    assert!(stdout(&out).is_empty());
}

#[test]
fn xxd_missing_input_exits_two() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "xxd", &["nosuch.bin"], b"");
    assert_eq!(code(&out), 2);
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "xxd: nosuch.bin: No such file or directory\n"
    );
}

#[test]
fn xxd_remaining_option_spellings() {
    let out = run("xxd", &["-plain", "-len", "2"], b"hello");
    assert_eq!(text(&out), "6865\n");
    let out = run("xxd", &["-v"], b"");
    assert!(text(&out).starts_with("xxd (agentcmd) "));
    let out = run("xxd", &["-h"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out).starts_with("Usage:"));
    let out = run("xxd", &["-C", "-i", "-n", "x"], b"A");
    assert!(
        text(&out).contains("unsigned int X_LEN = 1;"),
        "{}",
        text(&out)
    );
}

#[test]
fn xxd_bits_and_little_endian_layouts() {
    let out = run("xxd", &["-b", "-c", "2"], b"hi");
    assert_eq!(text(&out), "00000000: 01101000 01101001  hi\n");
    let out = run("xxd", &["-e", "-g", "3"], b"abc");
    assert_eq!(code(&out), 1, "group size must be a power of two with -e");
}

// ---------------------------------------------------------------- file

#[test]
fn file_reports_missing_operands_inline_and_exits_zero() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "file", &["nosuch"], b"");
    assert_eq!(code(&out), 0);
    assert_eq!(
        text(&out),
        "nosuch: cannot open `nosuch' (No such file or directory)\n"
    );
}

#[test]
fn file_identifies_text_encodings_and_terminators() {
    let sb = Sandbox::new();
    sb.write("ascii.txt", b"hello\n");
    sb.write("crlf.txt", b"a\r\nb\r\n");
    sb.write("mixed.txt", b"a\r\nb\n");
    sb.write("utf8.txt", "héllo".as_bytes());
    sb.write("nonl.txt", b"no newline");
    sb.write("empty.txt", b"");
    let out = run_in(
        sb.path(),
        "file",
        &[
            "ascii.txt",
            "crlf.txt",
            "mixed.txt",
            "utf8.txt",
            "nonl.txt",
            "empty.txt",
        ],
        b"",
    );
    assert_eq!(
        text(&out),
        "ascii.txt: ASCII text\n\
         crlf.txt: ASCII text, with CRLF line terminators\n\
         mixed.txt: ASCII text, with CRLF, LF line terminators\n\
         utf8.txt: Unicode text, UTF-8 text, with no line terminators\n\
         nonl.txt: ASCII text, with no line terminators\n\
         empty.txt: empty\n"
    );
}

#[test]
fn file_identifies_scripts_and_binary_signatures() {
    let sb = Sandbox::new();
    sb.write("run.py", b"#!/usr/bin/env python3\nprint(1)\n");
    sb.write(
        "pic.png",
        &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0,
            0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0,
        ],
    );
    sb.write("doc.pdf", b"%PDF-1.4\n%....\n");
    sb.write("blob.bin", b"a\x00b");
    let out = run_in(
        sb.path(),
        "file",
        &["run.py", "pic.png", "doc.pdf", "blob.bin"],
        b"",
    );
    assert_eq!(
        text(&out),
        "run.py: Python script, ASCII text executable\n\
         pic.png: PNG image data, 2 x 3, 8-bit/color RGB, non-interlaced\n\
         doc.pdf: PDF document, version 1.4\n\
         blob.bin: data\n"
    );
}

#[test]
fn file_identifies_elf_and_pe_headers() {
    let sb = Sandbox::new();
    let mut elf = vec![0u8; 64];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[16] = 2;
    elf[18] = 0x3e;
    sb.write("prog", &elf);
    let mut pe = vec![0u8; 0x200];
    pe[0] = b'M';
    pe[1] = b'Z';
    pe[0x3c] = 0x80;
    pe[0x80..0x84].copy_from_slice(b"PE\0\0");
    pe[0x84] = 0x64;
    pe[0x85] = 0x86;
    pe[0x98] = 0x0b;
    pe[0x99] = 0x02;
    pe[0xdc] = 3;
    sb.write("prog.exe", &pe);
    let out = run_in(sb.path(), "file", &["prog", "prog.exe"], b"");
    assert_eq!(
        text(&out),
        "prog: ELF 64-bit LSB executable, x86-64, version 1 (SYSV)\n\
         prog.exe: PE32+ executable (console) x86-64, for MS Windows\n"
    );
}

#[test]
fn file_directories_and_symlinks() {
    let sb = Sandbox::new();
    sb.mkdir("dir");
    sb.write("target.txt", b"hi\n");
    let out = run_in(sb.path(), "file", &["dir"], b"");
    assert_eq!(text(&out), "dir: directory\n");
    #[cfg(unix)]
    {
        if sb.symlink("target.txt", "link").is_some() {
            let out = run_in(sb.path(), "file", &["link"], b"");
            assert_eq!(text(&out), "link: symbolic link to target.txt\n");
            let out = run_in(sb.path(), "file", &["-L", "link"], b"");
            assert_eq!(text(&out), "link: ASCII text\n");
        }
    }
}

#[test]
fn file_mime_brief_and_print0_options() {
    let sb = Sandbox::new();
    sb.write("t.txt", b"hi\n");
    let out = run_in(sb.path(), "file", &["-i", "t.txt"], b"");
    assert_eq!(text(&out), "t.txt: text/plain; charset=us-ascii\n");
    let out = run_in(sb.path(), "file", &["--mime-type", "t.txt"], b"");
    assert_eq!(text(&out), "t.txt: text/plain\n");
    let out = run_in(sb.path(), "file", &["--mime-encoding", "t.txt"], b"");
    assert_eq!(text(&out), "t.txt: us-ascii\n");
    let out = run_in(sb.path(), "file", &["-b", "t.txt"], b"");
    assert_eq!(text(&out), "ASCII text\n");
    let out = run_in(sb.path(), "file", &["-0", "t.txt"], b"");
    assert_eq!(stdout(&out), b"t.txt\0: ASCII text\n");
}

#[test]
fn file_reads_names_from_a_list_and_stdin() {
    let sb = Sandbox::new();
    sb.write("a.txt", b"a\n");
    sb.write("names.lst", b"a.txt\n");
    let out = run_in(sb.path(), "file", &["-f", "names.lst"], b"");
    assert_eq!(text(&out), "a.txt: ASCII text\n");
    let out = run_in(sb.path(), "file", &["-b", "-"], b"#!/bin/sh\n");
    assert_eq!(text(&out), "POSIX shell script, ASCII text executable\n");
}

#[test]
fn file_help_version_and_no_dereference() {
    let out = run("file", &["--help"], b"");
    assert_eq!(code(&out), 0);
    assert!(text(&out).starts_with("Usage: file"));
    let out = run("file", &["--version"], b"");
    assert!(text(&out).starts_with("file (agentcmd) "));
    #[cfg(unix)]
    {
        let sb = Sandbox::new();
        sb.write("target.txt", b"hi\n");
        if sb.symlink("target.txt", "link").is_some() {
            let out = run_in(sb.path(), "file", &["-h", "link"], b"");
            assert_eq!(text(&out), "link: symbolic link to target.txt\n");
        }
    }
}

#[test]
fn file_usage_and_unsupported_options() {
    let sb = Sandbox::new();
    let out = run_in(sb.path(), "file", &[], b"");
    assert_eq!(code(&out), 1);
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: file"));
    let out = run_in(sb.path(), "file", &["-z", "x"], b"");
    assert_eq!(code(&out), 1);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("option '-z' is not supported by agentcmd")
    );
}
