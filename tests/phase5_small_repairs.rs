use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "p5-repair-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn write(&self, p: &str, b: &[u8]) {
        fs::write(self.0.join(p), b).unwrap()
    }
    fn run(&self, bin: &str, args: &[&str], input: &[u8]) -> Output {
        let executable = if bin == "xxd" {
            env!("CARGO_BIN_EXE_xxd")
        } else {
            env!("CARGO_BIN_EXE_file")
        };
        let mut child = Command::new(executable)
            .args(args)
            .current_dir(&self.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn reverse_preserves_existing_and_aliases() {
    let s = Sandbox::new();
    s.write("out", b"abcdef");
    let o = s.run("xxd", &["-r", "-", "out"], b"00000002: 5859  XY\n");
    assert!(o.status.success());
    assert_eq!(fs::read(s.0.join("out")).unwrap(), b"abXYef");
    s.write("same", b"00000000: 5859  XY\n");
    assert!(s.run("xxd", &["-r", "same", "same"], b"").status.success());
    assert_eq!(&fs::read(s.0.join("same")).unwrap()[..2], b"XY");
}
#[test]
fn reverse_adjustments_and_nibbles() {
    let s = Sandbox::new();
    assert_eq!(
        s.run("xxd", &["-r", "-s", "2"], b"00000000: 4142  AB\n")
            .stdout,
        b"\0\0AB"
    );
    assert_eq!(
        s.run("xxd", &["-r", "-s", "-2"], b"00000004: 4142  AB\n")
            .stdout,
        b"\0\0AB"
    );
    assert_eq!(s.run("xxd", &["-r", "-p"], b"4\n1\n4\t2").stdout, b"AB");
}
#[test]
fn exact_short_rows_and_include() {
    let s = Sandbox::new();
    assert_eq!(
        s.run("xxd", &["-b"], b"A").stdout,
        b"00000000: 01000001                                               A\n"
    );
    assert_eq!(
        s.run("xxd", &["-e"], b"ABC").stdout,
        b"00000000:   434241                             ABC\n"
    );
    assert_eq!(
        s.run(
            "xxd",
            &["-i", "-n", "buf", "-c", "3", "-u", "-C"],
            &[0, 1, 2, 3]
        )
        .stdout,
        b"unsigned char BUF[] = {\n  0X00, 0X01, 0X02,\n  0X03\n};\nunsigned int BUF_LEN = 4;\n"
    );
}
#[test]
fn hardlinks_are_not_corrupted() {
    let s = Sandbox::new();
    s.write("out", b"abcdef");
    fs::hard_link(s.0.join("out"), s.0.join("linked")).unwrap();
    assert!(s
        .run("xxd", &["-r", "-", "out"], b"00000002: 5859  XY\n")
        .status
        .success());
    assert_eq!(fs::read(s.0.join("linked")).unwrap(), b"abcdef");
    assert_eq!(fs::read(s.0.join("out")).unwrap(), b"abXYef");
}
#[test]
fn file_elf_and_pe_headers() {
    let s = Sandbox::new();
    let mut elf = vec![0; 64];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x02\x01");
    elf[16..18].copy_from_slice(&1u16.to_be_bytes());
    elf[18..20].copy_from_slice(&183u16.to_be_bytes());
    s.write("elf", &elf);
    assert_eq!(
        s.run("file", &["-b", "elf"], b"").stdout,
        b"ELF 64-bit MSB relocatable, ARM aarch64\n"
    );
    let mut pe = vec![0; 256];
    pe[..2].copy_from_slice(b"MZ");
    pe[60..64].copy_from_slice(&128u32.to_le_bytes());
    pe[128..132].copy_from_slice(b"PE\0\0");
    pe[132..134].copy_from_slice(&0x8664u16.to_le_bytes());
    pe[152..154].copy_from_slice(&0x20bu16.to_le_bytes());
    pe[220..222].copy_from_slice(&2u16.to_le_bytes());
    s.write("pe", &pe);
    assert_eq!(
        s.run("file", &["-b", "pe"], b"").stdout,
        b"PE32+ executable (GUI) x86-64, for MS Windows\n"
    );
    s.write("mz", b"MZ plain text\n");
    assert_eq!(s.run("file", &["-b", "mz"], b"").stdout, b"ASCII text\n");
}
#[test]
fn file_text_features() {
    let s = Sandbox::new();
    s.write("u8", "é\r\n".as_bytes());
    assert_eq!(
        s.run("file", &["-b", "u8"], b"").stdout,
        "Unicode text, UTF-8 text, with CRLF line terminators\n".as_bytes()
    );
    s.write("u16", &[0xff, 0xfe, 65, 0, 13, 0, 10, 0]);
    assert_eq!(
        s.run("file", &["-b", "u16"], b"").stdout,
        b"Unicode text, UTF-16, little-endian, with CRLF line terminators\n"
    );
    let mut line = vec![b'a'; 1000];
    line.push(b'\n');
    s.write("long", &line);
    assert_eq!(
        s.run("file", &["-b", "long"], b"").stdout,
        b"ASCII text, with very long lines\n"
    );
    s.write("controls", &[1, 2, 3]);
    assert_eq!(s.run("file", &["-b", "controls"], b"").stdout, b"data\n");
    let mut split = vec![b'a'; 65535];
    split.extend_from_slice("é".as_bytes());
    s.write("split", &split);
    assert!(
        String::from_utf8(s.run("file", &["-b", "split"], b"").stdout)
            .unwrap()
            .contains("UTF-8")
    );
}
#[test]
fn names_from_stdin_combine_operands() {
    let s = Sandbox::new();
    s.write("a", b"");
    s.write("b", b"a\n");
    assert_eq!(
        s.run("file", &["-f", "-", "b"], b"a\n").stdout,
        b"a: empty\nb: ASCII text\n"
    );
}
#[cfg(unix)]
#[test]
fn unix_raw_filename_list() {
    use std::os::unix::ffi::OsStringExt;
    let s = Sandbox::new();
    let name = std::ffi::OsString::from_vec(vec![b'x', 0xff]);
    fs::write(s.0.join(name), b"").unwrap();
    let o = s.run("file", &["-b", "-f", "-"], b"x\xff\n");
    assert_eq!(o.stdout, b"empty\n");
}
