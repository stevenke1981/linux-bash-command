use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-tools-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, contents: &[u8]) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    fn run(&self, name: &str, args: &[&str], stdin: &[u8]) -> Output {
        let binary = match name {
            "cat" => env!("CARGO_BIN_EXE_cat"),
            "head" => env!("CARGO_BIN_EXE_head"),
            "tail" => env!("CARGO_BIN_EXE_tail"),
            "less" => env!("CARGO_BIN_EXE_less"),
            "grep" => env!("CARGO_BIN_EXE_grep"),
            "rg" => env!("CARGO_BIN_EXE_rg"),
            "pwd" => env!("CARGO_BIN_EXE_pwd"),
            "cd" => env!("CARGO_BIN_EXE_cd"),
            "ls" => env!("CARGO_BIN_EXE_ls"),
            "find" => env!("CARGO_BIN_EXE_find"),
            "ps" => env!("CARGO_BIN_EXE_ps"),
            "df" => env!("CARGO_BIN_EXE_df"),
            "curl" => env!("CARGO_BIN_EXE_curl"),
            _ => panic!("unknown binary"),
        };
        let mut child = Command::new(binary)
            .current_dir(&self.0)
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Err(error) = child.stdin.take().unwrap().write_all(stdin) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        child.wait_with_output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn text_commands_handle_binary_streams_and_counts() {
    let f = Fixture::new();
    let bytes = b"one\n\xfftwo\nthree";
    for command in ["cat", "less"] {
        let out = f.run(command, &[], bytes);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, bytes);
    }
    assert_eq!(f.run("head", &["-n", "2"], bytes).stdout, b"one\n\xfftwo\n");
    assert_eq!(f.run("tail", &["-n", "2"], bytes).stdout, b"\xfftwo\nthree");
    assert!(f.run("head", &["-n", "0"], bytes).stdout.is_empty());
    assert_eq!(f.run("cat", &["missing"], b"").status.code(), Some(2));
    f.write("a", b"first");
    f.write("b", b"second");
    assert_eq!(f.run("less", &["a", "b"], b"").stdout, b"firstsecond");
}

#[test]
fn grep_and_rg_search_stdin_and_handle_errors() {
    let f = Fixture::new();
    let out = f.run("grep", &["-n", "-i", "hello"], b"No\nHello\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"2:Hello\n");
    assert_eq!(
        f.run("grep", &["missing"], b"hello\n").status.code(),
        Some(1)
    );
    assert_eq!(f.run("grep", &["["], b"hello\n").status.code(), Some(2));
    let out = f.run("rg", &["hello"], b"no\nhello\n");
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stdout).unwrap().contains("2:hello"));
    f.write("hello.txt", b"hello\n");
    f.write("src/main.rs", b"fn main() {}\n");
    let files = f.run("rg", &["--files", "-g", "*.rs"], b"");
    assert_eq!(files.status.code(), Some(0));
    let files = String::from_utf8(files.stdout).unwrap();
    assert!(files.contains("main.rs"));
    assert!(!files.contains("hello.txt"));
    assert_eq!(
        f.run("rg", &["--files", "-g", "*.missing"], b"")
            .status
            .code(),
        Some(1)
    );
    let out = f.run("grep", &["-h", "hello", "hello.txt"], b"");
    assert_eq!(out.stdout, b"hello\n");
}

#[test]
fn filesystem_tools_list_find_and_resolve_directories() {
    let f = Fixture::new();
    f.write("visible.txt", b"x");
    f.write(".secret", b"x");
    f.write("sub/nested.rs", b"x");
    let out = f.run("ls", &[], b"");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("visible.txt"));
    assert!(!text.contains(".secret"));
    assert!(String::from_utf8(f.run("ls", &["-a"], b"").stdout)
        .unwrap()
        .contains(".secret"));
    let found = f.run("find", &[".", "-name", "*.rs", "-type", "f"], b"");
    assert_eq!(found.status.code(), Some(0));
    assert!(String::from_utf8(found.stdout)
        .unwrap()
        .contains("nested.rs"));
    assert_eq!(f.run("find", &["missing"], b"").status.code(), Some(2));
    let out = f.run("pwd", &[], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
            .canonicalize()
            .unwrap(),
        f.0.canonicalize().unwrap()
    );
    let out = f.run("cd", &["sub"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
            .canonicalize()
            .unwrap(),
        f.0.join("sub").canonicalize().unwrap()
    );
    assert_eq!(f.run("cd", &["visible.txt"], b"").status.code(), Some(2));
}

#[test]
fn process_and_disk_commands_return_structured_information() {
    let f = Fixture::new();
    let processes = f.run("ps", &["--json"], b"");
    assert_eq!(processes.status.code(), Some(0));
    let records: serde_json::Value = serde_json::from_slice(&processes.stdout).unwrap();
    assert!(!records.as_array().unwrap().is_empty());
    assert!(records[0]["pid"].is_number());
    assert_eq!(f.run("ps", &["-p", "bad"], b"").status.code(), Some(2));
    let disk = f.run("df", &["-h", "."], b"");
    assert_eq!(
        disk.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&disk.stderr)
    );
    assert!(String::from_utf8(disk.stdout).unwrap().contains("Size"));
}

#[test]
fn curl_handles_http_body_and_fail_status() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    fn serve(status: &str, body: &'static [u8]) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let status = status.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        });
        (url, handle)
    }
    let f = Fixture::new();
    let (url, server) = serve("200 OK", b"hello\x00world");
    let out = f.run("curl", &[&url], b"");
    server.join().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"hello\x00world");
    let (url, server) = serve("404 Not Found", b"missing");
    let out = f.run("curl", &["-f", &url], b"");
    server.join().unwrap();
    assert_eq!(out.status.code(), Some(22));
    assert!(out.stdout.is_empty());
    assert_eq!(
        f.run("curl", &["-m", "1e300", "http://localhost/"], b"")
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn curl_handles_head_post_and_redirects() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for expected in ["HEAD", "POST", "GET", "GET"] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with(expected), "{request}");
            let response = if request.starts_with("GET /redirect ") {
                "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            } else if expected == "HEAD" {
                "HTTP/1.1 200 OK\r\nX-Test: yes\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    let f = Fixture::new();
    let head = f.run("curl", &["-I", &url], b"");
    assert_eq!(head.status.code(), Some(0));
    assert!(String::from_utf8(head.stdout)
        .unwrap()
        .to_lowercase()
        .contains("x-test: yes"));
    let post = f.run("curl", &["-d", "x=1", &url], b"");
    assert_eq!(post.status.code(), Some(0));
    assert_eq!(post.stdout, b"ok");
    let redirect = f.run("curl", &["-L", &format!("{url}redirect")], b"");
    assert_eq!(redirect.status.code(), Some(0));
    assert_eq!(redirect.stdout, b"ok");
    server.join().unwrap();
}
