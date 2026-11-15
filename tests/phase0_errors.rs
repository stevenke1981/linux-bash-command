mod common;

use common::{binary, Sandbox};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn existing_command_versions_are_available_directly_and_through_multicall() {
    let sandbox = Sandbox::new();
    for name in [
        "cat", "head", "tail", "ls", "pwd", "find", "df", "ps", "less", "curl", "cd",
    ] {
        for (command, args) in [
            (name, vec!["--version"]),
            ("agentcmd", vec![name, "--version"]),
        ] {
            let output = sandbox.run(command, &args, b"");
            assert_eq!(
                output.status.code(),
                Some(0),
                "{command} {args:?}: {}",
                stderr(&output)
            );
            assert_eq!(
                output.stdout,
                format!("{name} (agentcmd) {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
            );
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn help_and_version_exit_quietly_when_stdout_pipe_is_already_closed() {
    let sandbox = Sandbox::new();
    for name in [
        "cat", "head", "tail", "ls", "pwd", "find", "df", "ps", "less", "curl", "cd",
    ] {
        for flag in ["--help", "--version"] {
            // Close the reader before spawning, making this independent of scheduling
            // even when the information output fits in a single small write.
            let (reader, writer) = std::io::pipe().unwrap();
            drop(reader);
            let output = Command::new(binary(name))
                .current_dir(&sandbox.path)
                .arg(flag)
                .stdin(Stdio::null())
                .stdout(writer)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(0),
                "{name} {flag}: {}",
                stderr(&output)
            );
            assert!(
                output.stderr.is_empty(),
                "{name} {flag}: {}",
                stderr(&output)
            );
        }
    }
}

#[test]
fn help_and_version_return_one_for_other_output_errors() {
    let sandbox = Sandbox::new();
    sandbox.write("read-only-handle", b"original");
    for name in [
        "cat", "head", "tail", "ls", "pwd", "find", "df", "ps", "less", "curl", "cd",
    ] {
        for flag in ["--help", "--version"] {
            let (stdout, reader) = failing_stdout(&sandbox);
            let output = Command::new(binary(name))
                .current_dir(&sandbox.path)
                .arg(flag)
                .stdin(Stdio::null())
                .stdout(stdout)
                .output()
                .unwrap();
            // Keep a Unix pipe's reader alive until the child has exited, so the
            // failure stays WouldBlock instead of becoming BrokenPipe.
            drop(reader);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{name} {flag}: {}",
                stderr(&output)
            );
            assert!(
                stderr(&output).starts_with(&format!("{name}: write error: ")),
                "{}",
                stderr(&output)
            );
            assert!(!stderr(&output).contains("os error"));
        }
    }
    assert_eq!(
        std::fs::read(sandbox.path.join("read-only-handle")).unwrap(),
        b"original"
    );
}

#[cfg(not(unix))]
fn failing_stdout(sandbox: &Sandbox) -> (Stdio, Option<std::io::PipeReader>) {
    let file = std::fs::File::open(sandbox.path.join("read-only-handle")).unwrap();
    (file.into(), None)
}

#[cfg(unix)]
fn failing_stdout(_sandbox: &Sandbox) -> (Stdio, Option<std::io::PipeReader>) {
    use std::os::fd::AsRawFd;

    let (reader, mut writer) = std::io::pipe().unwrap();
    let descriptor = writer.as_raw_fd();
    // SAFETY: descriptor belongs to the live writer; F_GETFL takes no argument.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    assert_ne!(flags, -1, "{}", std::io::Error::last_os_error());
    // SAFETY: F_SETFL accepts these flags for the same live pipe descriptor.
    assert_ne!(
        unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) },
        -1,
        "{}",
        std::io::Error::last_os_error()
    );
    // Fill quickly, then consume every remaining byte of capacity. A large write
    // can return WouldBlock while a small information message would still fit.
    for bytes in [&[0u8; 65536][..], &[0u8; 1][..]] {
        loop {
            match writer.write(bytes) {
                Ok(count) => assert_ne!(count, 0),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("fill nonblocking stdout pipe: {error}"),
            }
        }
    }
    (writer.into(), Some(reader))
}

#[test]
fn ordinary_errors_have_the_required_status_and_normalized_os_messages() {
    let sandbox = Sandbox::new();
    for (name, args, code) in [
        ("cat", vec!["missing"], 1),
        ("head", vec!["missing"], 1),
        ("tail", vec!["missing"], 1),
        ("ls", vec!["missing"], 2),
        ("find", vec!["missing"], 1),
        ("df", vec!["missing"], 1),
        ("less", vec!["missing"], 1),
        ("cd", vec!["missing"], 1),
    ] {
        let output = sandbox.run(name, &args, b"");
        assert_eq!(
            output.status.code(),
            Some(code),
            "{name}: {}",
            stderr(&output)
        );
        let error = stderr(&output);
        assert!(error.starts_with(&format!("{name}: ")), "{error}");
        assert!(error.contains("No such file or directory"), "{error}");
        assert!(!error.contains("os error"), "{error}");
    }
}

#[test]
fn usage_errors_have_gnu_option_wording_and_the_shared_help_hint() {
    let sandbox = Sandbox::new();
    for (name, code) in [
        ("cat", 1),
        ("head", 1),
        ("tail", 1),
        ("ls", 2),
        ("pwd", 1),
        ("df", 1),
        ("ps", 1),
        ("less", 1),
        ("curl", 2),
    ] {
        let output = sandbox.run(name, &["--bogus"], b"");
        assert_eq!(
            output.status.code(),
            Some(code),
            "{name}: {}",
            stderr(&output)
        );
        assert_eq!(stderr(&output), format!("{name}: unrecognized option '--bogus'\nTry '{name} --help' for more information.\n"));
    }
    let find = sandbox.run("find", &["-bogus"], b"");
    assert_eq!(find.status.code(), Some(1));
    assert!(stderr(&find).ends_with("Try 'find --help' for more information.\n"));
}

#[test]
fn input_commands_continue_after_a_missing_operand() {
    let sandbox = Sandbox::new();
    sandbox.write("sample", b"still read\n");
    for name in ["cat", "head", "tail", "less"] {
        let output = sandbox.run(name, &["missing", "sample"], b"");
        assert_eq!(output.status.code(), Some(1), "{name}: {}", stderr(&output));
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("still read\n"),
            "{name}: {:?}",
            output.stdout
        );
    }
}

#[test]
fn required_values_consume_info_flags_instead_of_exiting_successfully() {
    let sandbox = Sandbox::new();
    for (name, option, code) in [
        ("head", "-n", 1),
        ("tail", "--lines", 1),
        ("df", "-B", 1),
        ("ps", "--pid", 1),
        ("curl", "--output", 2),
    ] {
        let output = sandbox.run(name, &[option, "--version"], b"");
        assert_eq!(
            output.status.code(),
            Some(code),
            "{name}: {}",
            stderr(&output)
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn remaining_long_options_expand_unique_prefixes_and_reject_ambiguous_ones() {
    let sandbox = Sandbox::new();
    sandbox.write("sample", b"body\n");
    for (name, args, full) in [
        ("pwd", vec!["--phys"], vec!["--physical"]),
        ("df", vec!["--human-r", "."], vec!["--human-readable", "."]),
        (
            "ps",
            vec!["--no-header", "--pid", "4294967295"],
            vec!["--no-headers", "--pid", "4294967295"],
        ),
        (
            "less",
            vec!["--LINE-N", "sample"],
            vec!["--LINE-NUMBERS", "sample"],
        ),
        ("curl", vec!["--vers"], vec!["--version"]),
    ] {
        let actual = sandbox.run(name, &args, b"");
        let expected = sandbox.run(name, &full, b"");
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{name}: {}",
            stderr(&actual)
        );
        assert_eq!(actual.stdout, expected.stdout, "{name}");
    }
    for name in [
        "cat", "head", "tail", "ls", "pwd", "df", "ps", "less", "curl",
    ] {
        let output = sandbox.run(name, &["--"], b"");
        assert!(!stderr(&output).contains("ambiguous"));
    }
    let ambiguous = sandbox.run("curl", &["--he"], b"");
    assert_eq!(ambiguous.status.code(), Some(2));
    assert!(stderr(&ambiguous).contains("ambiguous; possibilities: '--head' '--header' '--help'"));
}

#[test]
fn posix_mode_stops_option_permutation_in_remaining_parsers() {
    let sandbox = Sandbox::new();
    sandbox.write("sample", b"body\n");
    sandbox.write("--version", b"second\n");
    let less = Command::new(binary("less"))
        .current_dir(&sandbox.path)
        .env("POSIXLY_CORRECT", "1")
        .args(["sample", "--version"])
        .output()
        .unwrap();
    assert_eq!(less.status.code(), Some(0));
    assert_eq!(less.stdout, b"body\nsecond\n");
    let df = Command::new(binary("df"))
        .current_dir(&sandbox.path)
        .env("POSIXLY_CORRECT", "1")
        .args(["sample", "--not-an-option"])
        .output()
        .unwrap();
    assert_eq!(df.status.code(), Some(1));
    assert!(stderr(&df).contains("No such file or directory"));
    let ps = Command::new(binary("ps"))
        .current_dir(&sandbox.path)
        .env("POSIXLY_CORRECT", "1")
        .args(["aux", "--version"])
        .output()
        .unwrap();
    assert_eq!(ps.status.code(), Some(1));
    assert!(ps.stdout.is_empty());
}

fn curl(sandbox: &Sandbox, args: &[&str]) -> Output {
    Command::new(binary("curl"))
        .current_dir(&sandbox.path)
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .env_remove("POSIXLY_CORRECT")
        .args(args)
        .output()
        .unwrap()
}

/// One loopback request, with a bounded accept wait so a client failure cannot hang the test.
fn server(
    response: &'static [u8],
    delay_before: bool,
    delay_after: bool,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let task = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("local server did not receive a request: {error}"),
            }
        };
        // Windows may inherit the listener's nonblocking mode on accepted sockets.
        // Reading partial headers as EOF would leave unread request bytes and can
        // turn the eventual close into a reset instead of a complete response.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
        }
        if delay_before {
            thread::sleep(Duration::from_millis(400));
        }
        if delay_before || delay_after {
            // Timeout clients intentionally close the connection before delivery.
            reader.get_mut().write_all(response).ok();
            reader.get_mut().flush().ok();
        } else {
            reader
                .get_mut()
                .write_all(response)
                .expect("write complete local HTTP response");
            reader.get_mut().flush().expect("flush local HTTP response");
            reader
                .get_mut()
                .shutdown(Shutdown::Write)
                .expect("finish local HTTP response with FIN");
            // Keep the read side alive until the client observes the FIN and closes.
            // Dropping immediately after writing can reset a Windows connection.
            let mut trailing = Vec::new();
            if let Err(error) = reader.read_to_end(&mut trailing) {
                assert!(
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ),
                    "read client close: {error}"
                );
            }
        }
        if delay_after {
            thread::sleep(Duration::from_millis(400));
        }
    });
    (format!("http://{address}/"), task)
}

#[test]
fn curl_failure_statuses_use_local_sockets_and_temporary_files() {
    let sandbox = Sandbox::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let refused = curl(
        &sandbox,
        &["--connect-timeout", "5", &format!("http://{address}/")],
    );
    assert_eq!(refused.status.code(), Some(7), "{}", stderr(&refused));

    for (response, before, after) in [
        (
            &b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx"[..],
            true,
            false,
        ),
        (
            &b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n"[..],
            false,
            true,
        ),
    ] {
        let (url, task) = server(response, before, after);
        let timeout = curl(&sandbox, &["-m", "0.1", &url]);
        task.join().unwrap();
        assert_eq!(timeout.status.code(), Some(28), "{}", stderr(&timeout));
    }
    let (url, task) = server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx",
        false,
        false,
    );
    let write = curl(&sandbox, &["-o", ".", &url]);
    task.join().unwrap();
    assert_eq!(write.status.code(), Some(23), "{}", stderr(&write));

    let (url, task) = server(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 1\r\nConnection: close\r\n\r\nx",
        false,
        false,
    );
    let http = curl(&sandbox, &["-f", &url]);
    task.join().unwrap();
    assert_eq!(http.status.code(), Some(22), "{}", stderr(&http));
    assert!(http.stdout.is_empty());
    assert_eq!(curl(&sandbox, &["--bogus"]).status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn ls_distinguishes_inaccessible_operands_from_nested_directory_errors() {
    use std::os::unix::fs::PermissionsExt;
    let sandbox = Sandbox::new();
    let nested = sandbox.path.join("parent/blocked");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o0)).unwrap();
    // Restore permissions before assertions so cleanup also works on failure.
    let operand = sandbox.run("ls", &["parent/blocked"], b"");
    let recursive = sandbox.run("ls", &["-R", "parent"], b"");
    std::fs::set_permissions(&nested, std::fs::Permissions::from_mode(0o755)).unwrap();
    if operand.status.success() {
        return;
    } // A privileged test process bypasses permissions.
    assert_eq!(operand.status.code(), Some(2), "{}", stderr(&operand));
    assert_eq!(recursive.status.code(), Some(1), "{}", stderr(&recursive));
}
