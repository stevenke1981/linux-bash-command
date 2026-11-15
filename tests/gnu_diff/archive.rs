//! Independent, semantic archive comparisons. Never extract oracle archives.
use super::{capture, environment, inventory, Captured};
use std::{collections::BTreeMap, io::Read, path::Path, process::Command};

pub fn differences(
    modes: &[&str],
    actual: &Captured,
    oracle: &Captured,
    left: &Path,
    right: &Path,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    for mode in [
        "gzip_stdout",
        "gzip_files",
        "gzip_list",
        "tar_stdout",
        "tar_files",
    ] {
        if !modes.contains(&mode) {
            continue;
        }
        let same = match mode {
            "gzip_stdout" => equal(gunzip(&actual.stdout, left), gunzip(&oracle.stdout, right)),
            "gzip_files" => equal(files(left, false), files(right, false)),
            "gzip_list" => equal(list_sizes(&actual.stdout), list_sizes(&oracle.stdout)),
            "tar_stdout" => equal(tar(&actual.stdout, left), tar(&oracle.stdout, right)),
            "tar_files" => equal(files(left, true), files(right, true)),
            _ => unreachable!(),
        };
        if !same {
            reasons.push(mode);
        }
    }
    reasons
}

fn equal<T: PartialEq>(actual: Result<T, String>, oracle: Result<T, String>) -> bool {
    match (actual, oracle) {
        (Ok(a), Ok(b)) => a == b,
        (a, b) => {
            // Two malformed outputs must never count as a successful comparison.
            eprintln!(
                "archive normalization failed: actual={:?}, GNU={:?}",
                a.err(),
                b.err()
            );
            false
        }
    }
}

fn gunzip(bytes: &[u8], root: &Path) -> Result<Vec<u8>, String> {
    let mut command = Command::new("/usr/bin/gzip");
    command.args(["-dc"]);
    environment(&mut command, root, &serde_json::Value::Null);
    let decoded = capture(command, bytes, 10_000, None);
    if decoded.timed_out || decoded.code != 0 {
        return Err(format!(
            "GNU gzip validation: exit {}, timeout {}, {}",
            decoded.code,
            decoded.timed_out,
            String::from_utf8_lossy(&decoded.stderr)
        ));
    }
    Ok(decoded.stdout)
}

#[derive(Debug, PartialEq, Eq)]
struct Member {
    path: Vec<u8>,
    kind: u8,
    link: Option<Vec<u8>>,
    mode: u32,
    data: Vec<u8>,
}

fn tar(bytes: &[u8], root: &Path) -> Result<Vec<Member>, String> {
    let decoded;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        decoded = gunzip(bytes, root)?;
        decoded.as_slice()
    } else {
        bytes
    };
    members(bytes)
}

fn members(bytes: &[u8]) -> Result<Vec<Member>, String> {
    if bytes.len() < 512 {
        return Err("missing tar header or end block".to_owned());
    }
    let mut archive = tar::Archive::new(bytes);
    let entries = archive.entries().map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.header().entry_type();
        if kind.is_gnu_longname()
            || kind.is_gnu_longlink()
            || kind.is_pax_global_extensions()
            || kind.is_pax_local_extensions()
        {
            continue;
        }
        let path = member_path(&entry.path_bytes());
        let link = entry.link_name_bytes().map(|name| {
            if kind.is_hard_link() {
                member_path(&name)
            } else {
                name.into_owned()
            }
        });
        let mode = entry.header().mode().map_err(|e| e.to_string())? & 0o7777;
        let size = entry.size();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() as u64 != size {
            return Err("truncated tar entry".to_owned());
        }
        result.push(Member {
            path,
            kind: if kind.is_file() { b'0' } else { kind.as_byte() },
            link,
            mode,
            data,
        });
    }
    Ok(result)
}

fn member_path(bytes: &[u8]) -> Vec<u8> {
    let absolute = bytes.starts_with(b"/");
    let parts: Vec<_> = bytes
        .split(|b| *b == b'/')
        .filter(|part| !part.is_empty() && *part != b".")
        .collect();
    let mut path = if absolute { vec![b'/'] } else { Vec::new() };
    for (index, part) in parts.iter().enumerate() {
        if index != 0 {
            path.push(b'/');
        }
        path.extend_from_slice(part);
    }
    if path.is_empty() {
        path.push(b'.');
    }
    path
}

#[derive(PartialEq)]
enum Contents {
    Bytes(Vec<u8>),
    Archive(Vec<Member>),
}
type Inventory = BTreeMap<std::path::PathBuf, (u8, Contents, Option<u32>)>;

fn files(root: &Path, is_tar: bool) -> Result<Inventory, String> {
    inventory(root, false)
        .into_iter()
        .map(|(path, (kind, bytes, mode))| {
            let name = path.to_string_lossy();
            let contents = if kind == 0
                && is_tar
                && (name.ends_with(".tar") || name.ends_with(".tgz") || name.ends_with(".tar.gz"))
            {
                Contents::Archive(tar(&bytes, root)?)
            } else if kind == 0 && !is_tar && bytes.starts_with(&[0x1f, 0x8b]) {
                Contents::Bytes(gunzip(&bytes, root)?)
            } else {
                Contents::Bytes(bytes)
            };
            Ok((path, (kind, contents, mode)))
        })
        .collect()
}

fn list_sizes(bytes: &[u8]) -> Result<Vec<(u64, u64)>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let mut sizes = Vec::new();
    for line in text.lines() {
        let columns: Vec<_> = line.split_whitespace().collect();
        if let Some(index) = columns.iter().position(|column| column.ends_with('%')) {
            let pair = index
                .checked_sub(2)
                .and_then(|start| {
                    Some((
                        columns[start].parse().ok()?,
                        columns[start + 1].parse().ok()?,
                    ))
                })
                .ok_or_else(|| "invalid gzip size columns".to_owned())?;
            sizes.push(pair);
        }
    }
    if sizes.is_empty() {
        return Err("missing gzip size columns".to_owned());
    }
    Ok(sizes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(path: &str, mode: u32, modified: u64, data: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(mode);
        header.set_mtime(modified);
        header.set_cksum();
        builder.append_data(&mut header, path, data).unwrap();
        builder.into_inner().unwrap()
    }

    #[test]
    fn normalization_preserves_paths_modes_payloads_and_ignores_timestamps() {
        let a = members(&archive("./a", 0o640, 1, b"data")).unwrap();
        assert_eq!(a, members(&archive("a", 0o640, 99, b"data")).unwrap());
        assert_ne!(a, members(&archive("a", 0o644, 1, b"data")).unwrap());
        assert_ne!(a, members(&archive("a", 0o640, 1, b"dAta")).unwrap());
        assert_ne!(member_path(b"a/../b"), member_path(b"b"));
        assert_ne!(member_path(b"/a"), member_path(b"a"));
        assert_eq!(member_path(b"./a/\xff/"), b"a/\xff");
        let mut truncated = archive("a", 0o640, 1, b"data");
        truncated.truncate(514);
        assert!(members(&truncated).is_err());
        assert!(members(b"").is_err());
        assert!(!equal::<Vec<Member>>(Err("bad".into()), Err("bad".into())));
    }

    #[test]
    fn list_comparison_checks_sizes_without_ratio_name_or_timestamp_noise() {
        assert_eq!(
            list_sizes(b"compressed uncompressed ratio name\n24 4 25.0% a\n").unwrap(),
            vec![(24, 4)]
        );
        assert_eq!(
            list_sizes(b"defla 1234 Nov 15 10:00 24 4 -50.0% b\n").unwrap(),
            vec![(24, 4)]
        );
        assert!(list_sizes(b"invalid\n").is_err());
        assert!(list_sizes(b"x 4 25.0% a\n").is_err());
    }
}
