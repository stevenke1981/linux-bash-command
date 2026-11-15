//! Host and uptime utilities backed by the operating system and `sysinfo`.

use crate::common::{args, error, io};
use std::time::Duration;

const NPROC_HELP: &str = "Usage: nproc [OPTION]...\nPrint the number of processing units available to the current process.\n\
\n      --all        print the number of installed processors\n      --ignore=N   if possible, exclude N processing units\n      --help       display this help and exit\n      --version    output version information and exit\n";
const HOSTNAME_HELP: &str = "Usage: hostname [OPTION]... [HOSTNAME]\nShow or set the system's host name.\n\
\n  -s, --short   display the short host name\n  -f, --fqdn    display the fully qualified domain name\n      --help    display this help and exit\n      --version output version information and exit\n";
const ARCH_HELP: &str = "Usage: arch [OPTION]...\nPrint machine hardware name.\n\
\n      --help    display this help and exit\n      --version output version information and exit\n";
const UPTIME_HELP: &str = "Usage: uptime [OPTION]...\nTell how long the system has been running.\n\
\n  -p, --pretty  show uptime in a pretty format\n  -s, --since   system up since\n      --help    display this help and exit\n      --version output version information and exit\n";

pub fn run_nproc(args_in: &[String]) -> i32 {
    let specs = [
        args::OptSpec::flag("all", '\0'),
        args::OptSpec::value("ignore", '\0'),
    ];
    let items = match args::parse_command("nproc", args_in, NPROC_HELP, &specs, true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let mut all = false;
    let mut ignore = 0usize;
    for item in items {
        match item {
            args::Item::Opt("all", _) => all = true,
            args::Item::Opt("ignore", Some(value)) => match parse_nonnegative(&value) {
                Some(value) => ignore = value,
                None => return error::usage("nproc", format!("invalid number: '{value}'"), 1),
            },
            args::Item::Opt(_, _) => unreachable!(),
            args::Item::Operand(value) => {
                return error::usage("nproc", format!("unexpected operand '{value}'"), 1)
            }
        }
    }
    let mut count = if all {
        let system = sysinfo::System::new_all();
        system.cpus().len().max(1)
    } else {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    };
    count = apply_omp_limits(
        count,
        all,
        std::env::var("OMP_NUM_THREADS").ok().as_deref(),
        std::env::var("OMP_THREAD_LIMIT").ok().as_deref(),
    );
    count = count.saturating_sub(ignore).max(1);
    write_line("nproc", &count.to_string())
}

fn parse_nonnegative(value: &str) -> Option<usize> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn omp_limit(value: &str) -> Option<usize> {
    let first = value.split(',').next()?.trim();
    let parsed = parse_nonnegative(first)?;
    (parsed > 0).then_some(parsed)
}

fn apply_omp_limits(
    mut count: usize,
    all: bool,
    omp_num_threads: Option<&str>,
    omp_thread_limit: Option<&str>,
) -> usize {
    if all {
        return count.max(1);
    }
    if let Some(limit) = omp_num_threads.and_then(omp_limit) {
        count = limit;
    }
    if let Some(limit) = omp_thread_limit.and_then(omp_limit) {
        count = count.min(limit);
    }
    count.max(1)
}

pub fn run_hostname(args_in: &[String]) -> i32 {
    let specs = [
        args::OptSpec::flag("short", 's'),
        args::OptSpec::flag("fqdn", 'f'),
    ];
    let items = match args::parse_command("hostname", args_in, HOSTNAME_HELP, &specs, true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let mut short = false;
    let mut fqdn = false;
    let mut operands = Vec::new();
    for item in items {
        match item {
            args::Item::Opt("short", _) => short = true,
            args::Item::Opt("fqdn", _) => fqdn = true,
            args::Item::Operand(value) => operands.push(value),
            args::Item::Opt(_, _) => unreachable!(),
        }
    }
    if !operands.is_empty() {
        return error::usage("hostname", "setting the host name is not supported", 1);
    }
    if short && fqdn {
        return error::usage(
            "hostname",
            "options --short and --fqdn are mutually exclusive",
            1,
        );
    }
    let Some(name) = sysinfo::System::host_name() else {
        eprintln!("hostname: cannot determine host name");
        return 1;
    };
    let shown = hostname_choice(&name, short, fqdn);
    write_line("hostname", shown)
}

pub fn run_arch(args_in: &[String]) -> i32 {
    let items = match args::parse_command("arch", args_in, ARCH_HELP, &[], true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    if let Some(args::Item::Operand(value)) = items.first() {
        return error::usage("arch", format!("unexpected operand '{value}'"), 1);
    }
    write_line("arch", arch_name(std::env::consts::ARCH))
}

pub fn run_uptime(args_in: &[String]) -> i32 {
    let specs = [
        args::OptSpec::flag("pretty", 'p'),
        args::OptSpec::flag("since", 's'),
    ];
    let items = match args::parse_command("uptime", args_in, UPTIME_HELP, &specs, true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let mut pretty = false;
    let mut since = false;
    for item in items {
        match item {
            args::Item::Opt("pretty", _) => pretty = true,
            args::Item::Opt("since", _) => since = true,
            args::Item::Operand(value) => {
                return error::usage("uptime", format!("extra operand '{value}'"), 1)
            }
            args::Item::Opt(_, _) => unreachable!(),
        }
    }
    if pretty && since {
        return error::usage(
            "uptime",
            "options --pretty and --since are mutually exclusive",
            1,
        );
    }
    let seconds = sysinfo::System::uptime();
    if since {
        return match format_since(seconds) {
            Ok(text) => write_line("uptime", &text),
            Err(error) => {
                eprintln!("uptime: cannot format system start time: {error}");
                1
            }
        };
    }
    let duration = Duration::from_secs(seconds);
    let text = if pretty {
        pretty_uptime(duration)
    } else {
        match default_uptime(seconds) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("uptime: cannot format current time: {error}");
                return 1;
            }
        }
    };
    write_line("uptime", &text)
}

fn pretty_uptime(duration: Duration) -> String {
    let total = duration.as_secs();
    let weeks = total / 604_800;
    let days = total / 86_400;
    let hours = (total % 86_400) / 3_600;
    let minutes = (total % 3_600) / 60;
    let mut parts = Vec::new();
    for (value, unit) in [
        (weeks, "week"),
        (days % 7, "day"),
        (hours, "hour"),
        (minutes, "minute"),
    ] {
        if value > 0 {
            parts.push(plural(value, unit));
        }
    }
    if parts.is_empty() {
        parts.push(plural(0, "minute"));
    }
    format!("up {}", parts.join(", "))
}

fn plural(value: u64, unit: &str) -> String {
    format!("{value} {unit}{}", if value == 1 { "" } else { "s" })
}

fn default_uptime(seconds: u64) -> Result<String, String> {
    let now = crate::common::datetime::from_system_time(std::time::SystemTime::now())?;
    let time = crate::common::datetime::format(&now, "%H:%M:%S")?;
    let mut text = format!(" {time} up {}", uptime_span(seconds));
    if let Some(users) = logged_in_users() {
        text.push_str(&format!(
            ",  {users} user{}",
            if users == 1 { "" } else { "s" }
        ));
    }
    if let Some(load) = load_average() {
        text.push_str(&format!(",  {}", format_load_average(load)));
    }
    Ok(text)
}

fn uptime_span(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days} days,  {hours:02}:{minutes:02}")
    } else {
        format!(" {hours}:{minutes:02}")
    }
}

fn load_average() -> Option<[f64; 3]> {
    #[cfg(windows)]
    {
        None
    }
    #[cfg(not(windows))]
    {
        let avg = sysinfo::System::load_average();
        (sysinfo::IS_SUPPORTED_SYSTEM
            && [avg.one, avg.five, avg.fifteen]
                .iter()
                .all(|v| v.is_finite()))
        .then_some([avg.one, avg.five, avg.fifteen])
    }
}

#[cfg(target_os = "linux")]
fn logged_in_users() -> Option<usize> {
    use libc::{endutxent, getutxent, setutxent, USER_PROCESS};
    if !std::path::Path::new("/run/utmp").exists()
        && !std::path::Path::new("/var/run/utmp").exists()
    {
        return None;
    }
    let mut users = 0;
    unsafe {
        setutxent();
        loop {
            let entry = getutxent();
            if entry.is_null() {
                break;
            }
            if (*entry).ut_type == USER_PROCESS {
                users += 1;
            }
        }
        endutxent();
    }
    Some(users)
}

#[cfg(not(target_os = "linux"))]
fn logged_in_users() -> Option<usize> {
    None
}

fn format_since(uptime_seconds: u64) -> Result<String, String> {
    let boot = std::time::SystemTime::now()
        .checked_sub(Duration::from_secs(uptime_seconds))
        .unwrap_or(std::time::UNIX_EPOCH);
    let local = crate::common::datetime::from_system_time(boot)?;
    crate::common::datetime::format(&local, "%Y-%m-%d %H:%M:%S")
}

fn write_line(command: &str, text: &str) -> i32 {
    io::output(command, |out| writeln!(out, "{text}"))
}

fn arch_name(target: &str) -> &str {
    match target {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "x86" => "i686",
        "arm" => "arm",
        other => other,
    }
}

fn short_hostname(name: &str) -> &str {
    name.split('.')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
}

fn hostname_choice(name: &str, short: bool, fqdn: bool) -> &str {
    if short || (fqdn && !name.contains('.')) {
        short_hostname(name)
    } else {
        name
    }
}

fn format_load_average(load: [f64; 3]) -> String {
    format!(
        "load average: {:.2}, {:.2}, {:.2}",
        load[0], load[1], load[2]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nonnegative_integer_and_rejects_signs() {
        assert_eq!(parse_nonnegative("0"), Some(0));
        assert_eq!(parse_nonnegative("17"), Some(17));
        assert_eq!(parse_nonnegative("-1"), None);
        assert_eq!(parse_nonnegative("1.2"), None);
        assert_eq!(parse_nonnegative(""), None);
        assert_eq!(parse_nonnegative("184467440737095516160000"), None);
    }

    #[test]
    fn omp_num_threads_uses_first_comma_separated_value() {
        assert_eq!(omp_limit("8,4"), Some(8));
        assert_eq!(omp_limit(" 3 , 2"), Some(3));
        assert_eq!(omp_limit("0"), None);
    }

    #[test]
    fn omp_thread_limit_is_a_positive_ceiling() {
        assert_eq!(omp_limit("2"), Some(2));
        assert_eq!(omp_limit("invalid"), None);
    }

    #[test]
    fn nproc_caps_available_units_by_omp_variables() {
        assert_eq!(apply_omp_limits(16, false, Some("8,4"), Some("6")), 6);
        assert_eq!(apply_omp_limits(16, false, Some("8,4"), None), 8);
        assert_eq!(apply_omp_limits(16, false, Some("9999"), None), 9999);
    }

    #[test]
    fn nproc_all_ignores_both_omp_variables() {
        assert_eq!(apply_omp_limits(16, true, Some("3,2"), Some("1")), 16);
        assert_eq!(apply_omp_limits(16, true, None, Some("1")), 16);
    }

    #[test]
    fn nproc_never_returns_zero_after_limits() {
        assert_eq!(apply_omp_limits(0, false, Some("0"), Some("0")), 1);
    }

    #[test]
    fn pretty_uptime_formats_weeks_days_hours_and_minutes() {
        assert_eq!(pretty_uptime(Duration::from_secs(60)), "up 1 minute");
        assert_eq!(pretty_uptime(Duration::from_secs(3_600)), "up 1 hour");
        assert_eq!(pretty_uptime(Duration::from_secs(86_400)), "up 1 day");
        assert_eq!(pretty_uptime(Duration::from_secs(604_800)), "up 1 week");
        assert_eq!(
            pretty_uptime(Duration::from_secs(604_800 + 86_400 + 3_600 + 60)),
            "up 1 week, 1 day, 1 hour, 1 minute"
        );
    }

    #[test]
    fn plural_uses_singular_only_for_one() {
        assert_eq!(plural(1, "day"), "1 day");
        assert_eq!(plural(0, "day"), "0 days");
        assert_eq!(plural(2, "day"), "2 days");
    }

    #[test]
    fn architecture_is_mapped_to_uname_spellings() {
        assert_eq!(arch_name("x86"), "i686");
        assert_eq!(arch_name("aarch64"), "aarch64");
        assert_eq!(arch_name("arm"), "arm");
        assert_eq!(arch_name("x86_64"), "x86_64");
        assert_eq!(arch_name("riscv64"), "riscv64");
    }

    #[test]
    fn architecture_x86_64_name() {
        assert_eq!(arch_name("x86_64"), "x86_64");
    }

    #[test]
    fn architecture_arm64_name() {
        assert_eq!(arch_name("aarch64"), "aarch64");
    }

    #[test]
    fn architecture_32_bit_x86_uses_i686() {
        assert_eq!(arch_name("x86"), "i686");
    }

    #[test]
    fn architecture_arm_name() {
        assert_eq!(arch_name("arm"), "arm");
    }

    #[test]
    fn architecture_other_targets_keep_target_name() {
        assert_eq!(arch_name("riscv64"), "riscv64");
    }

    #[test]
    fn short_hostname_uses_first_label() {
        assert_eq!(short_hostname("node.example.org"), "node");
    }

    #[test]
    fn short_hostname_preserves_single_label() {
        assert_eq!(short_hostname("node"), "node");
    }

    #[test]
    fn hostname_without_flags_keeps_full_name() {
        assert_eq!(
            hostname_choice("node.example.org", false, false),
            "node.example.org"
        );
    }

    #[test]
    fn hostname_short_flag_selects_first_label() {
        assert_eq!(hostname_choice("node.example.org", true, false), "node");
    }

    #[test]
    fn hostname_fqdn_falls_back_to_short_when_unqualified() {
        assert_eq!(hostname_choice("node", false, true), "node");
    }

    #[test]
    fn hostname_fqdn_keeps_qualified_name() {
        assert_eq!(
            hostname_choice("node.example.org", false, true),
            "node.example.org"
        );
    }

    #[test]
    fn uptime_span_uses_procps_day_and_clock_forms() {
        assert_eq!(uptime_span(0), " 0:00");
        assert_eq!(uptime_span(65), " 0:01");
        assert_eq!(uptime_span(3_661), " 1:01");
        assert_eq!(uptime_span(86_400), "1 days,  00:00");
        assert_eq!(uptime_span(2 * 86_400 + 3_661), "2 days,  01:01");
    }

    #[test]
    fn uptime_pretty_truncates_subminute_seconds() {
        assert_eq!(pretty_uptime(Duration::from_secs(0)), "up 0 minutes");
        assert_eq!(pretty_uptime(Duration::from_secs(59)), "up 0 minutes");
        assert_eq!(pretty_uptime(Duration::from_secs(119)), "up 1 minute");
    }

    #[test]
    fn uptime_load_average_values_keep_order_and_precision() {
        assert_eq!(
            format_load_average([1.0, 0.25, 2.5]),
            "load average: 1.00, 0.25, 2.50"
        );
    }
}
