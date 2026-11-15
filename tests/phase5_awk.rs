use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[test]
fn incomplete_regex_intervals_match_literal_braces() {
    let s = Sandbox::new();
    success(
        s.run(
            &[r#"BEGIN{print match("a{b",/a{b/),match("a{",/a{/),match("a}",/a}/),match("a{,2}",/a{,2}/),match("a{q}",/a{q}/)}"#],
            b"",
        ),
        b"1 1 1 1 1\n",
    );
    error(s.run(&["BEGIN{print /a{2,/}"], b""), 2, "syntax error");
    error(s.run(&["BEGIN{print /a{}/}"], b""), 2, "syntax error");
}
#[test]
fn ubuntu_reference_format_and_assignment_edges() {
    let s = Sandbox::new();
    success(
        s.run(
            &["BEGIN{printf \"[%5.0d][%#.0o][%#.0x][%#.0X][%.0x]\\n\",0,0,0,0,0}"],
            b"",
        ),
        b"[     ][0][0][0][]\n",
    );
    for args in [
        ["-v", "x=a\\", "BEGIN{printf \"[%s]\\n\",x}"].as_slice(),
        ["END{printf \"[%s]\\n\",x}", "x=a\\"].as_slice(),
    ] {
        success(s.run(args, b""), b"[a]\n");
    }
}
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentcmd-awk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, bytes: &[u8]) {
        fs::write(self.0.join(name), bytes).unwrap();
    }
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).unwrap()
    }
    fn run(&self, args: &[&str], input: &[u8]) -> Output {
        run(&self.0, args, input)
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn command() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_awk"));
    c.env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env_remove("POSIXLY_CORRECT");
    c
}
fn run(path: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = command()
        .args(args)
        .current_dir(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    output
}
fn success(output: Output, expected: &[u8]) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(output.stdout, expected);
    assert!(output.stderr.is_empty(), "{output:?}");
}
fn error(output: Output, status: i32, expected: &str) {
    assert_eq!(output.status.code(), Some(status), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "{output:?}"
    );
}
fn run_limited(path: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = command()
        .args(args)
        .current_dir(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let bytes = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&bytes);
    });
    let mut stdout = child.stdout.take().unwrap();
    let output = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut expired = false;
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            expired = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let status = child.wait().unwrap();
    writer.join().unwrap();
    let stdout = output.join().unwrap();
    let stderr = errors.join().unwrap();
    assert!(
        !expired,
        "awk exceeded three-second regression deadline: {}",
        String::from_utf8_lossy(&stderr)
    );
    Output {
        status,
        stdout,
        stderr,
    }
}

#[test]
fn help_and_version() {
    let s = Sandbox::new();
    let out = s.run(&["--help"], b"");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("length counts UTF-8"));
    success(
        s.run(&["--version"], b""),
        format!("awk (agentcmd) {}\n", env!("CARGO_PKG_VERSION")).as_bytes(),
    );
}
#[test]
fn usage_errors() {
    let s = Sandbox::new();
    error(s.run(&["--unknown"], b""), 1, "unrecognized option");
    error(s.run(&["-v", "bad", "BEGIN{}"], b""), 1, "VAR=VALUE");
    error(s.run(&[], b""), 1, "no program");
}
#[test]
fn syntax_errors_are_status_two() {
    let s = Sandbox::new();
    for script in ["BEGIN{print (}", "BEGIN {x=}", "BEGIN {break}"] {
        error(s.run(&[script], b""), 2, "syntax error at source line");
    }
}
#[test]
fn missing_input_and_source_errors() {
    let s = Sandbox::new();
    error(
        s.run(&["{print}", "missing"], b""),
        2,
        "No such file or directory",
    );
    error(s.run(&["-f", "missing"], b""), 2, "source file");
}
#[test]
fn stdin_and_binary_data() {
    let s = Sandbox::new();
    success(
        s.run(&["{print NF,$1}"], b"\xff x\nlast"),
        b"2 \xff\n1 last\n",
    );
}
#[test]
fn cr_bytes_are_preserved() {
    let s = Sandbox::new();
    success(
        s.run(&["{print NF,$1,$2}"], b"a\rb\tc\r\n"),
        b"2 a\rb c\r\n",
    );
}
#[test]
fn length_decodes_utf8_and_invalid_bytes() {
    let s = Sandbox::new();
    success(
        s.run(&["{print length($0)}"], "a\u{732b}\n".as_bytes()),
        b"2\n",
    );
    success(s.run(&["{print length($0)}"], b"a\xff\xc3(\n"), b"4\n");
}
#[test]
fn field_option_and_assignments() {
    let s = Sandbox::new();
    success(
        s.run(
            &[
                "-F,",
                "-vx=001",
                "-v",
                "y=2",
                "BEGIN{print x,x==1,y}{print NF,$2}",
            ],
            b"a,b\n",
        ),
        b"001 1 2\n2 b\n",
    );
}
#[test]
fn program_files_are_concatenated() {
    let s = Sandbox::new();
    s.file("one", b"BEGIN{x=1}\n");
    s.file("two", b"{print x,$1}\n");
    success(s.run(&["-f", "one", "-f", "two"], b"a\n"), b"1 a\n");
}
#[test]
fn positional_assignments_follow_input_order() {
    let s = Sandbox::new();
    s.file("a", b"first\n");
    s.file("b", b"second\n");
    success(
        s.run(
            &[
                "BEGIN{print x}{print x,$0}END{print x}",
                "x=1",
                "a",
                "x=2",
                "b",
                "x=3",
            ],
            b"",
        ),
        b"\n1 first\n2 second\n3\n",
    );
}
#[test]
fn argv_can_select_and_append_files() {
    let s = Sandbox::new();
    s.file("a", b"one\n");
    s.file("b", b"two\n");
    success(
        s.run(
            &[
                "BEGIN{ARGV[1]=\"b\";ARGV[ARGC++]=\"a\"}{print FILENAME,FNR,NR,$0}",
                "a",
            ],
            b"",
        ),
        b"b 1 1 two\na 1 2 one\n",
    );
}
#[test]
fn field_and_record_mutations() {
    let s = Sandbox::new();
    success(
        s.run(&["{OFS=\":\";$4=5;print NF,$0;NF=1;print $0}"], b"a b\n"),
        b"4:a:b::5\na\n",
    );
}
#[test]
fn numeric_string_attributes() {
    let s = Sandbox::new();
    success(
        s.run(&["{print ($1<2),($1==1),($1?1:0),$1}"], b"001\n10\n0\n"),
        b"1 1 1 001\n0 0 1 10\n1 0 0 0\n",
    );
    success(
        s.run(
            &["BEGIN{print (\"10\"<2),(\"0\"?1:0);$0=\"0\";print ($0?1:0)}"],
            b"",
        ),
        b"1 1\n1\n",
    );
}
#[test]
fn arithmetic_assignment_and_power_extensions() {
    let s = Sandbox::new();
    success(
        s.run(&["BEGIN{x=2;x**=3;print x,2**4,-2^2}"], b""),
        b"8 16 -4\n",
    );
}
#[test]
fn arrays_multidimensional_delete_and_iteration() {
    let s = Sandbox::new();
    success(s.run(&["BEGIN{a[1,2]=3;a[2,3]=4;for(k in a)s+=a[k];print s,((1,2)in a);delete a;print length(a)}"],b""),b"7 1\n0\n");
}
#[test]
fn functions_copy_scalars_and_share_arrays() {
    let s = Sandbox::new();
    success(s.run(&["function set(a,x){a[1]=9;x=8} function fact(n){return n<2?1:n*fact(n-1)} BEGIN{x=1;set(a,x);print a[1],x,fact(8)}"],b""),b"9 1 40320\n");
}
#[test]
fn loops_break_continue_and_next() {
    let s = Sandbox::new();
    success(
        s.run(
            &["{if(NR==2)next;for(i=0;i<4;i++){if(i==1)continue;if(i==3)break;s+=i}} END{print s}"],
            b"a\nb\nc\n",
        ),
        b"4\n",
    );
}
#[test]
fn ranges_and_nextfile() {
    let s = Sandbox::new();
    s.file("a", b"a\nb\nc\n");
    s.file("b", b"d\ne\n");
    success(
        s.run(&["{print FILENAME,$0;nextfile}", "a", "b"], b""),
        b"a a\nb d\n",
    );
    success(s.run(&["/b/,/c/"], b"a\nb\nc\nd\n"), b"b\nc\n");
}
#[test]
fn main_getline_updates_counters() {
    let s = Sandbox::new();
    success(
        s.run(
            &["{r=getline x;print r,NR,NF,$0,\"[\"x\"]\"}"],
            b"a 1\nb 2\nc 3\n",
        ),
        b"1 2 2 a 1 [b 2]\n0 3 2 c 3 [b 2]\n",
    );
}
#[test]
fn redirected_getline_and_close() {
    let s = Sandbox::new();
    s.file("data", b"one 1\ntwo 2\n");
    success(s.run(&["BEGIN{getline x<\"data\";print x,NR;close(\"data\");getline<\"data\";print NF,$1,NR;print (getline x<\"missing\")}"],b""),b"one 1 0\n2 one 0\n-1\n");
}
#[test]
fn regex_and_paragraph_record_separators() {
    let s = Sandbox::new();
    success(
        s.run(&["BEGIN{RS=\":+\"}{print NR,\"[\"$0\"]\"}"], b"a::b:::c"),
        b"1 [a]\n2 [b]\n3 [c]\n",
    );
    success(
        s.run(
            &["BEGIN{RS=\"\";FS=\",\"}{print NF,$1,$2,$3}"],
            b"\n\na,b\nc\n\n\nd,e\n",
        ),
        b"3 a b c\n2 d e \n",
    );
}
#[test]
fn output_redirections_share_handles_until_close() {
    let s = Sandbox::new();
    success(s.run(&["BEGIN{print \"one\">\"out\";print \"two\">>\"out\";print fflush(\"out\");print close(\"out\");print close(\"out\")}"],b""),b"0\n0\n-1\n");
    assert_eq!(s.read("out"), b"one\ntwo\n");
}
#[test]
fn substitution_match_and_split() {
    let s = Sandbox::new();
    success(s.run(&["BEGIN{s=\"ab\";print gsub(/a*/,\"X\",s),s;print match(\"zabx\",/a|ab/),RSTART,RLENGTH;print split(\"a::b\",a,/:+/),a[2]}"],b""),b"2 XbX\n2 2 2\n2 b\n");
}
#[test]
fn printf_and_conversion_formats() {
    let s = Sandbox::new();
    success(s.run(&["BEGIN{CONVFMT=\"%.2f\";OFMT=\"%.1f\";x=1.234;print x,x\"\";printf \"[%#08x] %.2f %a\\n\",10,x,1.5}"],b""),b"1.2 1.23\n[0x00000a] 1.23 0x1.8p+0\n");
}
#[test]
fn math_random_and_string_builtins() {
    let s = Sandbox::new();
    success(s.run(&["BEGIN{print sin(0),cos(0),atan2(0,1),exp(0),log(1),sqrt(9),int(-2.9);srand(1);x=rand();srand(1);print x==rand();print substr(\"abc\",2),index(\"abc\",\"b\"),toupper(\"ab\"),tolower(\"CD\")}"],b""),b"0 1 0 1 0 3 -2\n1\nbc 2 AB cd\n");
}
#[test]
fn unsupported_shell_features_fail_when_executed() {
    let s = Sandbox::new();
    for script in [
        "BEGIN{system(\"anything\")}",
        "BEGIN{\"anything\"|getline x}",
        "BEGIN{print 1|\"anything\"}",
    ] {
        error(s.run(&[script], b""), 2, "not supported by agentcmd");
    }
    success(
        s.run(&["BEGIN{if(0)system(\"anything\");print 1}"], b""),
        b"1\n",
    );
}
#[test]
fn exit_runs_end_and_preserves_status() {
    let s = Sandbox::new();
    let out = s.run(&["{print;exit 7} END{print \"end\",NR}"], b"a\nb\n");
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(out.stdout, b"a\nend 1\n");
    assert!(out.stderr.is_empty());
}
#[test]
fn runtime_type_and_arithmetic_errors() {
    let s = Sandbox::new();
    for script in [
        "BEGIN{x=1/0}",
        "BEGIN{a[1]=1;print a}",
        "BEGIN{x=1;x[1]=2}",
        "BEGIN{NF=-1}",
    ] {
        error(s.run(&[script], b""), 2, "");
    }
}
#[test]
fn closed_stdout_is_quiet_success() {
    let s = Sandbox::new();
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut child = command()
        .arg("{print}")
        .current_dir(&s.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"a\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
}
#[test]
fn fflush_emits_a_record_without_waiting_for_eof() {
    let s = Sandbox::new();
    let mut child = command()
        .arg("{print;fflush()}")
        .current_dir(&s.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"first\n").unwrap();
    stdin.flush().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut bytes = [0; 6];
        let result = stdout.read_exact(&mut bytes).map(|()| bytes);
        let _ = sender.send(result);
    });
    let result = receiver.recv_timeout(std::time::Duration::from_secs(3));
    drop(stdin);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();
    assert_eq!(result.unwrap().unwrap(), *b"first\n");
    assert!(status.success());
}
#[test]
fn large_field_sum_streams_records() {
    let s = Sandbox::new();
    let input = b"a b 3\n".repeat(100_000);
    success(s.run(&["{s+=$3} END{print s}"], &input), b"300000\n");
}
#[test]
fn rebuild_timing_and_pattern_control_flow() {
    let s = Sandbox::new();
    success(
        s.run(
            &["{OFS=\":\";$1=1.25;OFS=\"!\";CONVFMT=\"%.3f\";print $0}"],
            b"a b\n",
        ),
        b"1.250:b\n",
    );
    success(
        s.run(
            &["function skip(){next} skip(){print \"bad\"} END{print NR}"],
            b"a\nb\n",
        ),
        b"2\n",
    );
}
#[test]
fn invalid_regex_is_rejected_even_in_unexecuted_literal() {
    let s = Sandbox::new();
    error(s.run(&["BEGIN{if(0)print /[/}"], b""), 2, "syntax error");
    error(
        s.run(&["BEGIN{p=\"[\";print (\"a\"~p)}"], b""),
        2,
        "invalid",
    );
}
#[test]
fn wide_formats_are_not_artificially_truncated() {
    let s = Sandbox::new();
    success(
        s.run(&["BEGIN{print length(sprintf(\"%1000001s\",\"x\"))}"], b""),
        b"1000001\n",
    );
}
#[test]
fn runaway_recursion_has_a_clean_resource_error() {
    let s = Sandbox::new();
    let out = s.run(&["function f(n){return n?f(n-1):0} BEGIN{f(1100)}"], b"");
    error(out.clone(), 2, "function call nesting limit exceeded");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("thread"));
}
#[test]
fn stdout_redirection_is_ordered_with_main_output_and_reopening() {
    let s = Sandbox::new();
    success(s.run(&["BEGIN{print \"A\";print \"B\">\"/dev/stdout\";print fflush(\"/dev/stdout\");print close(\"/dev/stdout\");print \"C\";print \"D\">>\"/dev/stdout\";print close(\"/dev/stdout\");print \"E\"}"],b""),b"A\nB\n0\n0\nC\nD\n0\nE\n");
}
#[test]
fn longest_matching_and_regex_record_growth_are_bounded() {
    let s = Sandbox::new();
    let mut data = vec![b'a'];
    data.extend(std::iter::repeat_n(b'x', 20_000));
    data.push(b'\n');
    let mut expected = data.clone();
    expected[0] = b'X';
    success(
        run_limited(&s.0, &["{sub(/a.*b|a/,\"X\");print}"], &data),
        &expected,
    );
    let mut data = vec![b'a'];
    data.extend(std::iter::repeat_n(b'x', 70_000));
    success(
        run_limited(&s.0, &["BEGIN{RS=\"a.*b|a\"}{print NR,length($0)}"], &data),
        b"1 0\n2 70000\n",
    );
}
#[test]
fn chunked_pipe_waits_for_an_extendable_regex_delimiter() {
    use std::io::BufRead;
    let s = Sandbox::new();
    let mut child = command()
        .arg("BEGIN{RS=\"a.*b|a\";print \"ready\";fflush()}{print NR,length($0);fflush()}")
        .current_dir(&s.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let output = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        let mut all = Vec::new();
        loop {
            let mut line = Vec::new();
            if reader.read_until(b'\n', &mut line).unwrap() == 0 {
                break;
            }
            all.extend_from_slice(&line);
            let _ = sender.send(line);
        }
        all
    });
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let ready = receiver.recv_timeout(std::time::Duration::from_secs(3));
    let first = stdin.write_all(b"ZaX").and_then(|()| stdin.flush());
    let early = receiver.recv_timeout(std::time::Duration::from_millis(200));
    let last = stdin.write_all(b"bQ");
    drop(stdin);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut expired = false;
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            expired = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let status = child.wait().unwrap();
    let stdout = output.join().unwrap();
    let stderr = errors.join().unwrap();
    assert!(!expired);
    assert_eq!(ready.unwrap(), b"ready\n");
    assert!(first.is_ok() && last.is_ok());
    assert!(
        early.is_err(),
        "delimiter committed before longer branch was known"
    );
    success(
        Output {
            status,
            stdout,
            stderr,
        },
        b"ready\n1 1\n2 1\n",
    );
}
#[test]
fn oversized_regex_has_a_clean_compilation_budget_error() {
    let s = Sandbox::new();
    error(
        run_limited(
            &s.0,
            &["BEGIN{p=\"a{100000000}\";print match(\"a\",p)}"],
            b"",
        ),
        2,
        "compilation budget",
    );
}
