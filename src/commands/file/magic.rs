//! Magic signatures: binary formats recognised from their leading bytes, and `#!` scripts.

use super::text;
use super::Verdict;

/// Returns the verdict for a sample with a known signature.
pub fn detect(sample: &[u8]) -> Option<Verdict> {
    if sample.starts_with(b"\x7fELF") {
        return Some(elf(sample));
    }
    if sample.starts_with(b"MZ") {
        return Some(pe(sample));
    }
    if let Some(verdict) = macho(sample) {
        return Some(verdict);
    }
    if sample.starts_with(b"#!") {
        return Some(shebang(sample));
    }
    if sample.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(binary(png(sample), "image/png"));
    }
    if sample.starts_with(b"\xff\xd8\xff") {
        return Some(binary("JPEG image data".to_string(), "image/jpeg"));
    }
    if sample.starts_with(b"GIF8") {
        return Some(gif(sample));
    }
    if sample.starts_with(b"%PDF-") {
        return Some(pdf(sample));
    }
    if let Some(verdict) = simple(sample) {
        return Some(verdict);
    }
    None
}

/// Formats with a fixed description and MIME type.
fn simple(sample: &[u8]) -> Option<Verdict> {
    let table: &[(&[u8], &str, &str)] = &[
        (b"PK\x03\x04", "Zip archive data", "application/zip"),
        (&[0x1f, 0x8b], "gzip compressed data", "application/gzip"),
        (
            &[0xfd, b'7', b'z', b'X', b'Z', 0],
            "XZ compressed data",
            "application/x-xz",
        ),
        (
            &[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c],
            "7-zip archive data",
            "application/x-7z-compressed",
        ),
        (
            b"SQLite format 3\0",
            "SQLite 3.x database",
            "application/vnd.sqlite3",
        ),
        (
            b"\0asm",
            "WebAssembly (wasm) binary module version 0x1 (MVP)",
            "application/wasm",
        ),
    ];
    for (prefix, description, mime) in table {
        if sample.starts_with(prefix) {
            return Some(binary(description.to_string(), mime));
        }
    }
    if sample.len() >= 4 && sample.starts_with(b"BZh") && (b'1'..=b'9').contains(&sample[3]) {
        return Some(binary(
            "bzip2 compressed data".to_string(),
            "application/x-bzip2",
        ));
    }
    if sample.len() >= 265 && &sample[257..262] == b"ustar" {
        let description = if &sample[257..265] == b"ustar  \0" {
            "POSIX tar archive (GNU)"
        } else {
            "POSIX tar archive"
        };
        return Some(binary(description.to_string(), "application/x-tar"));
    }
    None
}

fn binary(description: String, mime: &str) -> Verdict {
    Verdict {
        description,
        mime: mime.to_string(),
        charset: "binary".to_string(),
    }
}

fn read_u16(sample: &[u8], at: usize, big: bool) -> u16 {
    match sample.get(at..at + 2) {
        Some(bytes) if big => u16::from_be_bytes([bytes[0], bytes[1]]),
        Some(bytes) => u16::from_le_bytes([bytes[0], bytes[1]]),
        None => 0,
    }
}

fn read_u32(sample: &[u8], at: usize, big: bool) -> u32 {
    match sample.get(at..at + 4) {
        Some(bytes) => {
            let array = [bytes[0], bytes[1], bytes[2], bytes[3]];
            if big {
                u32::from_be_bytes(array)
            } else {
                u32::from_le_bytes(array)
            }
        }
        None => 0,
    }
}

fn elf(sample: &[u8]) -> Verdict {
    if sample.len() < 20 {
        return binary("ELF".to_string(), "application/x-executable");
    }
    let bits = match sample[4] {
        1 => "32-bit",
        2 => "64-bit",
        _ => "unknown class",
    };
    let big = sample[5] == 2;
    let order = if big { "MSB" } else { "LSB" };
    let (kind, mime) = match read_u16(sample, 16, big) {
        1 => ("relocatable", "application/x-object"),
        2 => ("executable", "application/x-executable"),
        3 => ("shared object", "application/x-sharedlib"),
        4 => ("core file", "application/x-coredump"),
        _ => ("unknown type", "application/octet-stream"),
    };
    let arch = match read_u16(sample, 18, big) {
        0x03 => "Intel 80386",
        0x3e => "x86-64",
        0x28 => "ARM",
        0xb7 => "ARM aarch64",
        0xf3 => "RISC-V",
        0x08 => "MIPS",
        0x14 => "PowerPC",
        0x15 => "PowerPC64",
        0x16 => "IBM S/390",
        _ => "unknown arch",
    };
    binary(
        format!("ELF {bits} {order} {kind}, {arch}, version 1 (SYSV)"),
        mime,
    )
}

/// PE files: `PE32`/`PE32+` with architecture, DLL flag and subsystem; otherwise MS-DOS.
fn pe(sample: &[u8]) -> Verdict {
    let offset = read_u32(sample, 0x3c, false) as usize;
    if sample.get(offset..offset + 4) != Some(&b"PE\0\0"[..]) {
        return binary("MS-DOS executable".to_string(), "application/x-dosexec");
    }
    let header = offset + 4;
    let arch = match read_u16(sample, header, false) {
        0x014c => "Intel 80386",
        0x8664 => "x86-64",
        0x01c0 => "ARM",
        0xaa64 => "Aarch64",
        _ => "unknown arch",
    };
    let dll = read_u16(sample, header + 18, false) & 0x2000 != 0;
    let optional = header + 20;
    let format = if read_u16(sample, optional, false) == 0x20b {
        "PE32+"
    } else {
        "PE32"
    };
    let subsystem = match read_u16(sample, optional + 68, false) {
        2 => "GUI",
        3 => "console",
        _ => "unknown",
    };
    let dll_text = if dll { " (DLL)" } else { "" };
    binary(
        format!("{format} executable{dll_text} ({subsystem}) {arch}, for MS Windows"),
        "application/x-dosexec",
    )
}

/// Mach-O thin binaries in either byte order.
fn macho(sample: &[u8]) -> Option<Verdict> {
    let (big, wide) = match sample.get(0..4)? {
        [0xfe, 0xed, 0xfa, 0xce] => (true, false),
        [0xfe, 0xed, 0xfa, 0xcf] => (true, true),
        [0xce, 0xfa, 0xed, 0xfe] => (false, false),
        [0xcf, 0xfa, 0xed, 0xfe] => (false, true),
        _ => return None,
    };
    let arch = match read_u32(sample, 4, big) {
        0x0100_0007 => "x86_64",
        0x0000_0007 => "i386",
        0x0100_000c => "arm64",
        0x0000_000c => "arm",
        _ => "unknown arch",
    };
    let kind = match read_u32(sample, 12, big) {
        1 => "object",
        2 => "executable",
        6 => "dynamically linked shared library",
        8 => "bundle",
        _ => "file",
    };
    let bits = if wide { "64" } else { "32" };
    Some(binary(
        format!("Mach-O {bits}-bit {arch} {kind}"),
        "application/x-mach-binary",
    ))
}

/// PNG: dimensions, colour type, bit depth and interlacing from the IHDR chunk.
fn png(sample: &[u8]) -> String {
    if sample.len() < 29 {
        return "PNG image data".to_string();
    }
    let width = read_u32(sample, 16, true);
    let height = read_u32(sample, 20, true);
    let depth = sample[24];
    let color = match sample[25] {
        0 => format!("{depth}-bit grayscale"),
        2 => format!("{depth}-bit/color RGB"),
        3 => format!("{depth}-bit colormap"),
        4 => format!("{depth}-bit/color gray+alpha"),
        6 => format!("{depth}-bit/color RGBA"),
        _ => format!("{depth}-bit"),
    };
    let interlace = if sample[28] == 1 {
        "interlaced"
    } else {
        "non-interlaced"
    };
    format!("PNG image data, {width} x {height}, {color}, {interlace}")
}

fn gif(sample: &[u8]) -> Verdict {
    if sample.len() < 10 {
        return binary("GIF image data".to_string(), "image/gif");
    }
    let version = String::from_utf8_lossy(&sample[3..6]).into_owned();
    let width = read_u16(sample, 6, false);
    let height = read_u16(sample, 8, false);
    binary(
        format!("GIF image data, version {version}, {width} x {height}"),
        "image/gif",
    )
}

fn pdf(sample: &[u8]) -> Verdict {
    let version: String = sample
        .get(5..sample.len().min(8))
        .unwrap_or(&[])
        .iter()
        .take_while(|&&b| b.is_ascii_digit() || b == b'.')
        .map(|&b| b as char)
        .collect();
    let description = if version.is_empty() {
        "PDF document".to_string()
    } else {
        format!("PDF document, version {version}")
    };
    binary(description, "application/pdf")
}

/// `#!` scripts: the interpreter from the first line (through `env` when used), with the
/// text encoding appended when the script is text.
fn shebang(sample: &[u8]) -> Verdict {
    let end = sample
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(sample.len())
        .min(256);
    let line = String::from_utf8_lossy(&sample[2..end]).into_owned();
    let mut words = line.split_whitespace();
    let first = words.next().unwrap_or("");
    let interpreter = if basename(first) == "env" {
        words.find(|w| !w.starts_with('-')).unwrap_or("")
    } else {
        first
    };
    let name = basename(interpreter);
    let (script, mime) = if name.starts_with("python") {
        ("Python script".to_string(), "text/x-python")
    } else {
        match name {
            "perl" => ("Perl script".to_string(), "text/x-perl"),
            "ruby" => ("Ruby script".to_string(), "text/x-ruby"),
            "bash" => (
                "Bourne-Again shell script".to_string(),
                "text/x-shellscript",
            ),
            "sh" | "dash" | "ash" | "ksh" => {
                ("POSIX shell script".to_string(), "text/x-shellscript")
            }
            "node" => ("Node.js script".to_string(), "text/javascript"),
            "awk" | "gawk" => ("awk script".to_string(), "text/x-awk"),
            _ => (format!("a {interpreter} script"), "text/plain"),
        }
    };
    match text::encoding(sample) {
        Some(encoding) => Verdict {
            description: format!(
                "{script}, {} executable{}",
                encoding.base,
                text::suffix(sample)
            ),
            mime: mime.to_string(),
            charset: encoding.charset.to_string(),
        },
        None => binary(script, mime),
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(sample: &[u8]) -> String {
        detect(sample).expect("signature").description
    }

    #[test]
    fn elf_reports_class_type_and_machine() {
        let mut elf = vec![0u8; 64];
        elf[..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[16] = 2;
        elf[18] = 0x3e;
        assert_eq!(
            desc(&elf),
            "ELF 64-bit LSB executable, x86-64, version 1 (SYSV)"
        );
    }

    #[test]
    fn pe_reports_format_subsystem_and_dll() {
        // e_lfanew = 0x80; the COFF header starts at 0x84 and the optional header at 0x98.
        let mut pe = vec![0u8; 0x200];
        pe[0] = b'M';
        pe[1] = b'Z';
        pe[0x3c] = 0x80;
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84] = 0x64; // machine 0x8664, little endian
        pe[0x85] = 0x86;
        pe[0x98] = 0x0b; // optional header magic 0x020b (PE32+)
        pe[0x99] = 0x02;
        pe[0xdc] = 3; // subsystem 3 (console) at optional header + 68
        assert_eq!(
            desc(&pe),
            "PE32+ executable (console) x86-64, for MS Windows"
        );
    }

    #[test]
    fn png_gif_pdf_and_archives() {
        let mut png = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        png.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        assert_eq!(
            desc(&png),
            "PNG image data, 1 x 1, 8-bit/color RGBA, non-interlaced"
        );
        assert_eq!(
            desc(b"GIF89a\x01\x00\x02\x00"),
            "GIF image data, version 89a, 1 x 2"
        );
        assert_eq!(desc(b"%PDF-1.7\n%"), "PDF document, version 1.7");
        assert_eq!(desc(b"PK\x03\x04rest"), "Zip archive data");
        assert_eq!(desc(&[0x1f, 0x8b, 8, 0]), "gzip compressed data");
        assert_eq!(desc(b"BZh91AY"), "bzip2 compressed data");
        assert!(detect(b"BZhx").is_none());
    }

    #[test]
    fn scripts_name_their_interpreter() {
        assert_eq!(
            desc(b"#!/usr/bin/env python3\nprint(1)\n"),
            "Python script, ASCII text executable"
        );
        assert_eq!(
            desc(b"#!/bin/sh\necho\n"),
            "POSIX shell script, ASCII text executable"
        );
        assert_eq!(
            desc(b"#!/bin/bash\r\necho\r\n"),
            "Bourne-Again shell script, ASCII text executable, with CRLF line terminators"
        );
        assert_eq!(
            desc(b"#!/opt/tool\n"),
            "a /opt/tool script, ASCII text executable"
        );
    }

    #[test]
    fn tar_and_macho_are_recognised() {
        let mut tar = vec![0u8; 512];
        tar[257..263].copy_from_slice(b"ustar\0");
        assert_eq!(desc(&tar), "POSIX tar archive");
        let macho = [
            0xcf, 0xfa, 0xed, 0xfe, 0x07, 0, 0, 0x01, 0, 0, 0, 0, 2, 0, 0, 0,
        ];
        assert_eq!(desc(&macho), "Mach-O 64-bit x86_64 executable");
    }
}
