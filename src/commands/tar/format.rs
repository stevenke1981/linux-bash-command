use std::{
    io::{self, Write},
    path::Path,
};
fn mode_string(mode: u32, kind: tar::EntryType) -> String {
    let first = if kind.is_dir() {
        'd'
    } else if kind.is_symlink() {
        'l'
    } else if kind.is_hard_link() {
        'h'
    } else {
        '-'
    };
    let mut text = String::from(first);
    for (shift, special) in [(6, 0o4000), (3, 0o2000), (0, 0o1000)] {
        text.push(if mode & (4 << shift) != 0 { 'r' } else { '-' });
        text.push(if mode & (2 << shift) != 0 { 'w' } else { '-' });
        let execute = mode & (1 << shift) != 0;
        text.push(match (mode & special != 0, execute, shift == 0) {
            (true, true, true) => 't',
            (true, false, true) => 'T',
            (true, true, false) => 's',
            (true, false, false) => 'S',
            (false, true, _) => 'x',
            (false, false, _) => '-',
        });
    }
    text
}
fn date(seconds: u64) -> String {
    i64::try_from(seconds)
        .ok()
        .and_then(|secs| jiff::Timestamp::from_second(secs).ok())
        .and_then(|t| {
            crate::common::datetime::format(&t.to_zoned(jiff::tz::TimeZone::UTC), "%Y-%m-%d %H:%M")
                .ok()
        })
        .unwrap_or_else(|| "1970-01-01 00:00".into())
}
fn owner(header: &tar::Header) -> String {
    if cfg!(windows) {
        return "0/0".into();
    }
    let uid = header.uid().unwrap_or(0);
    let gid = header.gid().unwrap_or(0);
    let user = header
        .username()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| uid.to_string());
    let group = header
        .groupname()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| gid.to_string());
    format!("{user}/{group}")
}
pub(super) fn write_name(out: &mut dyn Write, name: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        out.write_all(name.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        out.write_all(crate::common::path::display_path(name).as_bytes())
    }
}
pub fn list(
    out: &mut dyn Write,
    entry: &tar::Entry<'_, impl io::Read>,
    verbose: bool,
) -> io::Result<()> {
    let header = entry.header();
    if verbose {
        let owner = owner(header);
        let width = 18usize.saturating_sub(owner.len()).max(1);
        write!(
            out,
            "{} {} {:width$} {} ",
            mode_string(header.mode().unwrap_or(0), header.entry_type()),
            owner,
            header.size().unwrap_or(0),
            date(header.mtime().unwrap_or(0))
        )?;
    }
    write_name(out, &entry.path()?)?;
    if verbose && (header.entry_type().is_symlink() || header.entry_type().is_hard_link()) {
        if let Some(target) = entry.link_name()? {
            write!(
                out,
                " {} ",
                if header.entry_type().is_symlink() {
                    "->"
                } else {
                    "link to"
                }
            )?;
            write_name(out, &target)?;
        }
    }
    writeln!(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes() {
        assert_eq!(mode_string(0o644, tar::EntryType::Regular), "-rw-r--r--");
        assert_eq!(mode_string(0o755, tar::EntryType::Directory), "drwxr-xr-x");
    }
    #[test]
    fn epoch() {
        assert_eq!(date(0), "1970-01-01 00:00");
    }
    #[test]
    fn missing_owner() {
        let h = tar::Header::new_gnu();
        assert_eq!(owner(&h), "0/0");
    }
}
