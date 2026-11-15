use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::{ffi::OsString, fs, io::Read, path::Path};

const HELP: &str = "Usage: file [OPTION...] FILE...\nDetermine file type.\n  -b, --brief             do not prepend filenames\n  -i, --mime              output MIME type and encoding\n      --mime-type         output MIME type\n      --mime-encoding     output MIME encoding\n  -L, --dereference       follow symbolic links\n  -h, --no-dereference    do not follow symbolic links\n  -f, --files-from=FILE   read names from FILE\n  -0, --print0            terminate output with NUL\n      --help              display this help\n      --version           output version information\n";
#[derive(Default)]
struct Opt {
    brief: bool,
    mime: Option<u8>,
    follow: bool,
    from: Option<String>,
    nul: bool,
    files: Vec<String>,
}
fn parse(a: &[String]) -> Result<Opt, i32> {
    let s = [
        OptSpec::flag("brief", 'b'),
        OptSpec::flag("mime", 'i'),
        OptSpec::flag("mime-type", ' '),
        OptSpec::flag("mime-encoding", ' '),
        OptSpec::flag("dereference", 'L'),
        OptSpec::flag("no-dereference", 'h'),
        OptSpec::value("files-from", 'f'),
        OptSpec::flag("print0", '0'),
    ];
    let it = args::parse_command("file", a, HELP, &s, true)?;
    let mut o = Opt::default();
    for x in it {
        match x {
            Item::Operand(v) => o.files.push(v),
            Item::Opt("brief", _) => o.brief = true,
            Item::Opt("mime", _) => o.mime = Some(0),
            Item::Opt("mime-type", _) => o.mime = Some(1),
            Item::Opt("mime-encoding", _) => o.mime = Some(2),
            Item::Opt("dereference", _) => o.follow = true,
            Item::Opt("no-dereference", _) => o.follow = false,
            Item::Opt("files-from", Some(v)) => o.from = Some(v),
            Item::Opt("print0", _) => o.nul = true,
            _ => unreachable!(),
        }
    }
    Ok(o)
}
fn has_elf_interpreter(data: &[u8]) -> bool {
    let Some(class) = data.get(4).copied() else {
        return false;
    };
    let little = data.get(5) == Some(&1);
    let (off, entsize, count) = if class == 2 {
        let Some(bytes) = data.get(32..40) else {
            return false;
        };
        (
            read_u64(bytes, little) as usize,
            data.get(54..56)
                .map(|b| read_u16(b, little) as usize)
                .unwrap_or(0),
            data.get(56..58)
                .map(|b| read_u16(b, little) as usize)
                .unwrap_or(0),
        )
    } else if class == 1 {
        let Some(bytes) = data.get(28..32) else {
            return false;
        };
        (
            read_u32(bytes, little) as usize,
            data.get(42..44)
                .map(|b| read_u16(b, little) as usize)
                .unwrap_or(0),
            data.get(44..46)
                .map(|b| read_u16(b, little) as usize)
                .unwrap_or(0),
        )
    } else {
        return false;
    };
    (0..count.min(1024)).any(|i| {
        let pos = off.saturating_add(i.saturating_mul(entsize));
        data.get(pos..pos.saturating_add(4))
            .is_some_and(|b| read_u32(b, little) == 3)
    })
}
fn read_u16(b: &[u8], little: bool) -> u16 {
    let a = [b[0], b[1]];
    if little {
        u16::from_le_bytes(a)
    } else {
        u16::from_be_bytes(a)
    }
}
fn read_u32(b: &[u8], little: bool) -> u32 {
    let a = [b[0], b[1], b[2], b[3]];
    if little {
        u32::from_le_bytes(a)
    } else {
        u32::from_be_bytes(a)
    }
}
fn read_u64(b: &[u8], little: bool) -> u64 {
    let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
    if little {
        u64::from_le_bytes(a)
    } else {
        u64::from_be_bytes(a)
    }
}
fn classify(data: &[u8], is_dir: bool) -> (&'static str, &'static str, String) {
    if is_dir {
        return ("directory", "inode/directory", String::new());
    }
    if data.is_empty() {
        return ("empty", "inode/x-empty", String::new());
    }
    if data.starts_with(b"\x7fELF") {
        let width = if data.get(4) == Some(&1) { "32" } else { "64" };
        let little = data.get(5) != Some(&2);
        let order = if little { "LSB" } else { "MSB" };
        let number = |offset| {
            data.get(offset..offset + 2)
                .map(|v| read_u16(v, little))
                .unwrap_or(0)
        };
        let machine = match number(18) {
            62 => "x86-64",
            3 => "Intel 80386",
            183 => "ARM aarch64",
            40 => "ARM",
            _ => "unknown arch",
        };
        let kind = match number(16) {
            1 => "relocatable",
            2 => "executable",
            3 if has_elf_interpreter(data) => "pie executable",
            3 => "shared object",
            4 => "core file",
            _ => "unknown type",
        };
        let mime = match number(16) {
            1 => "application/x-object",
            3 if has_elf_interpreter(data) => "application/x-pie-executable",
            3 => "application/x-sharedlib",
            4 => "application/x-coredump",
            _ => "application/x-executable",
        };
        return (
            "ELF",
            mime,
            format!(", {width}-bit {order} {kind}, {machine}"),
        );
    }
    if data.starts_with(&[0xce, 0xfa, 0xed, 0xfe])
        || data.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
        || data.starts_with(&[0xfe, 0xed, 0xfa, 0xce])
        || data.starts_with(&[0xfe, 0xed, 0xfa, 0xcf])
    {
        return (
            "Mach-O executable",
            "application/x-mach-binary",
            String::new(),
        );
    }
    if data.starts_with(b"MZ") {
        if let Some(pe) = data.get(0x3c..0x40).map(|s| read_u32(s, true) as usize) {
            if data.get(pe..pe.saturating_add(4)) == Some(b"PE\0\0") {
                let number = |offset: usize| {
                    data.get(pe.saturating_add(offset)..pe.saturating_add(offset + 2))
                        .map(|s| read_u16(s, true))
                };
                if let (Some(machine), Some(magic), Some(subsystem)) =
                    (number(4), number(24), number(92))
                {
                    if magic == 0x10b || magic == 0x20b {
                        let width = if magic == 0x20b { "PE32+" } else { "PE32" };
                        let machine = match machine {
                            0x8664 => "x86-64",
                            0x14c => "Intel 80386",
                            0xaa64 => "Aarch64",
                            _ => "unknown architecture",
                        };
                        let subsystem = match subsystem {
                            2 => "GUI",
                            3 => "console",
                            _ => "unknown subsystem",
                        };
                        return (
                            width,
                            "application/vnd.microsoft.portable-executable",
                            format!(" executable ({subsystem}) {machine}, for MS Windows"),
                        );
                    }
                }
            }
        }
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let dims = if data.len() >= 24 {
            format!(
                ", {} x {}",
                u32::from_be_bytes(data[16..20].try_into().unwrap()),
                u32::from_be_bytes(data[20..24].try_into().unwrap())
            )
        } else {
            String::new()
        };
        return ("PNG image data", "image/png", dims);
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        let dims = if data.len() >= 10 {
            format!(
                ", {} x {}",
                u16::from_le_bytes([data[6], data[7]]),
                u16::from_le_bytes([data[8], data[9]])
            )
        } else {
            String::new()
        };
        return ("GIF image data", "image/gif", dims);
    }
    if data.starts_with(&[0xff, 0xd8, 0xff]) {
        let extra = if data.windows(4).any(|w| w == b"JFIF") {
            ", JFIF standard".to_owned()
        } else if data.windows(4).any(|w| w == b"Exif") {
            ", Exif standard".to_owned()
        } else {
            String::new()
        };
        return ("JPEG image data", "image/jpeg", extra);
    }
    if data.starts_with(b"%PDF-") {
        let v = String::from_utf8_lossy(&data[5..data.len().min(8)]);
        return ("PDF document", "application/pdf", v.into_owned());
    }
    if data.starts_with(b"PK\x03\x04") {
        return ("Zip archive data", "application/zip", String::new());
    }
    if data.starts_with(&[0x1f, 0x8b]) {
        return ("gzip compressed data", "application/gzip", String::new());
    }
    if data.starts_with(b"BZh") {
        return (
            "bzip2 compressed data",
            "application/x-bzip2",
            String::new(),
        );
    }
    if data.starts_with(b"\xfd7zXZ\0") {
        return ("XZ compressed data", "application/x-xz", String::new());
    }
    if data.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return (
            "7-zip archive data",
            "application/x-7z-compressed",
            String::new(),
        );
    }
    if data.starts_with(b"!<arch>\n") {
        return ("current ar archive", "application/x-archive", String::new());
    }
    if data.len() >= 262 && &data[257..262] == b"ustar" {
        return ("POSIX tar archive", "application/x-tar", String::new());
    }
    if data.starts_with(b"SQLite format 3\0") {
        return (
            "SQLite 3.x database",
            "application/vnd.sqlite3",
            String::new(),
        );
    }
    if data.starts_with(b"\0asm") {
        return (
            "WebAssembly (wasm) binary module version 0x1 (MVP)",
            "application/wasm",
            String::new(),
        );
    }
    if data.starts_with(b"#!") {
        let first = data.split(|b| *b == b'\n').next().unwrap_or_default();
        let script = if first.windows(6).any(|w| w == b"python") {
            "Python script"
        } else if first.windows(4).any(|w| w == b"perl") {
            "Perl script"
        } else if first.windows(4).any(|w| w == b"ruby") {
            "Ruby script"
        } else {
            "script"
        };
        return (
            if script == "script" {
                "script, ASCII text executable"
            } else if script == "Python script" {
                "Python script, ASCII text executable"
            } else if script == "Perl script" {
                "Perl script, ASCII text executable"
            } else {
                "Ruby script, ASCII text executable"
            },
            match script {
                "Python script" => "text/x-script.python",
                "Perl script" => "text/x-perl",
                "Ruby script" => "text/x-ruby",
                _ => "text/x-shellscript",
            },
            String::new(),
        );
    }
    if let Some((desc, extra)) = text_type(data) {
        return (desc, "text/plain", extra);
    }
    ("data", "application/octet-stream", String::new())
}
fn text_type(data: &[u8]) -> Option<(&'static str, String)> {
    let (desc, chars): (&str, Vec<char>) =
        if data.starts_with(&[0xff, 0xfe]) || data.starts_with(&[0xfe, 0xff]) {
            let little = data[0] == 0xff;
            let units: Vec<u16> = data[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| read_u16(c, little))
                .collect();
            let chars = std::char::decode_utf16(units)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            (
                if little {
                    "Unicode text, UTF-16, little-endian"
                } else {
                    "Unicode text, UTF-16, big-endian"
                },
                chars,
            )
        } else {
            let text = match std::str::from_utf8(data) {
                Ok(s) => s,
                Err(e) if e.error_len().is_none() && data.len() >= 65536 => {
                    std::str::from_utf8(&data[..e.valid_up_to()]).ok()?
                }
                Err(_) => return None,
            };
            let desc = if data.starts_with(&[0xef, 0xbb, 0xbf]) {
                "Unicode text, UTF-8 (with BOM)"
            } else if !text.is_ascii() {
                "Unicode text, UTF-8 text"
            } else {
                "ASCII text"
            };
            (desc, text.chars().collect())
        };
    if chars
        .iter()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\x08' | '\x0c' | '\x1b'))
    {
        return None;
    }
    let mut extra = String::new();
    let longest = chars
        .split(|c| *c == '\n')
        .map(|line| line.len())
        .max()
        .unwrap_or(0);
    if longest > 300 {
        extra.push_str(", with very long lines");
    }
    if chars.windows(2).any(|w| w == ['\r', '\n']) {
        extra.push_str(", with CRLF line terminators");
    }
    if !chars.contains(&'\n') && !chars.contains(&'\r') {
        extra.push_str(", with no line terminators");
    }
    Some((desc, extra))
}

fn inspect(path: &Path, follow: bool) -> Result<(String, String, String), String> {
    if path == Path::new("-") {
        let mut bytes = Vec::new();
        std::io::stdin()
            .lock()
            .take(65540)
            .read_to_end(&mut bytes)
            .map_err(|e| error::io_text(&e))?;
        let (description, mime, extra) = classify(&bytes, false);
        return Ok((description.into(), mime.into(), extra));
    }
    let display = path.to_string_lossy();
    let meta = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
    .map_err(|e| format!("cannot open `{display}' ({})", error::io_text(&e)))?;
    if meta.file_type().is_symlink() && !follow {
        let target = fs::read_link(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Ok((
            format!("symbolic link to {target}"),
            "inode/symlink".into(),
            String::new(),
        ));
    }
    if meta.is_dir() {
        return Ok(("directory".into(), "inode/directory".into(), String::new()));
    }
    let f = fs::File::open(path)
        .map_err(|e| format!("cannot open `{display}' ({})", error::io_text(&e)))?;
    let mut buf = Vec::new();
    f.take(65540)
        .read_to_end(&mut buf)
        .map_err(|e| error::io_text(&e))?;
    let n = buf.len();
    let (d, m, x) = classify(&buf[..n], false);
    Ok((d.into(), m.into(), x))
}
pub fn run(a: &[String]) -> i32 {
    let o = match parse(a) {
        Ok(x) => x,
        Err(c) => return c,
    };
    let mut files: Vec<OsString> = o.files.iter().map(OsString::from).collect();
    if let Some(p) = &o.from {
        let bytes = if p == "-" {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).map(|_| bytes)
        } else {
            fs::read(p)
        };
        match bytes {
            Ok(bytes) => {
                let names = bytes
                    .split(|b| *b == b'\n')
                    .filter(|s| !s.is_empty())
                    .map(filename);
                files.splice(0..0, names);
            }
            Err(e) => {
                eprintln!("file: {p}: {}", error::io_text(&e));
                return 1;
            }
        }
    }
    if files.is_empty() {
        return error::usage("file", "missing file operand", 1);
    }
    io::output("file", |out| {
        for p in files {
            let (desc, mime, extra) = match inspect(Path::new(&p), o.follow) {
                Ok(x) => x,
                Err(e) => {
                    if !o.brief {
                        write!(out, "{}: ", p.to_string_lossy())?;
                    }
                    writeln!(out, "{e}")?;
                    continue;
                }
            };
            let result = match o.mime {
                Some(1) => mime,
                Some(2) => encoding(&mime, &desc).to_owned(),
                Some(_) => format!("{mime}; charset={}", encoding(&mime, &desc)),
                None => {
                    if extra.is_empty() {
                        desc
                    } else if desc == "ELF" {
                        format!("{desc} {}", extra.trim_start_matches(", "))
                    } else if desc == "PE32" || desc == "PE32+" || extra.starts_with(", ") {
                        format!("{desc}{extra}")
                    } else {
                        format!("{desc}, version {extra}")
                    }
                }
            };
            if !o.brief {
                write!(out, "{}: ", p.to_string_lossy())?
            }
            write!(out, "{result}")?;
            out.write_all(&[if o.nul { 0 } else { b'\n' }])?;
        }
        Ok(())
    })
}

#[cfg(unix)]
fn filename(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}
#[cfg(not(unix))]
fn filename(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

fn encoding(mime: &str, description: &str) -> &'static str {
    if !mime.starts_with("text/") {
        "binary"
    } else if description.contains("UTF-16, little-endian") {
        "utf-16le"
    } else if description.contains("UTF-16, big-endian") {
        "utf-16be"
    } else if description.starts_with("Unicode") {
        "utf-8"
    } else {
        "us-ascii"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_type() {
        assert_eq!(classify(&[], false).0, "empty");
    }
    #[test]
    fn png_dimensions() {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&3u32.to_be_bytes());
        b.extend_from_slice(&7u32.to_be_bytes());
        assert_eq!(classify(&b, false).2, ", 3 x 7");
    }
    #[test]
    fn text_and_binary() {
        assert!(classify(b"hi\r\n", false).2.contains("CRLF"));
        assert_eq!(classify(&[0, 1, 2], false).0, "data");
    }
    #[test]
    fn signatures_and_mime_goldens() {
        let signatures: &[(&[u8], &str, &str)] = &[
            (b"\x7fELF\x02\x01\x01", "ELF", "application/x-executable"),
            (
                b"\xcf\xfa\xed\xfe",
                "Mach-O executable",
                "application/x-mach-binary",
            ),
            (
                b"#!/usr/bin/python\n",
                "Python script, ASCII text executable",
                "text/x-script.python",
            ),
            (b"\x89PNG\r\n\x1a\n", "PNG image data", "image/png"),
            (b"\xff\xd8\xff", "JPEG image data", "image/jpeg"),
            (b"GIF89a", "GIF image data", "image/gif"),
            (b"%PDF-1.7", "PDF document", "application/pdf"),
            (b"PK\x03\x04", "Zip archive data", "application/zip"),
            (b"\x1f\x8b", "gzip compressed data", "application/gzip"),
            (b"BZh", "bzip2 compressed data", "application/x-bzip2"),
            (b"\xfd7zXZ\0", "XZ compressed data", "application/x-xz"),
            (
                b"7z\xbc\xaf\x27\x1c",
                "7-zip archive data",
                "application/x-7z-compressed",
            ),
            (
                b"SQLite format 3\0",
                "SQLite 3.x database",
                "application/vnd.sqlite3",
            ),
            (
                b"\0asm\x01\0\0\0",
                "WebAssembly (wasm) binary module version 0x1 (MVP)",
                "application/wasm",
            ),
        ];
        for &(bytes, description, mime) in signatures {
            let result = classify(bytes, false);
            assert_eq!((result.0, result.1), (description, mime));
        }
        let mut tar = [0; 512];
        tar[257..262].copy_from_slice(b"ustar");
        assert_eq!(
            (classify(&tar, false).0, classify(&tar, false).1),
            ("POSIX tar archive", "application/x-tar")
        );
    }
}
