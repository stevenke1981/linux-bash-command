use std::{
    cmp::Ordering,
    io::{self, BufRead},
};

pub(super) fn output(
    name: &str,
    write: impl FnOnce(&mut dyn std::io::Write) -> io::Result<()>,
) -> io::Result<()> {
    use std::{fs, io::Write, path::Path};
    let supplied = Path::new(name);
    let destination = fs::canonicalize(supplied).unwrap_or_else(|_| supplied.to_path_buf());
    let metadata = fs::metadata(&destination).ok();
    // Devices and FIFOs retain their native streaming behavior.
    if metadata.as_ref().is_some_and(|m| !m.is_file()) {
        let file = fs::OpenOptions::new().write(true).open(&destination)?;
        let mut out = std::io::BufWriter::with_capacity(crate::common::io::BUFFER_SIZE, file);
        return write(&mut out).and_then(|()| out.flush());
    }
    let directory = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = crate::common::external::TemporaryFile::with_mode(directory, 0o666)?;
    let permissions = metadata.map_or_else(
        || temporary.file().metadata().map(|m| m.permissions()),
        |m| Ok(m.permissions()),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    {
        let mut out =
            std::io::BufWriter::with_capacity(crate::common::io::BUFFER_SIZE, temporary.file());
        write(&mut out)?;
        out.flush()?;
    }
    temporary.file().set_permissions(permissions)?;
    temporary.persist(&destination)
}

pub(super) fn buffer_size(value: &str) -> Result<usize, &'static str> {
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return Err("invalid buffer size");
    }
    let number: u128 = value[..digits].parse().map_err(|_| "invalid buffer size")?;
    let suffix = &value[digits..];
    let size = if suffix == "%" {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        number.saturating_mul(u128::from(system.total_memory())) / 100
    } else {
        let power = match suffix {
            "b" => 0,
            "" | "k" | "K" => 1,
            "M" => 2,
            "G" => 3,
            "T" => 4,
            "P" => 5,
            "E" => 6,
            "Z" => 7,
            "Y" => 8,
            "R" => 9,
            "Q" => 10,
            _ => return Err("invalid buffer size"),
        };
        number.saturating_mul(1024u128.pow(power))
    };
    Ok(usize::try_from(size).unwrap_or(usize::MAX).max(1))
}

pub(super) fn check(
    input: &mut dyn BufRead,
    delimiter: u8,
    unique: bool,
    compare: &dyn Fn(&[u8], &[u8]) -> Ordering,
    keys: &dyn Fn(&[u8], &[u8]) -> Ordering,
) -> io::Result<Option<(usize, Vec<u8>)>> {
    let mut previous: Option<Vec<u8>> = None;
    let mut row = Vec::new();
    let mut number = 0;
    while input.read_until(delimiter, &mut row)? != 0 {
        number += 1;
        if row.last() == Some(&delimiter) {
            row.pop();
        }
        if previous
            .as_ref()
            .is_some_and(|old| compare(old, &row).is_gt() || unique && keys(old, &row).is_eq())
        {
            return Ok(Some((number, row)));
        }
        previous = Some(std::mem::take(&mut row));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_rewrite_preserves_original_and_removes_temporary() {
        let root = std::env::temp_dir().join(format!(
            "sort-atomic-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("input");
        std::fs::write(&path, b"original").unwrap();
        let error = output(path.to_str().unwrap(), |writer| {
            writer.write_all(b"partially rewritten")?;
            Err(io::Error::other("injected write failure"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "injected write failure");
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sizes_use_sort_units_and_reject_unrecognized_suffixes() {
        for (value, expected) in [
            ("1", 1024),
            ("2b", 2),
            ("3K", 3072),
            ("1M", 1048576),
            ("0", 1),
        ] {
            assert_eq!(buffer_size(value), Ok(expected));
        }
        for value in ["", "-1", "1B", "1KB", "1.5K", "letters"] {
            assert!(buffer_size(value).is_err());
        }
        assert!(buffer_size("1%").unwrap() >= 1);
    }
    #[test]
    fn check_stops_at_the_first_disorder_or_duplicate() {
        let compare = |a: &[u8], b: &[u8]| a.cmp(b);
        assert_eq!(
            check(&mut &b"a\nc\nb\nx\n"[..], b'\n', false, &compare, &compare).unwrap(),
            Some((3, b"b".to_vec()))
        );
        assert_eq!(
            check(&mut &b"a\0a\0"[..], 0, true, &compare, &compare).unwrap(),
            Some((2, b"a".to_vec()))
        );
        assert!(check(&mut &b"a\nb"[..], b'\n', false, &compare, &compare)
            .unwrap()
            .is_none());
    }
}
