//! `uptime`: how long the system has been running, in the procps layout.

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

const NAME: &str = "uptime";

const HELP: &str = "\
Usage:
 uptime [options]

Options:
 -p, --pretty   show uptime in pretty format
 -h, --help     display this help and exit
 -s, --since    system up since
 -V, --version  output version information and exit

For more details see uptime(1).
";

const OPTIONS: &[OptSpec] = &[
    OptSpec::flag("pretty", Some('p'), Some("pretty")),
    OptSpec::flag("since", Some('s'), Some("since")),
    OptSpec::flag("help", Some('h'), Some("help")),
    OptSpec::flag("version", Some('V'), Some("version")),
];

/// Entry point for `uptime`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(NAME, OPTIONS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(NAME, &error, EXIT_FAIL),
    };
    let mut pretty = false;
    let mut since = false;
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(NAME),
            Item::Opt { id: "pretty", .. } => pretty = true,
            Item::Opt { id: "since", .. } => since = true,
            Item::Opt { .. } => {}
            Item::Operand(extra) => {
                let error = UsageError {
                    message: format!("extra operand '{extra}'"),
                };
                return report_usage(NAME, &error, EXIT_FAIL);
            }
        }
    }
    let up_secs = sysinfo::System::uptime();
    if since {
        let boot = sysinfo::System::boot_time();
        match local_time(boot, "%Y-%m-%d %H:%M:%S") {
            Some(text) => println!("{text}"),
            None => {
                eprintln!("{NAME}: cannot read the boot time");
                return EXIT_FAIL;
            }
        }
    } else if pretty {
        println!("up {}", pretty_span(up_secs));
    } else {
        println!("{}", status_line(up_secs));
    }
    EXIT_OK
}

/// ` HH:MM:SS up 54 min,  0 users,  load average: 1.73, 4.05, 3.26`. The users and load
/// fields are left out where the platform cannot provide them.
fn status_line(up_secs: u64) -> String {
    let now = jiff::Zoned::now().strftime("%H:%M:%S").to_string();
    let mut line = format!(" {now} up {}", short_span(up_secs));
    if let Some(users) = logged_in_users() {
        let noun = if users == 1 { "user" } else { "users" };
        line.push_str(&format!(",  {users} {noun}"));
    }
    if let Some([one, five, fifteen]) = load_average() {
        line.push_str(&format!(
            ",  load average: {one:.2}, {five:.2}, {fifteen:.2}"
        ));
    }
    line
}

/// `D days, H:MM`, `H:MM` or `N min`, as procps prints the uptime column.
fn short_span(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3600;
    let minutes = (secs % 3600) / 60;
    if days > 0 {
        let noun = if days == 1 { "day" } else { "days" };
        format!("{days} {noun}, {hours}:{minutes:02}")
    } else if hours > 0 {
        format!("{hours}:{minutes:02}")
    } else {
        format!("{minutes} min")
    }
}

/// `1 day, 2 hours, 5 minutes` for `-p`.
fn pretty_span(secs: u64) -> String {
    let parts = [
        (secs / 86_400, "day", "days"),
        ((secs % 86_400) / 3600, "hour", "hours"),
        ((secs % 3600) / 60, "minute", "minutes"),
    ];
    let shown: Vec<String> = parts
        .iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, one, many)| format!("{count} {}", if *count == 1 { one } else { many }))
        .collect();
    if shown.is_empty() {
        "0 minutes".to_owned()
    } else {
        shown.join(", ")
    }
}

fn local_time(epoch_secs: u64, format: &str) -> Option<String> {
    let secs = i64::try_from(epoch_secs).ok()?;
    let timestamp = jiff::Timestamp::from_second(secs).ok()?;
    let zoned = timestamp.to_zoned(jiff::tz::TimeZone::system());
    Some(zoned.strftime(format).to_string())
}

#[cfg(unix)]
fn load_average() -> Option<[f64; 3]> {
    let avg = sysinfo::System::load_average();
    Some([avg.one, avg.five, avg.fifteen])
}

#[cfg(not(unix))]
fn load_average() -> Option<[f64; 3]> {
    None
}

/// Counts `USER_PROCESS` records in the utmp database. Only Linux is supported here;
/// other platforms omit the users field.
#[cfg(target_os = "linux")]
fn logged_in_users() -> Option<usize> {
    let mut count = 0usize;
    // SAFETY: setutxent/getutxent/endutxent are used as a sequence: the stream is
    // rewound, read until a null record, then closed. Records are only read through
    // the returned pointer while it is valid.
    unsafe {
        libc::setutxent();
        loop {
            let entry = libc::getutxent();
            if entry.is_null() {
                break;
            }
            if (*entry).ut_type == libc::USER_PROCESS {
                count += 1;
            }
        }
        libc::endutxent();
    }
    Some(count)
}

#[cfg(not(target_os = "linux"))]
fn logged_in_users() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_span_uses_procps_shapes() {
        assert_eq!(short_span(54 * 60), "54 min");
        assert_eq!(short_span(2 * 3600 + 5 * 60), "2:05");
        assert_eq!(short_span(86_400 + 3 * 3600 + 4 * 60), "1 day, 3:04");
        assert_eq!(short_span(3 * 86_400), "3 days, 0:00");
    }

    #[test]
    fn pretty_span_lists_nonzero_parts() {
        assert_eq!(pretty_span(0), "0 minutes");
        assert_eq!(pretty_span(60), "1 minute");
        assert_eq!(
            pretty_span(86_400 + 2 * 3600 + 5 * 60),
            "1 day, 2 hours, 5 minutes"
        );
    }

    #[test]
    fn epoch_formats_in_utc_when_tz_is_utc() {
        // Independent of the host zone: the year and month of a fixed instant.
        let text = local_time(0, "%Y").expect("time");
        assert!(text == "1970" || text == "1969");
    }

    #[test]
    fn status_line_has_time_and_uptime() {
        let line = status_line(54 * 60);
        assert!(line.starts_with(' '));
        assert!(line.contains(" up 54 min"));
    }
}
