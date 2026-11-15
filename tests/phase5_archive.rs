mod common;
use common::Sandbox;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Output},
};

fn run(s: &Sandbox, name: &str, args: &[&str], input: &[u8]) -> Output {
    let exe = match name {
        "gzip" => env!("CARGO_BIN_EXE_gzip"),
        "gunzip" => env!("CARGO_BIN_EXE_gunzip"),
        "zcat" => env!("CARGO_BIN_EXE_zcat"),
        "tar" => env!("CARGO_BIN_EXE_tar"),
        _ => unreachable!(),
    };
    let mut cmd = Command::new(exe);
    cmd.current_dir(&s.path).args(args);
    common::execute(cmd, input)
}
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}
fn ungzip(data: &[u8]) -> Vec<u8> {
    let mut decoder = flate2::read::MultiGzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).unwrap();
    out
}
fn ok(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}
#[test]
fn gzip_roundtrip_and_original_policy() {
    let s = Sandbox::new();
    s.write("a", b"one\ntwo\0");
    ok(&run(&s, "gzip", &["a"], b""));
    assert!(!s.path.join("a").exists());
    assert_eq!(
        ungzip(&fs::read(s.path.join("a.gz")).unwrap()),
        b"one\ntwo\0"
    );
    ok(&run(&s, "gunzip", &["a.gz"], b""));
    assert!(!s.path.join("a.gz").exists());
    assert_eq!(fs::read(s.path.join("a")).unwrap(), b"one\ntwo\0");
    ok(&run(&s, "gzip", &["-k", "a"], b""));
    assert!(s.path.join("a").exists());
}
#[test]
fn aliases_stdout_and_multimember() {
    let s = Sandbox::new();
    let mut joined = gzip(b"first");
    joined.extend_from_slice(&gzip(b"second"));
    for (cmd, args) in [
        ("zcat", &[][..]),
        ("gzip", &["-dc"][..]),
        ("gunzip", &["-c"][..]),
    ] {
        let out = run(&s, cmd, args, &joined);
        ok(&out);
        assert_eq!(out.stdout, b"firstsecond");
    }
    let out = run(&s, "gzip", &["-c", "-1"], b"stdin\n");
    ok(&out);
    assert_eq!(ungzip(&out.stdout), b"stdin\n");
}
#[test]
fn overwrite_and_bad_data_preserve_inputs() {
    let s = Sandbox::new();
    s.write("a", b"source");
    s.write("a.gz", b"target");
    let refusal = run(&s, "gzip", &["a"], b"");
    assert_eq!(refusal.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&refusal.stderr).contains("a.gz already exists"));
    assert_eq!(fs::read(s.path.join("a")).unwrap(), b"source");
    assert_eq!(fs::read(s.path.join("a.gz")).unwrap(), b"target");
    ok(&run(&s, "gzip", &["-fk", "a"], b""));
    assert_eq!(ungzip(&fs::read(s.path.join("a.gz")).unwrap()), b"source");
    s.write("bad.gz", b"not a gzip");
    assert_ne!(run(&s, "gunzip", &["bad.gz"], b"").status.code(), Some(0));
    let quiet = run(&s, "gunzip", &["-q", "bad.gz"], b"");
    assert_eq!(quiet.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&quiet.stderr).contains("not in gzip format"));
    s.write("plain", b"bad");
    let unknown = run(&s, "gunzip", &["plain"], b"");
    assert_eq!(unknown.status.code(), Some(2));
    assert!(s.path.join("bad.gz").exists());
    assert!(!s.path.join("bad").exists());
}
#[test]
fn gzip_test_list_suffix_and_levels() {
    let s = Sandbox::new();
    s.write("a", b"contents");
    ok(&run(
        &s,
        "gzip",
        &["--best", "--keep", "--suffix=.z", "a"],
        b"",
    ));
    ok(&run(&s, "gunzip", &["--test", "--suffix=.z", "a.z"], b""));
    let out = run(&s, "gzip", &["--list", "--suffix=.z", "a.z"], b"");
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("uncompressed"));
    let out = run(&s, "gzip", &["--fast", "--stdout", "a"], b"");
    ok(&out);
    assert_eq!(ungzip(&out.stdout), b"contents");
}
#[test]
fn gzip_header_restore_and_no_name() {
    let s = Sandbox::new();
    s.write("original", b"content");
    let timestamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    fs::OpenOptions::new()
        .write(true)
        .open(s.path.join("original"))
        .unwrap()
        .set_modified(timestamp)
        .unwrap();
    ok(&run(&s, "gzip", &["-k", "original"], b""));
    fs::rename(s.path.join("original.gz"), s.path.join("renamed.gz")).unwrap();
    fs::remove_file(s.path.join("original")).unwrap();
    ok(&run(&s, "gunzip", &["-N", "renamed.gz"], b""));
    assert_eq!(fs::read(s.path.join("original")).unwrap(), b"content");
    assert_eq!(
        fs::metadata(s.path.join("original"))
            .unwrap()
            .modified()
            .unwrap(),
        timestamp
    );
    let encoded = run(&s, "gzip", &["-cn", "original"], b"");
    ok(&encoded);
    let decoder = flate2::read::MultiGzDecoder::new(&encoded.stdout[..]);
    assert!(decoder.header().unwrap().filename().is_none());
    let named = run(&s, "gzip", &["-c", "original"], b"");
    ok(&named);
    let decoder = flate2::read::MultiGzDecoder::new(&named.stdout[..]);
    assert_eq!(
        decoder.header().unwrap().filename(),
        Some(b"original".as_slice())
    );
}
#[test]
fn alias_options_and_help() {
    let s = Sandbox::new();
    let encoded = gzip(b"data\n");
    for command in ["gunzip", "zcat"] {
        for opt in ["-c", "-d", "-q", "-k", "-n", "-N", "-v", "--stdout"] {
            let out = run(&s, command, &[opt], &encoded);
            ok(&out);
            assert_eq!(out.stdout, b"data\n", "{command} {opt}");
        }
        for opt in ["--help", "--version"] {
            let out = run(&s, command, &[opt], b"");
            ok(&out);
            assert!(!out.stdout.is_empty());
        }
        let out = run(&s, command, &["-t"], &encoded);
        ok(&out);
        assert!(out.stdout.is_empty());
        let out = run(&s, command, &["-fc"], b"plain\n");
        ok(&out);
        assert_eq!(out.stdout, b"plain\n");
    }
}
#[test]
fn delayed_magic_bytes_do_not_disable_decompression() {
    use std::{process::Stdio, thread, time::Duration};
    let s = Sandbox::new();
    let bytes = gzip(b"delayed stream");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gunzip"))
        .current_dir(&s.path)
        .args(["-fc"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(&bytes[..1]).unwrap();
    input.flush().unwrap();
    thread::sleep(Duration::from_millis(100));
    input.write_all(&bytes[1..]).unwrap();
    drop(input);
    let out = child.wait_with_output().unwrap();
    ok(&out);
    assert_eq!(out.stdout, b"delayed stream");
}
#[test]
fn gzip_recursive_and_quiet() {
    let s = Sandbox::new();
    s.write("dir/a", b"a");
    s.write("dir/sub/b", b"b");
    ok(&run(&s, "gzip", &["-rqk", "dir"], b""));
    assert_eq!(ungzip(&fs::read(s.path.join("dir/a.gz")).unwrap()), b"a");
    assert_eq!(
        ungzip(&fs::read(s.path.join("dir/sub/b.gz")).unwrap()),
        b"b"
    );
}
#[cfg(unix)]
#[test]
fn gzip_recursive_preserves_non_utf8_names() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let s = Sandbox::new();
    fs::create_dir(s.path.join("dir")).unwrap();
    let name = OsString::from_vec(b"f\xff".to_vec());
    fs::write(s.path.join("dir").join(&name), b"data").unwrap();
    ok(&run(&s, "gzip", &["-r", "dir"], b""));
    let mut compressed = name.clone();
    compressed.push(".gz");
    assert!(s.path.join("dir").join(compressed).exists());
    ok(&run(&s, "gunzip", &["-r", "dir"], b""));
    assert_eq!(fs::read(s.path.join("dir").join(name)).unwrap(), b"data");
}
#[test]
fn tar_create_list_extract_and_gzip() {
    let s = Sandbox::new();
    s.write("src/a", b"first\n");
    s.write("src/sub/b", b"second\n");
    ok(&run(
        &s,
        "tar",
        &["-czf", "archive.tgz", "-C", "src", "."],
        b"",
    ));
    let list = run(&s, "tar", &["-tzf", "archive.tgz"], b"");
    ok(&list);
    assert!(String::from_utf8_lossy(&list.stdout).contains("sub/b"));
    let auto = run(&s, "tar", &["-tf", "archive.tgz"], b"");
    ok(&auto);
    assert_eq!(auto.stdout, list.stdout);
    fs::create_dir(s.path.join("dest")).unwrap();
    ok(&run(&s, "tar", &["-xzf", "archive.tgz", "-C", "dest"], b""));
    assert_eq!(fs::read(s.path.join("dest/a")).unwrap(), b"first\n");
    assert_eq!(fs::read(s.path.join("dest/sub/b")).unwrap(), b"second\n");
}
#[test]
fn tar_old_syntax_stdout_and_strip() {
    let s = Sandbox::new();
    s.write("src/dir/f", b"payload");
    ok(&run(&s, "tar", &["cf", "a.tar", "-C", "src", "dir"], b""));
    let out = run(&s, "tar", &["xOf", "a.tar"], b"");
    ok(&out);
    assert_eq!(out.stdout, b"payload");
    fs::create_dir(s.path.join("out")).unwrap();
    ok(&run(
        &s,
        "tar",
        &["xf", "a.tar", "--strip-components=1", "-C", "out"],
        b"",
    ));
    assert_eq!(fs::read(s.path.join("out/f")).unwrap(), b"payload");
}
#[test]
fn tar_verbose_and_missing_members() {
    let s = Sandbox::new();
    s.write("dir/file", b"x");
    let out = run(&s, "tar", &["-cvf", "a.tar", "dir"], b"");
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("dir/"));
    let out = run(&s, "tar", &["-tvf", "a.tar"], b"");
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stdout).contains("dir/file"));
    let out = run(&s, "tar", &["-tf", "a.tar", "missing"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Not found in archive"));
}
#[test]
fn tar_long_names_and_auto_compression() {
    let s = Sandbox::new();
    let name = "x".repeat(140);
    s.write(&name, b"long name");
    ok(&run(&s, "tar", &["-caf", "a.tgz", &name], b""));
    fs::create_dir(s.path.join("out")).unwrap();
    ok(&run(&s, "tar", &["-xf", "a.tgz", "-C", "out"], b""));
    assert_eq!(
        fs::read(s.path.join("out").join(name)).unwrap(),
        b"long name"
    );
}
#[test]
fn tar_sniffs_delayed_gzip_header() {
    use std::{process::Stdio, thread, time::Duration};
    let s = Sandbox::new();
    let mut archive = Vec::new();
    {
        let mut b = tar::Builder::new(&mut archive);
        append_raw(&mut b, b"a", b"data");
        b.finish().unwrap();
    }
    let bytes = gzip(&archive);
    let mut child = Command::new(env!("CARGO_BIN_EXE_tar"))
        .current_dir(&s.path)
        .args(["-tf", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(&bytes[..1]).unwrap();
    input.flush().unwrap();
    thread::sleep(Duration::from_millis(100));
    input.write_all(&bytes[1..]).unwrap();
    drop(input);
    let out = child.wait_with_output().unwrap();
    ok(&out);
    assert_eq!(out.stdout, b"a\n");
}
#[test]
fn gzip_list_totals_and_verbose() {
    let s = Sandbox::new();
    s.write("a.gz", gzip(b"abc"));
    s.write("b.gz", gzip(b"def"));
    let out = run(&s, "gzip", &["-lv", "a.gz", "b.gz"], b"");
    ok(&out);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("method  crc"));
    assert!(text.contains("(totals)"));
    let out = run(&s, "gzip", &["-lq", "a.gz", "b.gz"], b"");
    ok(&out);
    assert!(!String::from_utf8(out.stdout).unwrap().contains("(totals)"));
    s.write("plain", b"content");
    let out = run(&s, "gzip", &["-vc", "plain"], b"");
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stderr).contains("-- replaced with stdout"));
    let out = run(&s, "gzip", &["-vt", "a.gz"], b"");
    ok(&out);
    assert!(String::from_utf8_lossy(&out.stderr).contains("OK"));
}
#[test]
fn tar_keep_old_and_exclude() {
    let s = Sandbox::new();
    s.write("src/a", b"new");
    s.write("src/skip.tmp", b"excluded");
    ok(&run(
        &s,
        "tar",
        &["-cf", "a.tar", "--exclude=*.tmp", "-C", "src", "."],
        b"",
    ));
    s.write("out/a", b"old");
    assert_ne!(
        run(&s, "tar", &["-xkf", "a.tar", "-C", "out"], b"")
            .status
            .code(),
        Some(0)
    );
    assert_eq!(fs::read(s.path.join("out/a")).unwrap(), b"old");
    assert!(!s.path.join("out/skip.tmp").exists());
    ok(&run(
        &s,
        "tar",
        &["-xf", "a.tar", "--overwrite", "-C", "out"],
        b"",
    ));
    assert_eq!(fs::read(s.path.join("out/a")).unwrap(), b"new");
}
#[test]
fn tar_files_from_null_and_directory_order() {
    let s = Sandbox::new();
    s.write("first/a", b"a");
    s.write("second/b", b"b");
    s.write("names", b"a\0");
    ok(&run(
        &s,
        "tar",
        &[
            "-cf",
            "a.tar",
            "--null",
            "-C",
            "first",
            "-T",
            "names",
            "-C",
            "../second",
            "b",
        ],
        b"",
    ));
    let out = run(&s, "tar", &["-tf", "a.tar"], b"");
    ok(&out);
    let listing = String::from_utf8(out.stdout).unwrap();
    assert!(listing.lines().any(|s| s == "a"));
    assert!(listing.lines().any(|s| s == "b"));
}
#[cfg(unix)]
#[test]
fn tar_files_from_preserves_carriage_return() {
    let s = Sandbox::new();
    s.write("name\r", b"data");
    s.write("names", b"name\r\n");
    ok(&run(&s, "tar", &["-cf", "a.tar", "-T", "names"], b""));
    let out = run(&s, "tar", &["-tf", "a.tar"], b"");
    ok(&out);
    assert_eq!(out.stdout, b"name\r\n");
}
fn append_raw(builder: &mut tar::Builder<&mut Vec<u8>>, name: &[u8], contents: &[u8]) {
    let mut h = tar::Header::new_gnu();
    h.set_size(contents.len() as u64);
    h.set_mode(0o644);
    h.set_entry_type(tar::EntryType::Regular);
    h.as_mut_bytes()[..100].fill(0);
    h.as_mut_bytes()[..name.len()].copy_from_slice(name);
    h.set_cksum();
    builder.append(&h, contents).unwrap();
}
#[test]
fn tar_rejects_parent_traversal() {
    let s = Sandbox::new();
    let mut bytes = Vec::new();
    {
        let mut b = tar::Builder::new(&mut bytes);
        append_raw(&mut b, b"../escape", b"bad");
        b.finish().unwrap();
    }
    s.write("a.tar", bytes);
    fs::create_dir(s.path.join("out")).unwrap();
    let out = run(&s, "tar", &["-xf", "a.tar", "-C", "out"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Member name contains '..'"));
    assert!(!s.path.join("escape").exists());
}
#[test]
fn tar_rejects_parent_traversal_from_pax_path() {
    let s = Sandbox::new();
    let mut bytes = Vec::new();
    {
        let mut b = tar::Builder::new(&mut bytes);
        b.append_pax_extensions([("path", b"../escape".as_slice())])
            .unwrap();
        append_raw(&mut b, b"safe", b"bad");
        b.finish().unwrap();
    }
    s.write("a.tar", bytes);
    fs::create_dir(s.path.join("out")).unwrap();
    let out = run(&s, "tar", &["-xf", "a.tar", "-C", "out"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(!s.path.join("escape").exists());
}
#[test]
fn tar_failed_hardlink_preserves_existing_destination() {
    let s = Sandbox::new();
    let mut bytes = Vec::new();
    {
        let mut b = tar::Builder::new(&mut bytes);
        let mut h = tar::Header::new_gnu();
        h.set_size(0);
        h.set_mode(0o644);
        h.set_entry_type(tar::EntryType::Link);
        h.set_cksum();
        b.append_link(&mut h, "file", "missing").unwrap();
        b.finish().unwrap();
    }
    s.write("a.tar", bytes);
    s.write("out/file", b"original");
    let out = run(&s, "tar", &["-xf", "a.tar", "-C", "out"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(s.path.join("out/file")).unwrap(), b"original");
}
#[cfg(unix)]
#[test]
fn tar_rejects_traversal_through_archive_symlink() {
    let s = Sandbox::new();
    let mut bytes = Vec::new();
    {
        let mut b = tar::Builder::new(&mut bytes);
        let mut h = tar::Header::new_gnu();
        h.set_size(0);
        h.set_mode(0o777);
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_cksum();
        b.append_link(&mut h, "link", "../outside").unwrap();
        append_raw(&mut b, b"link/escape", b"bad");
        b.finish().unwrap();
    }
    s.write("a.tar", bytes);
    fs::create_dir(s.path.join("out")).unwrap();
    fs::create_dir(s.path.join("outside")).unwrap();
    let out = run(&s, "tar", &["-xf", "a.tar", "-C", "out"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(!s.path.join("outside/escape").exists());
}
#[cfg(unix)]
#[test]
fn tar_rejects_preexisting_symlink_parent() {
    use std::os::unix::fs::symlink;
    let s = Sandbox::new();
    let mut bytes = Vec::new();
    {
        let mut b = tar::Builder::new(&mut bytes);
        append_raw(&mut b, b"link/escape", b"bad");
        b.finish().unwrap();
    }
    s.write("a.tar", bytes);
    fs::create_dir(s.path.join("out")).unwrap();
    fs::create_dir(s.path.join("outside")).unwrap();
    symlink("../outside", s.path.join("out/link")).unwrap();
    let out = run(&s, "tar", &["-xf", "a.tar", "-C", "out"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(!s.path.join("outside/escape").exists());
}
#[cfg(unix)]
#[test]
fn tar_preserves_hardlinks_and_modes() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    s.write("src/a", b"hardlink");
    fs::set_permissions(s.path.join("src/a"), fs::Permissions::from_mode(0o751)).unwrap();
    fs::hard_link(s.path.join("src/a"), s.path.join("src/b")).unwrap();
    ok(&run(&s, "tar", &["-cf", "a.tar", "-C", "src", "."], b""));
    fs::create_dir(s.path.join("out")).unwrap();
    ok(&run(&s, "tar", &["-xpf", "a.tar", "-C", "out"], b""));
    let a = fs::metadata(s.path.join("out/a")).unwrap();
    let b = fs::metadata(s.path.join("out/b")).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()));
    assert_eq!(a.permissions().mode() & 0o777, 0o751);
}
#[test]
fn tar_absolute_names_strip_and_no_self_archive() {
    let s = Sandbox::new();
    let mut bytes = Vec::new();
    {
        let mut b = tar::Builder::new(&mut bytes);
        append_raw(&mut b, b"/absolute", b"data");
        b.finish().unwrap();
    }
    s.write("a.tar", bytes);
    fs::create_dir(s.path.join("out")).unwrap();
    let out = run(&s, "tar", &["-xf", "a.tar", "-C", "out"], b"");
    ok(&out);
    assert_eq!(fs::read(s.path.join("out/absolute")).unwrap(), b"data");
    s.write("tree/f", b"data");
    ok(&run(
        &s,
        "tar",
        &["-cf", "tree/archive.tar", "-C", "tree", "."],
        b"",
    ));
    let archive = fs::File::open(s.path.join("tree/archive.tar")).unwrap();
    let mut ar = tar::Archive::new(archive);
    let names: Vec<_> = ar
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().into_owned())
        .collect();
    assert!(!names
        .iter()
        .any(|p| p.to_string_lossy().contains(".agentcmd-") || p == Path::new("archive.tar")));
}
#[cfg(unix)]
#[test]
fn tar_absolute_names_option_targets_sandbox_path() {
    use std::os::unix::ffi::OsStrExt;
    let s = Sandbox::new();
    let absolute = fs::canonicalize(&s.path).unwrap().join("absolute");
    let mut bytes = Vec::new();
    {
        let mut b = tar::Builder::new(&mut bytes);
        b.append_pax_extensions([("path", absolute.as_os_str().as_bytes())])
            .unwrap();
        append_raw(&mut b, b"safe", b"data");
        b.finish().unwrap();
    }
    s.write("a.tar", bytes);
    fs::create_dir(s.path.join("out")).unwrap();
    let out = run(&s, "tar", &["-xPf", "a.tar", "-C", "out"], b"");
    ok(&out);
    assert_eq!(fs::read(absolute).unwrap(), b"data");
    assert!(!s.path.join("out/absolute").exists());
}

#[test]
fn gzip_padding_and_trailing_garbage_status() {
    let s = Sandbox::new();
    for n in [1, 8, 20] {
        let mut bytes = gzip(b"abc");
        bytes.extend(vec![0; n]);
        let out = run(&s, "gzip", &["-dc"], &bytes);
        assert_eq!(out.status.code(), Some(0), "padding {n}: {out:?}");
        assert_eq!(out.stdout, b"abc");
        assert!(out.stderr.is_empty());
    }
    let mut bytes = gzip(b"abc");
    bytes.extend_from_slice(b"junk");
    let out = run(&s, "gzip", &["-dc"], &bytes);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(out.stdout, b"abc");
    assert!(String::from_utf8_lossy(&out.stderr).contains("trailing garbage ignored"));
    let out = run(&s, "gzip", &["-qdc"], &bytes);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(out.stdout, b"abc");
    assert!(out.stderr.is_empty());
    let out = run(&s, "gzip", &["-l"], &bytes);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let out = run(&s, "gzip", &["-t"], &bytes);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    s.write("keep.gz", &bytes);
    let out = run(&s, "gunzip", &["-qk", "keep.gz"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    assert!(s.path.join("keep.gz").exists());
    assert_eq!(fs::read(s.path.join("keep")).unwrap(), b"abc");
    s.write("warn.gz", bytes);
    let out = run(&s, "gunzip", &["warn.gz"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(s.path.join("warn")).unwrap(), b"abc");
    assert!(!s.path.join("warn.gz").exists());
}
#[test]
fn gzip_crc_and_truncated_member_errors_remain_fatal() {
    let s = Sandbox::new();
    let mut bytes = gzip(b"abc");
    let n = bytes.len();
    bytes[n - 8] ^= 1;
    let out = run(&s, "gzip", &["-dc"], &bytes);
    assert_eq!(out.status.code(), Some(1));
    s.write("bad.gz", &bytes);
    s.write("bad", b"original");
    let out = run(&s, "gunzip", &["-f", "bad.gz"], b"");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read(s.path.join("bad")).unwrap(), b"original");
    assert!(s.path.join("bad.gz").exists());
    let mut bytes = gzip(b"abc");
    bytes.push(0x1f);
    assert_eq!(run(&s, "gzip", &["-dc"], &bytes).status.code(), Some(1));
}
#[cfg(unix)]
#[test]
fn gzip_force_symlink_uses_target_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let s = Sandbox::new();
    s.write("secret", b"private");
    fs::set_permissions(s.path.join("secret"), fs::Permissions::from_mode(0o600)).unwrap();
    symlink("secret", s.path.join("link")).unwrap();
    ok(&run(&s, "gzip", &["-fk", "link"], b""));
    assert_eq!(
        fs::metadata(s.path.join("link.gz"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        ungzip(&fs::read(s.path.join("link.gz")).unwrap()),
        b"private"
    );
}
#[test]
fn tar_truncated_member_preserves_existing_file() {
    let s = Sandbox::new();
    let mut header = tar::Header::new_gnu();
    header.set_path("a").unwrap();
    header.set_size(3);
    header.set_mode(0o644);
    header.set_cksum();
    let mut bytes = header.as_bytes().to_vec();
    bytes.extend_from_slice(b"NE");
    s.write("bad.tar", bytes);
    s.write("a", b"OLD");
    let out = run(&s, "tar", &["-xf", "bad.tar"], b"");
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::read(s.path.join("a")).unwrap(), b"OLD");
}
#[test]
fn tar_gzip_trailer_is_validated_after_end_markers() {
    let s = Sandbox::new();
    let mut archive = Vec::new();
    {
        let mut b = tar::Builder::new(&mut archive);
        append_raw(&mut b, b"a", b"data");
        b.finish().unwrap();
    }
    let mut bytes = gzip(&archive);
    let n = bytes.len();
    bytes[n - 8] ^= 1;
    s.write("bad.tgz", bytes);
    for args in [&["-tzf", "bad.tgz"][..], &["-xzf", "bad.tgz"][..]] {
        let out = run(&s, "tar", args, b"");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
    }
}
#[test]
fn tar_strips_hardlink_targets() {
    let s = Sandbox::new();
    let mut archive = Vec::new();
    {
        let mut b = tar::Builder::new(&mut archive);
        append_raw(&mut b, b"dir/a", b"data");
        let mut h = tar::Header::new_gnu();
        h.set_size(0);
        h.set_mode(0o644);
        h.set_entry_type(tar::EntryType::Link);
        h.set_cksum();
        b.append_link(&mut h, "dir/b", "dir/a").unwrap();
        b.finish().unwrap();
    }
    s.write("a.tar", archive);
    fs::create_dir(s.path.join("out")).unwrap();
    let out = run(
        &s,
        "tar",
        &["-xf", "a.tar", "--strip-components=1", "-C", "out"],
        b"",
    );
    ok(&out);
    assert_eq!(fs::read(s.path.join("out/a")).unwrap(), b"data");
    assert_eq!(fs::read(s.path.join("out/b")).unwrap(), b"data");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let a = fs::metadata(s.path.join("out/a")).unwrap();
        let b = fs::metadata(s.path.join("out/b")).unwrap();
        assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()));
    }
}
#[test]
fn tar_files_from_resolves_in_invocation_directory() {
    let s = Sandbox::new();
    s.write("src/a", b"data");
    s.write("list", b"a\n");
    ok(&run(
        &s,
        "tar",
        &["-cf", "a.tar", "-C", "src", "-T", "list"],
        b"",
    ));
    let out = run(&s, "tar", &["-tf", "a.tar"], b"");
    ok(&out);
    assert_eq!(out.stdout, b"a\n");
}
#[test]
fn tar_traditional_bundle_consumes_separate_arguments() {
    let s = Sandbox::new();
    s.write("a", b"data");
    let out = run(&s, "tar", &["cfv", "out.tar", "a"], b"");
    ok(&out);
    assert_eq!(out.stdout, b"a\n");
    assert!(s.path.join("out.tar").exists());
    let out = run(&s, "tar", &["-cfv", "a"], b"");
    ok(&out);
    assert!(out.stdout.is_empty());
    assert!(s.path.join("v").exists());
}
