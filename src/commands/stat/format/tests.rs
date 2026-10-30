use super::*;
use crate::commands::stat::platform::{S_IFDIR, S_IFLNK, S_IFREG};

fn info() -> FileInfo {
    FileInfo {
        name: "a.txt".to_string(),
        link: None,
        stat: RawStat {
            dev: 254 << 8,
            ino: 680_061,
            mode: S_IFREG | 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            size: 6,
            blocks: 8,
            blksize: 4096,
            atime: (1_700_000_000, 0),
            mtime: (1_700_000_000, 0),
            ctime: (1_793_357_911, 235_737_267),
            btime: Some((1_793_357_911, 231_045_444)),
        },
        user: "root".to_string(),
        group: "root".to_string(),
        mount: "/".to_string(),
    }
}

fn utc() -> TimeZone {
    TimeZone::UTC
}

#[test]
fn simple_numeric_and_name_sequences() {
    let text = expand("%a|%A|%b|%B|%d|%D|%f|%F|%h|%i|%n|%o|%s", &info(), &utc());
    assert_eq!(
        text,
        "644|-rw-r--r--|8|512|65024|fe00|81a4|regular file|1|680061|a.txt|4096|6"
    );
}

#[test]
fn user_group_and_mount() {
    assert_eq!(expand("%u:%g:%U:%G:%m", &info(), &utc()), "0:0:root:root:/");
}

#[test]
fn times_epoch_and_human() {
    let text = expand("%X %Y %Z %W", &info(), &utc());
    assert_eq!(text, "1700000000 1700000000 1793357911 1793357911");
    assert_eq!(
        expand("%x", &info(), &utc()),
        "2023-11-14 22:13:20.000000000 +0000"
    );
    assert_eq!(
        expand("%z", &info(), &utc()),
        "2026-10-30 10:58:31.235737267 +0000"
    );
}

#[test]
fn birth_unknown_prints_dash_and_zero() {
    let mut no_birth = info();
    no_birth.stat.btime = None;
    assert_eq!(expand("%w|%W", &no_birth, &utc()), "-|0");
}

#[test]
fn percent_and_escape_free_text() {
    assert_eq!(expand("100%%", &info(), &utc()), "100%");
    assert_eq!(expand("[%n]", &info(), &utc()), "[a.txt]");
    assert_eq!(expand("%q", &info(), &utc()), "?");
    assert_eq!(expand("%C", &info(), &utc()), "?");
    assert_eq!(expand("tail %", &info(), &utc()), "tail %");
}

#[test]
fn width_and_flags() {
    assert_eq!(expand("%-10s|", &info(), &utc()), "6         |");
    assert_eq!(expand("%5s|", &info(), &utc()), "    6|");
    assert_eq!(expand("%010s", &info(), &utc()), "0000000006");
    assert_eq!(expand("%-8U|", &info(), &utc()), "root    |");
    assert_eq!(expand("%8U|", &info(), &utc()), "    root|");
}

#[test]
fn zero_fill_goes_after_sign() {
    assert_eq!(
        pad(
            "-5",
            &Flags {
                left: false,
                zero: true,
                width: 4
            }
        ),
        "-005"
    );
    assert_eq!(
        pad(
            "ab",
            &Flags {
                left: true,
                zero: true,
                width: 4
            }
        ),
        "ab  "
    );
    assert_eq!(
        pad(
            "ab",
            &Flags {
                left: false,
                zero: false,
                width: 4
            }
        ),
        "  ab"
    );
}

#[test]
fn device_fields_in_decimal_and_hex() {
    let mut dev = info();
    // `%Hd,%Ld` is the file's own device (254,0 here); `%Hr,%Lr` and `%t %T` are the
    // special-file device number, which is 8,1 in hex and 8,1 in decimal for this value.
    dev.stat.rdev = (8 << 8) | 1;
    assert_eq!(expand("%Hd,%Ld", &dev, &utc()), "254,0");
    assert_eq!(expand("%Hr,%Lr", &dev, &utc()), "8,1");
    assert_eq!(expand("%t %T", &dev, &utc()), "8 1");
}

#[test]
fn single_quote_escapes_embedded_quotes() {
    assert_eq!(single_quote("a.txt"), "'a.txt'");
    assert_eq!(single_quote("it's"), "'it'\\''s'");
}

#[test]
fn quoted_name_with_link_target() {
    let mut link = info();
    link.name = "link".to_string();
    link.link = Some("a.txt".to_string());
    assert_eq!(expand("%N", &link, &utc()), "'link' -> 'a.txt'");
    assert_eq!(quoted_name(&info()), "'a.txt'");
}

#[test]
fn default_layout_matches_gnu_shape() {
    let text = default_layout(&info(), &utc());
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "  File: a.txt");
    assert_eq!(
        lines[1],
        "  Size: 6         \tBlocks: 8          IO Block: 4096   regular file"
    );
    assert_eq!(lines[2], "Device: 254,0\tInode: 680061      Links: 1");
    assert_eq!(
        lines[3],
        "Access: (0644/-rw-r--r--)  Uid: (    0/    root)   Gid: (    0/    root)"
    );
    assert_eq!(lines[4], "Access: 2023-11-14 22:13:20.000000000 +0000");
    assert_eq!(lines[7], " Birth: 2026-10-30 10:58:31.231045444 +0000");
}

#[test]
fn default_layout_shows_symlink_target() {
    let mut link = info();
    link.name = "link".to_string();
    link.link = Some("a.txt".to_string());
    link.stat.mode = S_IFLNK | 0o777;
    let text = default_layout(&link, &utc());
    assert!(text.starts_with("  File: link -> a.txt\n"));
    assert!(text.contains("symbolic link"));
}

#[test]
fn terse_layout_fields() {
    assert_eq!(
        terse_layout(&info()),
        "a.txt 6 8 81a4 0 0 fe00 680061 1 0 0 1700000000 1700000000 1793357911 1793357911 4096\n"
    );
}

#[test]
fn directory_kind_and_mode_letters() {
    let mut dir = info();
    dir.stat.mode = S_IFDIR | 0o755;
    assert_eq!(expand("%F|%A|%a", &dir, &utc()), "directory|drwxr-xr-x|755");
}
