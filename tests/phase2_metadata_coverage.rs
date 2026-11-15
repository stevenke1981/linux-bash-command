mod common;
use common::Sandbox;
use std::process::Command;
fn stat(s: &Sandbox, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stat"));
    command.current_dir(&s.path).args(args);
    common::execute(command, b"")
}
#[test]
fn native_filesystem_layouts_and_query_errors() {
    let s = Sandbox::new();
    s.write("file", b"abc");
    for args in [
        vec!["-f", "file"],
        vec!["-ft", "file"],
        vec![
            "-f",
            "--printf=%n %i %l %t %T %s %S %b %f %a %c %d %% %Q",
            "file",
        ],
    ] {
        let out = stat(&s, &args);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert!(!out.stdout.is_empty());
    }
    let out = stat(&s, &["-f", "missing"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}
#[test]
fn metadata_directives_report_real_special_and_regular_types() {
    let s = Sandbox::new();
    s.write("empty", b"");
    s.write("file name", b"abc");
    std::fs::create_dir(s.path.join("dir")).unwrap();
    for (path, kind) in [
        ("empty", "regular empty file"),
        ("dir", "directory"),
        ("file name", "regular file"),
    ] {
        let out = stat(
            &s,
            &[
                "--printf=%F|%m|%B|%g|%G|%u|%U|%t|%T|%Hr|%Lr|%Hd|%Ld|%N",
                path,
            ],
        );
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert!(String::from_utf8_lossy(&out.stdout).starts_with(kind));
        assert!(!out.stdout.contains(&b'%'));
    }
    #[cfg(unix)]
    {
        use std::os::unix::{ffi::OsStrExt, net::UnixListener};
        std::os::unix::fs::symlink("file name", s.path.join("link")).unwrap();
        let fifo = s.path.join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: this creates only a named pipe inside the temporary sandbox.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let socket_root = std::path::Path::new("/tmp")
            .join(format!("agentcmd-stat-coverage-{}", std::process::id()));
        std::fs::create_dir_all(&socket_root).unwrap();
        let socket_path = socket_root.join("socket");
        let listener = UnixListener::bind(&socket_path).unwrap();
        for (path, kind) in [
            ("link", "symbolic link"),
            ("fifo", "fifo"),
            (socket_path.to_str().unwrap(), "socket"),
        ] {
            let out = stat(&s, &["--printf=%F|%f|%A|%N", path]);
            assert_eq!(out.status.code(), Some(0));
            assert!(String::from_utf8_lossy(&out.stdout).starts_with(kind));
        }
        drop(listener);
        std::fs::remove_dir_all(socket_root).unwrap();
    }
}
