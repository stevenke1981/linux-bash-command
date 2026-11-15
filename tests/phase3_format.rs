mod common;
use common::Sandbox;
use std::io::Write;
use std::process::{Command, Output, Stdio};
fn binary(name: &str) -> &'static str {
    match name {
        "fmt" => env!("CARGO_BIN_EXE_fmt"),
        "pr" => env!("CARGO_BIN_EXE_pr"),
        "numfmt" => env!("CARGO_BIN_EXE_numfmt"),
        "ptx" => env!("CARGO_BIN_EXE_ptx"),
        _ => unreachable!(),
    }
}
fn run(s: &Sandbox, name: &str, args: &[&str], bytes: &[u8]) -> Output {
    let mut c = Command::new(binary(name));
    c.args(args).current_dir(&s.path);
    common::execute(c, bytes)
}
fn ok(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}
fn info(name: &str) {
    let s = Sandbox::new();
    for flag in ["--help", "--version"] {
        let out = run(&s, name, &[flag], b"");
        ok(&out);
        assert!(!out.stdout.is_empty());
    }
}

mod fmt {
    use super::*;
    #[test]
    fn information() {
        info("fmt");
    }
    #[test]
    fn invalid_options_and_widths() {
        let s = Sandbox::new();
        for args in [
            vec!["--bogus"],
            vec!["--width=bad"],
            vec!["--width=10", "--goal=11"],
            vec!["--goal=bad", "--help"],
        ] {
            assert_eq!(run(&s, "fmt", &args, b"").status.code(), Some(1));
        }
    }
    #[test]
    fn optimal_breaks_use_a_target_below_the_width() {
        let s = Sandbox::new();
        let out = run(
            &s,
            "fmt",
            &["-20"],
            b"one two three four five six seven eight nine ten\n",
        );
        ok(&out);
        assert_eq!(
            out.stdout,
            b"one two three four\nfive six seven\neight nine ten\n"
        );
    }
    #[test]
    fn spacing_split_prefix_and_blank_lines() {
        let s = Sandbox::new();
        assert_eq!(
            run(
                &s,
                "fmt",
                &["--uniform-spacing", "-w30"],
                b"Hello.   This   is a sentence.\nMore here.\n"
            )
            .stdout,
            b"Hello.  This is a sentence.\nMore here.\n"
        );
        assert_eq!(
            run(&s, "fmt", &["--split-only"], b"one\ntwo\n").stdout,
            b"one\ntwo\n"
        );
        assert_eq!(
            run(
                &s,
                "fmt",
                &["--prefix=> ", "-w20"],
                b"> one two three four five six\n> seven eight nine\npass\n"
            )
            .stdout,
            b"> one two three\n> four five six\n> seven eight nine\npass\n"
        );
    }
    #[test]
    fn crown_and_tagged_indentation() {
        let s = Sandbox::new();
        for flag in ["--crown-margin", "--tagged-paragraph"] {
            let out = run(
                &s,
                "fmt",
                &[flag, "-w20"],
                b"A heading with words\n   body words continue here and more words follow\n",
            );
            ok(&out);
            let lines = out
                .stdout
                .split(|b| *b == b'\n')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            assert!(!lines[0].starts_with(b" "));
            assert!(lines.iter().skip(1).all(|line| line.starts_with(b"   ")));
        }
    }
    #[test]
    fn files_continue_after_missing_and_preserve_raw_bytes() {
        let s = Sandbox::new();
        s.write("a", b"one \xff\n");
        s.write("b", b"two\n");
        let out = run(&s, "fmt", &["missing", "a", "b"], b"");
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(out.stdout, b"one \xff\ntwo\n");
        assert!(!String::from_utf8_lossy(&out.stderr).contains("os error"));
    }
}
mod pr {
    use super::*;
    #[test]
    fn merged_stdin_operands_share_one_buffer() {
        let s = Sandbox::new();
        for args in [
            vec!["-t", "-m", "-", "-"],
            vec!["-t", "-m", "-", "/dev/stdin"],
        ] {
            let out = run(&s, "pr", &args, b"a\nb\nc\nd\n");
            ok(&out);
            assert_eq!(out.stdout, b"a\t\t\t\t    b\nc\t\t\t\t    d\n");
        }
    }
    #[test]
    fn multibyte_optional_characters_are_clean_usage_errors() {
        let s = Sandbox::new();
        for flag in ["-eé", "-né", "-ié"] {
            let out = run(&s, "pr", &["-t", flag], b"a\n");
            assert_eq!(out.status.code(), Some(1));
            assert!(out.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
        }
    }
    #[test]
    fn starting_line_number_rejects_overflow_and_accepts_signed_values() {
        let s = Sandbox::new();
        s.write("in", b"a\nb\n");
        for flag in ["-N18446744073709551615", "-N2147483648", "-N-2147483649"] {
            let out = run(&s, "pr", &["-t", "-m", flag, "in"], b"");
            assert_eq!(out.status.code(), Some(1));
            assert!(out.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
        }
        let out = run(&s, "pr", &["-t", "-N-1", "-n:3", "in"], b"");
        ok(&out);
        assert_eq!(out.stdout, b" -1:a\n  0:b\n");
    }
    #[test]
    fn information() {
        info("pr");
    }
    #[test]
    fn invalid_and_missing_inputs() {
        let s = Sandbox::new();
        for args in [
            vec!["--bogus"],
            vec!["--length=0"],
            vec!["--pages=3:1"],
            vec!["-a", "-m"],
            vec!["missing"],
        ] {
            assert_eq!(run(&s, "pr", &args, b"").status.code(), Some(1));
        }
        let out = run(&s, "pr", &["-r", "missing"], b"");
        assert!(out.stderr.is_empty());
    }
    #[test]
    fn columns_fill_down_or_across() {
        let s = Sandbox::new();
        let data = b"one\ntwo\nthree\nfour\nfive\nsix\nseven\n";
        assert_eq!(
            run(&s, "pr", &["-t", "-3", "-w30"], data).stdout,
            b"one\t  four\t    six\ntwo\t  five\t    seven\nthree\n"
        );
        assert_eq!(
            run(&s, "pr", &["-t", "-3", "-a", "-s:"], data).stdout,
            b"one:two:three\nfour:five:six\nseven\n"
        );
    }
    #[test]
    fn page_headers_ranges_and_formfeeds() {
        let s = Sandbox::new();
        let out = run(
            &s,
            "pr",
            &[
                "-l12",
                "--date-format=DATE",
                "--header=TITLE",
                "--form-feed",
            ],
            b"one\ntwo\nthree\n",
        );
        ok(&out);
        assert!(out.stdout.starts_with(b"\n\nDATE"));
        assert_eq!(out.stdout.iter().filter(|b| **b == 12).count(), 2);
        assert!(out.stdout.windows(6).any(|s| s == b"Page 2"));
        assert_eq!(
            run(
                &s,
                "pr",
                &["-t", "-l2", "+2:2"],
                b"one\ntwo\nthree\nfour\nfive\n"
            )
            .stdout,
            b"three\nfour\n"
        );
    }
    #[test]
    fn tabs_controls_numbers_and_joining() {
        let s = Sandbox::new();
        assert_eq!(run(&s, "pr", &["-t", "-c"], b"a\x01b\n").stdout, b"a^Ab\n");
        assert_eq!(
            run(&s, "pr", &["-T", "-v"], b"a\x0cb\x01\n").stdout,
            b"a\nb\\001\n"
        );
        assert_eq!(
            run(&s, "pr", &["-t", "-n:3", "-N10", "-o2"], b"one\n").stdout,
            b"   10:one\n"
        );
        assert_eq!(
            run(&s, "pr", &["-t", "-2", "-J", "-S|"], b"one\ntwo\n").stdout,
            b"one|two\n"
        );
    }
    #[test]
    fn merge_and_multiple_files() {
        let s = Sandbox::new();
        s.write("a", b"a\nb\nc\n");
        s.write("b", b"1\n2\n");
        let out = run(&s, "pr", &["-t", "-m", "-J", "a", "b"], b"");
        ok(&out);
        assert_eq!(out.stdout, b"a\t1\nb\t2\nc\t\n");
        assert_eq!(
            run(&s, "pr", &["-t", "a", "b"], b"").stdout,
            b"a\nb\nc\n1\n2\n"
        );
    }
}
mod numfmt {
    use super::*;
    #[test]
    fn information() {
        info("numfmt");
    }
    #[test]
    fn empty_input_and_output_units_are_usage_errors() {
        let s = Sandbox::new();
        for flag in ["--from-unit=", "--to-unit="] {
            let out = run(&s, "numfmt", &[flag, "2"], b"");
            assert_eq!(out.status.code(), Some(1));
            assert!(out.stdout.is_empty());
            assert!(String::from_utf8_lossy(&out.stderr).contains("invalid unit size"));
        }
    }
    #[test]
    fn invalid_options_formats_and_fields() {
        let s = Sandbox::new();
        for args in [
            vec!["--bogus"],
            vec!["--format=%q", "1"],
            vec!["--field=3-1", "1"],
            vec!["--from=bad", "--help"],
        ] {
            assert_eq!(run(&s, "numfmt", &args, b"").status.code(), Some(1));
        }
    }
    #[test]
    fn scaling_units_suffixes_and_rounding() {
        let s = Sandbox::new();
        assert_eq!(
            run(&s, "numfmt", &["--from=auto", "1K", "1Ki", "1.25M"], b"").stdout,
            b"1000\n1024\n1250000\n"
        );
        assert_eq!(
            run(&s, "numfmt", &["--to=iec-i", "4096"], b"").stdout,
            b"4.0Ki\n"
        );
        assert_eq!(
            run(
                &s,
                "numfmt",
                &["--to-unit=2", "--round=nearest", "--", "3", "-3"],
                b""
            )
            .stdout,
            b"2\n-2\n"
        );
    }
    #[test]
    fn ronna_quetta_and_binary_equivalents_round_trip() {
        let s = Sandbox::new();
        for (value, scale, expected) in [
            (1000i128.pow(9), "si", "1.0R\n"),
            (1000i128.pow(10), "si", "1.0Q\n"),
            (1024i128.pow(9), "iec", "1.1R\n"),
            (1024i128.pow(10), "iec-i", "1.1Qi\n"),
        ] {
            let value = value.to_string();
            let flag = format!("--to={scale}");
            let out = run(&s, "numfmt", &[&flag, &value], b"");
            ok(&out);
            assert_eq!(out.stdout, expected.as_bytes());
        }
        for (from, to, value, expected) in [
            ("si", "si", "1R", "1.0R\n"),
            ("si", "si", "1Q", "1.0Q\n"),
            ("iec-i", "iec-i", "1Ri", "1.0Ri\n"),
            ("auto", "iec-i", "1Qi", "1.0Qi\n"),
        ] {
            let out = run(
                &s,
                "numfmt",
                &[&format!("--from={from}"), &format!("--to={to}"), value],
                b"",
            );
            ok(&out);
            assert_eq!(out.stdout, expected.as_bytes());
        }
    }
    #[test]
    fn headers_and_selected_fields() {
        let s = Sandbox::new();
        let out = run(
            &s,
            "numfmt",
            &["--header", "--field=2-3", "--delimiter=,", "--to=si"],
            b"name,size,other\nx,1000,2000\n",
        );
        ok(&out);
        assert_eq!(out.stdout, b"name,size,other\nx,1.0K,2.0K\n");
    }
    #[test]
    fn invalid_modes_and_partial_abort() {
        let s = Sandbox::new();
        for (mode, status) in [("abort", 2), ("fail", 2), ("warn", 0), ("ignore", 0)] {
            let out = run(&s, "numfmt", &[&format!("--invalid={mode}")], b"bad\n3\n");
            assert_eq!(out.status.code(), Some(status));
            assert_eq!(
                out.stdout,
                if mode == "abort" {
                    b"".as_slice()
                } else {
                    b"bad\n3\n"
                }
            );
            assert_eq!(out.stderr.is_empty(), mode == "ignore");
        }
        assert_eq!(
            run(&s, "numfmt", &["--from=si", "--field=1-"], b"1K BAD 2K\n").stdout,
            b"1000 "
        );
    }
    #[test]
    fn formats_padding_suffix_and_nul() {
        let s = Sandbox::new();
        assert_eq!(
            run(&s, "numfmt", &["--format=x%08.2fZ", "--to=si", "1234"], b"").stdout,
            b"x00001.24KZ\n"
        );
        assert_eq!(
            run(
                &s,
                "numfmt",
                &["--padding=-8", "--suffix=B", "--to=si", "1000"],
                b""
            )
            .stdout,
            b"1.0KB   \n"
        );
        assert_eq!(
            run(&s, "numfmt", &["-z", "--to=si"], b"1000\0").stdout,
            b"1.0K\0"
        );
    }
}
mod ptx {
    use super::*;
    #[test]
    fn information() {
        info("ptx");
    }
    #[test]
    fn invalid_and_missing_inputs() {
        let s = Sandbox::new();
        for args in [
            vec!["--bogus"],
            vec!["-w0"],
            vec!["-W["],
            vec!["-S.*"],
            vec!["missing"],
        ] {
            assert_eq!(run(&s, "ptx", &args, b"").status.code(), Some(1));
        }
    }
    #[test]
    fn default_index_sorts_keywords_with_sentence_context() {
        let s = Sandbox::new();
        let out = run(&s, "ptx", &[], b"one two\n");
        ok(&out);
        assert_eq!(out.stdout,b"                                       one two\n                                 one   two\n");
    }
    #[test]
    fn filters_and_word_sentence_regexes() {
        let s = Sandbox::new();
        s.write("ignore", b"two\n");
        s.write("only", b"one\n");
        for args in [
            vec!["--ignore-file=ignore"],
            vec!["--only-file=only"],
            vec!["--word-regexp=one"],
        ] {
            let out = run(&s, "ptx", &args, b"one two\n");
            ok(&out);
            assert_eq!(out.stdout.iter().filter(|b| **b == b'\n').count(), 1);
        }
        let out = run(&s, "ptx", &["--sentence-regexp=\\n"], b"one\ntwo\n");
        assert!(!out.stdout.windows(7).any(|s| s == b"one two"));
    }
    #[test]
    fn roff_and_tex_goldens() {
        let s = Sandbox::new();
        assert_eq!(
            run(&s, "ptx", &["-O", "-Mindex"], b"one two\n").stdout,
            b".index \"\" \"\" \"one two\" \"\"\n.index \"\" \"one\" \"two\" \"\"\n"
        );
        assert_eq!(
            run(&s, "ptx", &["-T"], b"one two\n").stdout,
            b"\\xx {}{}{one}{ two}{}\n\\xx {}{one}{two}{}{}\n"
        );
    }
    #[test]
    fn references_truncation_and_traditional_output_file() {
        let s = Sandbox::new();
        let out = run(
            &s,
            "ptx",
            &["-r", "-w20", "-F!"],
            b"REF one two three four five six\n",
        );
        ok(&out);
        assert!(out.stdout.contains(&b'!'));
        assert!(out.stdout.starts_with(b"REF"));
        s.write("a", b"one two\n");
        let out = run(&s, "ptx", &["-G", "a", "out"], b"");
        ok(&out);
        assert!(out.stdout.is_empty());
        assert!(std::fs::read(s.path.join("out"))
            .unwrap()
            .starts_with(b".xx"));
    }
}
#[test]
fn all_formatting_commands_handle_closed_stdout() {
    let s = Sandbox::new();
    for name in ["fmt", "pr", "numfmt", "ptx"] {
        for args in [vec!["--help"], vec!["--version"], vec![]] {
            let (reader, writer) = std::io::pipe().unwrap();
            drop(reader);
            let mut command = Command::new(binary(name));
            command
                .args(args)
                .current_dir(&s.path)
                .stdin(Stdio::piped())
                .stdout(writer)
                .stderr(Stdio::piped());
            let mut child = command.spawn().unwrap();
            let data = if name == "numfmt" {
                b"1000\n".as_slice()
            } else {
                b"one two\n"
            };
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(data);
            }
            let out = child.wait_with_output().unwrap();
            assert_eq!(out.status.code(), Some(0), "{name}: {out:?}");
            assert!(out.stderr.is_empty(), "{name}: {out:?}");
        }
    }
}
