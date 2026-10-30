//! Integration tests for the `curl` compatibility command.
//! Every request goes to a local `TcpListener` server started by the test itself.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-curl-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &[u8]) {
        fs::write(self.0.join(name), contents).unwrap();
    }

    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.0.join(name)).unwrap()
    }

    fn run(&self, args: &[&str], stdin: &[u8]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_curl"))
            .current_dir(&self.0)
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .env_remove("HTTP_PROXY")
            .env_remove("http_proxy")
            .env_remove("HTTPS_PROXY")
            .env_remove("https_proxy")
            .env_remove("ALL_PROXY")
            .env_remove("all_proxy")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).ok();
        child.wait_with_output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn connection_refused_returns_curl_exit_code_seven() {
    let fixture = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let output = fixture.run(&[&format!("http://{address}")], b"");
    assert_eq!(output.status.code(), Some(7), "{}", text(&output.stderr));
}

/// Starts a server on 127.0.0.1 and returns its base URL. Routes:
/// `/echo` (method, target, lowercased headers, then `--BODY--` and the raw body),
/// `/redirect` (302 to `/final`), `/final`, `/missing` (404), `/flaky` (503 once, then 200),
/// `/files/hello.txt`.
fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let flaky_hits = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let hits = Arc::clone(&flaky_hits);
            thread::spawn(move || handle(stream, &hits));
        }
    });
    format!("http://{address}")
}

fn handle(stream: TcpStream, hits: &AtomicUsize) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    let mut content_length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).ok();
    let mut response = respond(&method, &target, &headers, &body, hits);
    if method == "HEAD" {
        let end = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map_or(response.len(), |position| position + 4);
        response.truncate(end);
    }
    let mut stream = reader.into_inner();
    stream.write_all(&response).ok();
}

fn respond(
    method: &str,
    target: &str,
    headers: &[(String, String)],
    body: &[u8],
    hits: &AtomicUsize,
) -> Vec<u8> {
    let path = target.split('?').next().unwrap_or(target);
    match path {
        "/redirect" => reply("302 Found", &[("Location", "/final")], b""),
        "/final" => reply("200 OK", &[], b"final body"),
        "/missing" => reply("404 Not Found", &[], b"not here"),
        "/flaky" if hits.fetch_add(1, Ordering::SeqCst) == 0 => {
            reply("503 Service Unavailable", &[], b"busy")
        }
        "/flaky" => reply("200 OK", &[], b"recovered"),
        "/files/hello.txt" => reply("200 OK", &[("Content-Type", "text/plain")], b"hello body"),
        "/echo" => {
            let mut echo = format!("{method} {target}\n");
            for (name, value) in headers {
                echo.push_str(&format!("{name}: {value}\n"));
            }
            echo.push_str("--BODY--\n");
            let mut payload = echo.into_bytes();
            payload.extend_from_slice(body);
            reply("200 OK", &[("Content-Type", "text/plain")], &payload)
        }
        _ => reply("404 Not Found", &[], b"unknown route"),
    }
}

fn reply(status: &str, extra: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Returns the echo body (`--BODY--` section) of an `/echo` response.
fn echo_body(stdout: &[u8]) -> String {
    text(stdout)
        .rsplit_once("--BODY--\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default()
}

#[test]
fn silent_show_error_fetches_body() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    let out = f.run(&["-sS", &url], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).starts_with("GET /echo\n"));
    assert!(out.stderr.is_empty());
}

#[test]
fn bundled_follow_redirects_with_ssl() {
    let base = serve();
    let url = format!("{base}/redirect");
    let f = Fixture::new();
    let out = f.run(&["-sSL", &url], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"final body");
    let plain = f.run(&["-sL", &url], b"");
    assert_eq!(plain.stdout, b"final body");
}

#[test]
fn without_location_redirect_is_not_followed() {
    let base = serve();
    let url = format!("{base}/redirect");
    let f = Fixture::new();
    let out = f.run(&["-s", "-w", "%{http_code} %{redirect_url}", &url], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), format!("302 {base}/final"));
}

#[test]
fn fssl_on_404_exits_22_with_message() {
    let base = serve();
    let url = format!("{base}/missing");
    let f = Fixture::new();
    let out = f.run(&["-fsSL", &url], b"");
    assert_eq!(out.status.code(), Some(22));
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).contains("(22)"));

    let quiet = f.run(&["-fs", &url], b"");
    assert_eq!(quiet.status.code(), Some(22));
    assert!(quiet.stderr.is_empty());
}

#[test]
fn bundled_lo_writes_output_file() {
    let base = serve();
    let url = format!("{base}/redirect");
    let f = Fixture::new();
    let out = f.run(&["-Lo", "out.txt", &url], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert_eq!(f.read("out.txt"), b"final body");
}

#[test]
fn remote_name_uses_url_file_name() {
    let base = serve();
    let url = format!("{base}/files/hello.txt");
    let f = Fixture::new();
    let out = f.run(&["-sO", &url], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert_eq!(f.read("hello.txt"), b"hello body");

    let no_name = f.run(&["-sO", &format!("{base}/")], b"");
    assert_eq!(no_name.status.code(), Some(23));
}

#[test]
fn include_prints_status_and_headers_before_body() {
    let base = serve();
    let url = format!("{base}/files/hello.txt");
    let f = Fixture::new();
    let out = f.run(&["-si", &url], b"");
    let printed = text(&out.stdout).to_ascii_lowercase();
    assert!(printed.starts_with("http/1.1 200 ok\n"));
    assert!(printed.contains("content-type: text/plain\n"));
    assert!(printed.ends_with("\nhello body"));
}

#[test]
fn head_prints_only_headers() {
    let base = serve();
    let url = format!("{base}/files/hello.txt");
    let f = Fixture::new();
    let out = f.run(&["-sI", &url], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).starts_with("HTTP/1.1 200 OK\n"));
    assert!(!text(&out.stdout).contains("hello body"));
}

#[test]
fn write_out_reports_code_size_and_escapes() {
    let base = serve();
    let url = format!("{base}/files/hello.txt");
    let f = Fixture::new();
    let code = f.run(&["-s", "-o", "copy.txt", "-w", "%{http_code}", &url], b"");
    assert_eq!(text(&code.stdout), "200");
    let detail = f.run(
        &[
            "-s",
            "-o",
            "copy.txt",
            "-w",
            "code=%{http_code}\\tsize=%{size_download} 100%%\\n",
            &url,
        ],
        b"",
    );
    assert_eq!(text(&detail.stdout), "code=200\tsize=10 100%\n");
    let bad = f.run(&["-s", "-w", "%{nope}", &url], b"");
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn repeated_data_joins_and_at_file_strips_newlines() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    f.write("body.txt", b"b=2\r\n");
    let out = f.run(&["-s", "-d", "a=1", "-d", "@body.txt", &url], b"");
    assert!(text(&out.stdout).starts_with("POST /echo\n"));
    assert_eq!(echo_body(&out.stdout), "a=1&b=2");
    let from_stdin = f.run(&["-s", "-d", "@-", &url], b"c=3\n");
    assert_eq!(echo_body(&from_stdin.stdout), "c=3");
    let binary = f.run(&["-s", "--data-binary", "@body.txt", &url], b"");
    assert_eq!(echo_body(&binary.stdout), "b=2\r\n");
}

#[test]
fn json_sets_headers_and_accepts_file() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    f.write("payload.json", b"{\"y\":2}");
    let out = f.run(&["-s", "--json", "{\"x\":1}", &url], b"");
    let printed = text(&out.stdout).to_ascii_lowercase();
    assert!(printed.starts_with("post /echo\n"));
    assert!(printed.contains("content-type: application/json\n"));
    assert!(printed.contains("accept: application/json\n"));
    assert_eq!(echo_body(&out.stdout), "{\"x\":1}");
    let from_file = f.run(&["-s", "--json", "@payload.json", &url], b"");
    assert_eq!(echo_body(&from_file.stdout), "{\"y\":2}");
}

#[test]
fn get_moves_data_into_query() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    let out = f.run(&["-sG", "-d", "a=1", &url], b"");
    assert!(text(&out.stdout).starts_with("GET /echo?a=1\n"));
}

#[test]
fn user_option_sends_basic_authorization() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    let out = f.run(&["-s", "-u", "user:pass", &url], b"");
    assert!(text(&out.stdout).contains("authorization: Basic dXNlcjpwYXNz\n"));
}

#[test]
fn custom_headers_removal_and_empty_values() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    let out = f.run(
        &[
            "-sS", "-XPOST", "-HX-A: b", "-H", "Accept:", "-H", "X-E;", &url,
        ],
        b"",
    );
    let printed = text(&out.stdout).to_ascii_lowercase();
    assert!(printed.starts_with("post /echo\n"));
    assert!(printed.contains("x-a: b\n"));
    assert!(printed.contains("x-e: \n"));
    // reqwest's built-in `Accept: */*` default is replaced by an empty value (see request_headers).
    assert!(printed.contains("\naccept: \n"), "headers were:\n{printed}");
    assert!(!printed.contains("*/*"));
}

#[test]
fn agent_and_cookie_options_are_sent() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    let out = f.run(
        &[
            "-s",
            "-A",
            "agent/9",
            "-e",
            "http://ref.test/",
            "-b",
            "a=1",
            &url,
        ],
        b"",
    );
    let printed = text(&out.stdout).to_ascii_lowercase();
    assert!(printed.contains("user-agent: agent/9\n"));
    assert!(printed.contains("referer: http://ref.test/\n"));
    assert!(printed.contains("cookie: a=1\n"));
}

#[test]
fn verbose_writes_headers_to_stderr_only() {
    let base = serve();
    let url = format!("{base}/files/hello.txt");
    let f = Fixture::new();
    let out = f.run(&["-sv", &url], b"");
    let stderr = text(&out.stderr);
    assert!(stderr.contains("> GET /files/hello.txt HTTP/1.1\n"));
    assert!(stderr.contains("< HTTP/1.1 200 OK\n"));
    assert_eq!(out.stdout, b"hello body");
}

#[test]
fn compressed_is_accepted_without_accept_encoding() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    let out = f.run(&["-s", "--compressed", &url], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(!text(&out.stdout)
        .to_ascii_lowercase()
        .contains("accept-encoding"));
}

#[test]
fn retry_recovers_from_503() {
    let base = serve();
    let url = format!("{base}/flaky");
    let f = Fixture::new();
    let out = f.run(&["-sS", "--retry", "1", "--retry-delay", "0", &url], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"recovered");
}

#[test]
fn max_redirs_zero_exits_47() {
    let base = serve();
    let url = format!("{base}/redirect");
    let f = Fixture::new();
    let out = f.run(&["-sS", "-L", "--max-redirs", "0", &url], b"");
    assert_eq!(out.status.code(), Some(47));
}

#[test]
fn dump_header_to_stdout_with_output_file() {
    let base = serve();
    let url = format!("{base}/files/hello.txt");
    let f = Fixture::new();
    let out = f.run(&["-s", "-D", "-", "-o", "body.bin", &url], b"");
    assert!(text(&out.stdout)
        .to_ascii_lowercase()
        .starts_with("http/1.1 200 ok\n"));
    assert_eq!(f.read("body.bin"), b"hello body");
}

#[test]
fn multipart_form_is_rejected() {
    let base = serve();
    let url = format!("{base}/echo");
    let f = Fixture::new();
    let out = f.run(&["-F", "name=value", &url], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(text(&out.stderr).contains("not supported"));
}

#[test]
fn unknown_option_exits_2() {
    let f = Fixture::new();
    let out = f.run(&["--no-such-option", "http://127.0.0.1:9/"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("unknown option"));
}
