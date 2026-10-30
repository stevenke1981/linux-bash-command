//! `tar -t` and `tar -tv`: member names and the GNU long listing.

use std::io::{self, Read, Write};
use std::time::{Duration, UNIX_EPOCH};

use tar::{Archive, Entry, EntryType};

use super::archive_io;
use super::extract::fatal_read;
use super::names;
use super::options::{Operand, Options, CMD};
use crate::common::datetime::{format_in, zone_from_env};
use crate::common::error::{io_error_text, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::{finish_output, stdout_writer};
use crate::common::path::display_path;

/// Everything `-tv` shows about one member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub kind: char,
    pub mode: u32,
    pub owner: String,
    pub group: String,
    pub size: u64,
    pub mtime: u64,
    pub name: String,
    /// `" -> target"` for symlinks, `" link to target"` for hard links, else empty.
    pub link_suffix: String,
}

/// The permission string, for example `drwxr-xr-x` or `lrwxrwxrwx`.
pub fn mode_string(kind: char, mode: u32) -> String {
    let mut text = String::with_capacity(10);
    text.push(kind);
    let classes = [
        (0o400, 0o200, 0o100, 0o4000, 's'),
        (0o040, 0o020, 0o010, 0o2000, 's'),
    ];
    for (read, write, exec, special, special_char) in classes {
        text.push(if mode & read != 0 { 'r' } else { '-' });
        text.push(if mode & write != 0 { 'w' } else { '-' });
        let runnable = mode & exec != 0;
        text.push(match (mode & special != 0, runnable) {
            (true, true) => special_char,
            (true, false) => special_char.to_ascii_uppercase(),
            (false, true) => 'x',
            (false, false) => '-',
        });
    }
    text.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    text.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    text.push(match (mode & 0o1000 != 0, mode & 0o001 != 0) {
        (true, true) => 't',
        (true, false) => 'T',
        (false, true) => 'x',
        (false, false) => '-',
    });
    text
}

/// One `-tv` line, without the trailing newline. The owner/group column is padded so the
/// size ends at a fixed column, as GNU tar does.
pub fn format_long(item: &Listing) -> String {
    let owner = format!("{}/{}", item.owner, item.group);
    let width = 19usize.saturating_sub(owner.len());
    let date = format_in(
        "%Y-%m-%d %H:%M",
        crate::common::datetime::from_system_time(UNIX_EPOCH + Duration::from_secs(item.mtime)),
        &zone_from_env(),
    );
    format!(
        "{} {owner}{:>width$} {date} {}{}",
        mode_string(item.kind, item.mode),
        item.size,
        item.name,
        item.link_suffix
    )
}

fn kind_char(kind: EntryType) -> char {
    match kind {
        EntryType::Directory => 'd',
        EntryType::Symlink => 'l',
        EntryType::Char => 'c',
        EntryType::Block => 'b',
        EntryType::Fifo => 'p',
        EntryType::Link => 'h',
        _ => '-',
    }
}

/// Builds the `-tv` record for an entry.
fn describe<R: Read>(entry: &Entry<'_, R>, name: String) -> Listing {
    let header = entry.header();
    let owner = header
        .username()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| header.uid().unwrap_or(0).to_string());
    let group = header
        .groupname()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| header.gid().unwrap_or(0).to_string());
    let kind = kind_char(header.entry_type());
    let link_suffix = match (header.entry_type(), entry.link_name()) {
        (EntryType::Symlink, Ok(Some(target))) => format!(" -> {}", display_path(&target)),
        (EntryType::Link, Ok(Some(target))) => format!(" link to {}", display_path(&target)),
        _ => String::new(),
    };
    let size = match header.entry_type() {
        EntryType::Char | EntryType::Block | EntryType::Directory => 0,
        _ => entry.size(),
    };
    Listing {
        kind,
        mode: header.mode().unwrap_or(0),
        owner,
        group,
        size,
        mtime: header.mtime().unwrap_or(0),
        name,
        link_suffix,
    }
}

/// Runs `-t`. Returns the process exit status.
pub fn run(opts: &Options, operands: &[Operand]) -> i32 {
    let reader = match archive_io::open_source(opts.archive.as_deref()) {
        Ok(reader) => reader,
        Err(error) => {
            let name = opts.archive.as_deref().unwrap_or("-");
            eprintln!("{CMD}: {name}: Cannot open: {}", io_error_text(&error));
            eprintln!("{CMD}: Error is not recoverable: exiting now");
            return EXIT_TROUBLE;
        }
    };
    let mut out = stdout_writer();
    let wanted: Vec<String> = operands.iter().map(|o| o.name.clone()).collect();
    let mut archive = Archive::new(reader);
    let entries = match archive.entries() {
        Ok(entries) => entries,
        Err(error) => return fatal_read(&error, true),
    };
    let mut missing = false;
    let mut found: Vec<bool> = vec![false; wanted.len()];
    for (index, entry) in entries.enumerate() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                let _ = out.flush();
                return fatal_read(&error, index == 0);
            }
        };
        let name_bytes = entry.path_bytes().into_owned();
        if !names::selected(&name_bytes, &wanted) || names::excluded(&name_bytes, &opts.excludes) {
            continue;
        }
        mark_found(&name_bytes, &wanted, &mut found);
        let name = display_path(&names::bytes_to_path(&name_bytes));
        let line = if opts.verbose {
            format_long(&describe(&entry, name))
        } else {
            name
        };
        if let Err(error) = writeln!(out, "{line}") {
            return report_write(&error);
        }
    }
    for (operand, seen) in wanted.iter().zip(found) {
        if !seen {
            eprintln!("{CMD}: {operand}: Not found in archive");
            missing = true;
        }
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    if missing {
        eprintln!("{CMD}: Exiting with failure status due to previous errors");
        return EXIT_TROUBLE;
    }
    EXIT_OK
}

fn report_write(error: &io::Error) -> i32 {
    if crate::common::io::is_broken_pipe(error) {
        return EXIT_OK;
    }
    eprintln!("{CMD}: write error: {}", io_error_text(error));
    EXIT_TROUBLE
}

/// Records which operands matched this member name.
fn mark_found(name: &[u8], wanted: &[String], found: &mut [bool]) {
    for (index, operand) in wanted.iter().enumerate() {
        if names::selected(name, std::slice::from_ref(operand)) {
            found[index] = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_strings_match_ls_conventions() {
        assert_eq!(mode_string('-', 0o644), "-rw-r--r--");
        assert_eq!(mode_string('d', 0o755), "drwxr-xr-x");
        assert_eq!(mode_string('-', 0o4755), "-rwsr-xr-x");
        assert_eq!(mode_string('-', 0o1777), "-rwxrwxrwt");
        assert_eq!(mode_string('l', 0o777), "lrwxrwxrwx");
    }

    #[test]
    fn long_line_pads_size_to_a_fixed_column() {
        let base = Listing {
            kind: '-',
            mode: 0o640,
            owner: "root".into(),
            group: "root".into(),
            size: 6,
            mtime: 1_700_000_000,
            name: "src/a.txt".into(),
            link_suffix: String::new(),
        };
        let line = format_long(&base);
        assert_eq!(
            line,
            "-rw-r----- root/root         6 2023-11-14 22:13 src/a.txt"
        );
        let wide = Listing {
            owner: "al".into(),
            group: "staff".into(),
            size: 0,
            name: "src/".into(),
            kind: 'd',
            mode: 0o755,
            ..base.clone()
        };
        assert_eq!(
            format_long(&wide),
            "drwxr-xr-x al/staff          0 2023-11-14 22:13 src/"
        );
    }

    #[test]
    fn symlink_suffix_follows_the_name() {
        let link = Listing {
            kind: 'l',
            mode: 0o777,
            owner: "0".into(),
            group: "0".into(),
            size: 0,
            mtime: 0,
            name: "link".into(),
            link_suffix: " -> a.txt".into(),
        };
        assert!(format_long(&link).ends_with("link -> a.txt"));
    }

    #[test]
    fn operands_mark_themselves_and_children() {
        let wanted = vec!["src".to_string(), "missing".to_string()];
        let mut found = vec![false, false];
        mark_found(b"src/a.txt", &wanted, &mut found);
        assert_eq!(found, vec![true, false]);
    }
}
