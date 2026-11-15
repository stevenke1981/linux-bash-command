use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct BoxDir(PathBuf);
impl BoxDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "agentcmd-p5-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn run(&self, name: &str, args: &[&str], input: &[u8]) -> Output {
        let bin = match name {
            "xxd" => env!("CARGO_BIN_EXE_xxd"),
            "file" => env!("CARGO_BIN_EXE_file"),
            _ => unreachable!(),
        };
        let mut c = Command::new(bin)
            .args(args)
            .current_dir(&self.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input).unwrap();
        c.wait_with_output().unwrap()
    }
    fn write(&self, n: &str, b: &[u8]) {
        fs::write(self.0.join(n), b).unwrap()
    }
}
impl Drop for BoxDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}
mod xxd_cli {
    use super::*;
    #[test]
    fn help_and_version() {
        let b = BoxDir::new();
        assert_eq!(b.run("xxd", &["--help"], b"").status.code(), Some(0));
        assert_eq!(b.run("xxd", &["--version"], b"").status.code(), Some(0));
    }
    #[test]
    fn normal_plain_binary_include() {
        let b = BoxDir::new();
        assert!(out(&b.run("xxd", &[], b"AB")).starts_with("00000000: 4142"));
        assert_eq!(out(&b.run("xxd", &["-p"], b"AB")), "4142\n");
        assert!(out(&b.run("xxd", &["-b"], b"A")).contains("01000001"));
        assert_eq!(
            out(&b.run("xxd", &["-i", "-n", "buf"], b"A")),
            "unsigned char buf[] = {\n  0x41\n};\nunsigned int buf_len = 1;\n"
        );
    }
    #[test]
    fn aliases_columns_seek_and_limit() {
        let b = BoxDir::new();
        assert_eq!(out(&b.run("xxd", &["-plain"], b"AB")), "4142\n");
        assert!(out(&b.run("xxd", &["-cols", "1", "-len", "1"], b"AB")).contains("00000000"));
        assert!(out(&b.run("xxd", &["-s", "1"], b"AB")).contains("00000001"));
    }
    #[test]
    fn endian_decimal_uppercase_and_autoskip() {
        let b = BoxDir::new();
        assert!(out(&b.run("xxd", &["-e", "-g", "2"], b"ABCD")).contains("4241 4443"));
        assert!(out(&b.run("xxd", &["-d", "-u", "-o", "16"], b"A")).starts_with("00000016: 41"));
        let mut zeros = vec![0; 16];
        zeros.push(65);
        assert!(!out(&b.run("xxd", &["-a"], &zeros)).contains("*"));
    }
    #[test]
    fn reverse_dump_and_plain() {
        let b = BoxDir::new();
        assert_eq!(
            b.run("xxd", &["-r"], b"00000000: 4142 43  ABC\n").stdout,
            b"ABC"
        );
        assert_eq!(b.run("xxd", &["-r", "-p"], b"414243\n").stdout, b"ABC");
    }
    #[test]
    fn errors_and_file_operand() {
        let b = BoxDir::new();
        assert_ne!(b.run("xxd", &["-c", "0"], b"").status.code(), Some(0));
        b.write("data", b"Q");
        assert!(out(&b.run("xxd", &["data"], b"")).contains("51"));
        assert_eq!(b.run("xxd", &["missing"], b"").status.code(), Some(2));
    }
    #[test]
    fn toggle_autoskip() {
        let b = BoxDir::new();
        let s = out(&b.run("xxd", &["-a", "-a"], &[0, 0, 0, 0, 65]));
        assert!(!s.contains("*"));
    }
    #[test]
    fn uppercase_and_capitalized_include() {
        let b = BoxDir::new();
        assert!(out(&b.run("xxd", &["-u"], b"a")).contains("61"));
        assert!(out(&b.run("xxd", &["-i", "-C", "-n", "buf"], b"a")).contains("BUF"));
    }
    #[test]
    fn reverse_gap_and_bad_plain() {
        let b = BoxDir::new();
        assert_eq!(
            b.run("xxd", &["-r"], b"00000002: 4142  AB\n").stdout,
            b"\0\0AB"
        );
        assert_eq!(b.run("xxd", &["-r", "-p"], b"41zz42\n").stdout, b"A");
    }
    #[test]
    fn output_operand_and_conflicting_modes() {
        let b = BoxDir::new();
        let o = b.run("xxd", &["-p", "-", "out"], b"abc");
        assert_eq!(o.status.code(), Some(0));
        assert_eq!(fs::read(b.0.join("out")).unwrap(), b"616263\n");
        assert_ne!(b.run("xxd", &["-i", "-p"], b"").status.code(), Some(0));
    }
    #[test]
    fn atomic_in_place_output_and_plus_seek() {
        let b = BoxDir::new();
        b.write("same", b"xy");
        let o = b.run("xxd", &["-p", "same", "same"], b"");
        assert_eq!(o.status.code(), Some(0));
        assert_eq!(fs::read(b.0.join("same")).unwrap(), b"7879\n");
        assert!(out(&b.run("xxd", &["-s", "+2"], b"ABCDE")).contains("00000002: 4344"));
    }
}
mod file_cli {
    use super::*;
    #[test]
    fn help_and_version() {
        let b = BoxDir::new();
        assert_eq!(b.run("file", &["--help"], b"").status.code(), Some(0));
        assert_eq!(b.run("file", &["--version"], b"").status.code(), Some(0));
    }
    #[test]
    fn dynamic_types_and_brief_mime() {
        let b = BoxDir::new();
        b.write("empty", b"");
        b.write("png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03");
        b.write("pdf", b"%PDF-1.7");
        b.write("elf", b"\x7fELF\x02\x01\x01");
        assert!(out(&b.run("file", &["empty"], b"")).contains("empty"));
        assert!(out(&b.run("file", &["-b", "png"], b"")).contains("2 x 3"));
        assert!(out(&b.run("file", &["-i", "pdf"], b"")).contains("application/pdf"));
        assert!(out(&b.run("file", &["elf"], b"")).contains("ELF"));
    }
    #[test]
    fn text_flags_missing_and_files_from() {
        let b = BoxDir::new();
        b.write("text", b"hello\r\n");
        b.write("names", b"text\n");
        assert!(out(&b.run("file", &["-b", "text"], b"")).contains("CRLF"));
        assert_eq!(b.run("file", &["-z", "text"], b"").status.code(), Some(1));
        assert!(out(&b.run("file", &["-f", "names"], b"")).contains("text:"));
        assert!(out(&b.run("file", &["absent"], b"")).contains("cannot open"));
    }
    #[test]
    fn nul_terminator_and_mime_suboptions() {
        let b = BoxDir::new();
        b.write("sample", b"hi");
        assert_eq!(
            b.run("file", &["-0", "-b", "sample"], b"").stdout,
            b"ASCII text, with no line terminators\n"
        );
        assert_eq!(
            out(&b.run("file", &["--mime-type", "sample"], b"")),
            "sample: text/plain\n"
        );
        assert_eq!(
            out(&b.run("file", &["--mime-encoding", "sample"], b"")),
            "sample: us-ascii\n"
        );
    }
    #[test]
    fn symlink_modes_and_binary_magic() {
        let b = BoxDir::new();
        b.write("target", b"hi");
        b.write("zip", b"PK\x03\x04");
        #[cfg(unix)]
        std::os::unix::fs::symlink("target", b.0.join("link")).unwrap();
        assert!(out(&b.run("file", &["-b", "zip"], b"")).contains("Zip archive"));
        #[cfg(unix)]
        {
            assert!(out(&b.run("file", &["-h", "link"], b"")).contains("symbolic link"));
            assert!(out(&b.run("file", &["-L", "link"], b"")).contains("ASCII text"));
        }
    }
    #[test]
    fn utf16_and_text_bom() {
        let b = BoxDir::new();
        b.write("le", &[0xff, 0xfe, 65, 0]);
        b.write("bom", &[0xef, 0xbb, 0xbf, 65]);
        assert!(out(&b.run("file", &["-b", "le"], b"")).contains("UTF-16"));
        assert!(out(&b.run("file", &["-b", "bom"], b"")).contains("BOM"));
    }
    #[test]
    fn missing_operand_and_unknown_flag() {
        let b = BoxDir::new();
        assert_ne!(b.run("file", &[], b"").status.code(), Some(0));
        assert_ne!(b.run("file", &["-z"], b"").status.code(), Some(0));
    }
    #[test]
    fn files_from_and_brief() {
        let b = BoxDir::new();
        b.write("x", b"");
        b.write("list", b"x\n");
        assert_eq!(out(&b.run("file", &["-b", "-f", "list"], b"")), "empty\n");
    }
    #[test]
    fn common_archive_and_script_signatures() {
        let b = BoxDir::new();
        for (name, bytes) in [
            ("gz", &b"\x1f\x8b"[..]),
            ("bz", &b"BZh"[..]),
            ("xz", &b"\xfd7zXZ\0"[..]),
            ("7z", &b"7z\xbc\xaf\x27\x1c"[..]),
            ("sql", &b"SQLite format 3\0"[..]),
            ("wasm", &b"\0asm\x01\0\0\0"[..]),
            ("script", &b"#!/bin/sh\n"[..]),
            ("ar", &b"!<arch>\n"[..]),
        ] {
            b.write(name, bytes);
        }
        let mut tar = vec![0u8; 512];
        tar[257..262].copy_from_slice(b"ustar");
        b.write("tar", &tar);
        for (name, marker) in [
            ("gz", "gzip"),
            ("bz", "bzip2"),
            ("xz", "XZ"),
            ("7z", "7-zip"),
            ("sql", "SQLite"),
            ("wasm", "WebAssembly"),
            ("script", "script"),
            ("ar", "ar archive"),
            ("tar", "tar archive"),
        ] {
            assert!(
                out(&b.run("file", &["-b", name], b"")).contains(marker),
                "{name}"
            );
        }
    }
    #[test]
    fn dimensions_and_archive_mime() {
        let b = BoxDir::new();
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&4u32.to_be_bytes());
        png.extend_from_slice(&9u32.to_be_bytes());
        b.write("p", &png);
        assert!(out(&b.run("file", &["-b", "p"], b"")).contains("4 x 9"));
        assert_eq!(
            out(&b.run("file", &["--mime-type", "p"], b"")),
            "p: image/png\n"
        );
    }
    #[test]
    fn generated_pe_macho_and_elf_variants() {
        let b = BoxDir::new();
        let mut pe = vec![0u8; 0x80 + 94];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
        pe[0xdc..0xde].copy_from_slice(&3u16.to_le_bytes());
        b.write("pe", &pe);
        let mut elf = vec![0u8; 20];
        elf[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        elf[16..18].copy_from_slice(&3u16.to_le_bytes());
        elf[18..20].copy_from_slice(&3u16.to_le_bytes());
        b.write("elf32", &elf);
        b.write("macho", &[0xcf, 0xfa, 0xed, 0xfe]);
        assert!(out(&b.run("file", &["-b", "pe"], b"")).contains("PE32+ executable"));
        assert!(out(&b.run("file", &["-b", "elf32"], b"")).contains("ELF 32-bit LSB shared object"));
        assert!(out(&b.run("file", &["-b", "macho"], b"")).contains("Mach-O"));
    }
    #[test]
    fn utf8_long_line_and_image_variants() {
        let b = BoxDir::new();
        b.write("utf8", "café".as_bytes());
        b.write("jpeg", &[0xff, 0xd8, 0xff, 0xe0, b'J', b'F', b'I', b'F']);
        b.write("gif", b"GIF89a\x02\0\x03\0");
        b.write("long", &vec![b'a'; 65536]);
        assert!(out(&b.run("file", &["-b", "utf8"], b"")).contains("UTF-8"));
        assert!(out(&b.run("file", &["-b", "jpeg"], b"")).contains("JPEG"));
        assert!(out(&b.run("file", &["-b", "gif"], b"")).contains("2 x 3"));
        assert!(out(&b.run("file", &["-b", "long"], b"")).contains("very long lines"));
    }
}
