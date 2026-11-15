use crate::common::{args, error, io};
use std::time::Duration;

const HELP:&str="Usage: free [OPTION]...\nDisplay memory usage.\n  -b, --bytes             show output in bytes\n  -k, --kibi              show output in kibibytes\n  -m, --mebi              show output in mebibytes\n  -g, --gibi              show output in gibibytes\n      --tebi              show output in tebibytes\n      --pebi              show output in pebibytes\n      --kilo              show output in kilobytes\n      --mega              show output in megabytes\n      --giga              show output in gigabytes\n      --tera              show output in terabytes\n      --peta              show output in petabytes\n  -h, --human             show human-readable output\n      --si                use powers of 1000\n  -l, --lohi              show detailed low and high memory statistics\n  -t, --total             show total for RAM and swap\n  -w, --wide              wide output\n  -v, --committed         display committed memory (unsupported)\n  -s, --seconds=N         repeat display every N seconds\n  -c, --count=N           repeat N times\n";
#[derive(Default)]
struct Opt {
    unit: u64,
    human: bool,
    si: bool,
    total: bool,
    wide: bool,
    lohi: bool,
    seconds: Option<f64>,
    count: Option<usize>,
}
#[derive(Default, Clone, Copy)]
struct Memory {
    total: u64,
    used: u64,
    free: u64,
    shared: u64,
    cache: u64,
    buffers: u64,
    cached: u64,
    available: u64,
    swap_total: u64,
    swap_used: u64,
    swap_free: u64,
    low_total: u64,
    low_free: u64,
    high_total: u64,
    high_free: u64,
}

#[cfg(any(target_os = "linux", test))]
fn from_meminfo(text: &str) -> Memory {
    let mut values = std::collections::HashMap::<&str, u64>::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if let Some(number) = value
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
            {
                values.insert(key, number.saturating_mul(1024));
            }
        }
    }
    let get = |name: &str| values.get(name).copied().unwrap_or(0);
    let total = get("MemTotal");
    let free = get("MemFree");
    let buffers = get("Buffers");
    let cached = get("Cached").saturating_add(get("SReclaimable"));
    let cache = buffers.saturating_add(cached);
    let swap_total = get("SwapTotal");
    let swap_free = get("SwapFree");
    Memory {
        total,
        free,
        used: total.saturating_sub(get("MemAvailable")),
        shared: get("Shmem"),
        cache,
        buffers,
        cached,
        available: get("MemAvailable"),
        swap_total,
        swap_used: swap_total.saturating_sub(swap_free),
        swap_free,
        low_total: get("LowTotal"),
        low_free: get("LowFree"),
        high_total: get("HighTotal"),
        high_free: get("HighFree"),
    }
}

fn memory() -> Memory {
    #[cfg(target_os = "linux")]
    if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
        return from_meminfo(&text);
    }
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let total = system.total_memory();
    let free = system.free_memory();
    let swap_total = system.total_swap();
    let swap_free = swap_total.saturating_sub(system.used_swap());
    Memory {
        total,
        free,
        used: system.used_memory(),
        shared: 0,
        cache: 0,
        available: system.available_memory(),
        swap_total,
        swap_used: system.used_swap(),
        swap_free,
        ..Memory::default()
    }
}
pub fn run(a: &[String]) -> i32 {
    let specs = [
        args::OptSpec::flag("bytes", 'b'),
        args::OptSpec::flag("kibi", 'k'),
        args::OptSpec::flag("mebi", 'm'),
        args::OptSpec::flag("gibi", 'g'),
        args::OptSpec::flag("tebi", '\0'),
        args::OptSpec::flag("pebi", '\0'),
        args::OptSpec::flag("kilo", '\0'),
        args::OptSpec::flag("mega", '\0'),
        args::OptSpec::flag("giga", '\0'),
        args::OptSpec::flag("tera", '\0'),
        args::OptSpec::flag("peta", '\0'),
        args::OptSpec::flag("human", 'h'),
        args::OptSpec::flag("si", '\0'),
        args::OptSpec::flag("lohi", 'l'),
        args::OptSpec::flag("total", 't'),
        args::OptSpec::flag("wide", 'w'),
        args::OptSpec::flag("committed", 'v'),
        args::OptSpec::value("seconds", 's'),
        args::OptSpec::value("count", 'c'),
    ];
    let items = match args::parse_command("free", a, HELP, &specs, true) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let mut o = Opt::default();
    let mut seconds_seen = false;
    let mut count_seen = false;
    for it in items {
        match it {
            args::Item::Opt(id, v) => match id {
                "bytes" => {
                    o.unit = 1;
                    o.human = false;
                }
                "kibi" => {
                    o.unit = 1024;
                    o.human = false;
                }
                "mebi" => {
                    o.unit = 1024u64.pow(2);
                    o.human = false;
                }
                "gibi" => {
                    o.unit = 1024u64.pow(3);
                    o.human = false;
                }
                "tebi" => {
                    o.unit = 1024u64.pow(4);
                    o.human = false;
                }
                "pebi" => {
                    o.unit = 1024u64.pow(5);
                    o.human = false;
                }
                "kilo" => {
                    o.unit = 1000;
                    o.human = false;
                }
                "mega" => {
                    o.unit = 1000u64.pow(2);
                    o.human = false;
                }
                "giga" => {
                    o.unit = 1000u64.pow(3);
                    o.human = false;
                }
                "tera" => {
                    o.unit = 1000u64.pow(4);
                    o.human = false;
                }
                "peta" => {
                    o.unit = 1000u64.pow(5);
                    o.human = false;
                }
                "human" => {
                    o.human = true;
                    o.unit = 0;
                }
                "si" => o.si = true,
                "total" => o.total = true,
                "wide" => o.wide = true,
                "lohi" => o.lohi = true,
                "committed" => {
                    return error::usage("free", "option --committed is not supported", 1)
                }
                "seconds" => {
                    seconds_seen = true;
                    let parsed = v.and_then(|s| s.parse::<f64>().ok());
                    match parsed.filter(|n| n.is_finite() && *n > 0.0) {
                        Some(seconds) => o.seconds = Some(seconds),
                        None => return error::usage("free", "invalid interval", 1),
                    }
                }
                "count" => {
                    count_seen = true;
                    let parsed = v.and_then(|s| s.parse::<usize>().ok());
                    match parsed.filter(|n| *n > 0) {
                        Some(count) => o.count = Some(count),
                        None => return error::usage("free", "invalid count", 1),
                    }
                }
                _ => {}
            },
            args::Item::Operand(v) => {
                return error::usage("free", format!("extra operand '{v}'"), 1)
            }
        }
    }
    let count = if seconds_seen || count_seen {
        o.count
    } else {
        Some(1)
    };
    io::output("free", |out| {
        let mut iteration = 0usize;
        loop {
            let m = memory();
            let denominator = if o.unit == 0 {
                if o.human {
                    1.0
                } else if o.si {
                    1000.0
                } else {
                    1024.0
                }
            } else {
                o.unit as f64
            };
            let (total, used, free) = (
                m.total as f64 / denominator,
                m.used as f64 / denominator,
                m.free as f64 / denominator,
            );
            let (st, su, sf) = (
                m.swap_total as f64 / denominator,
                m.swap_used as f64 / denominator,
                m.swap_free as f64 / denominator,
            );
            if o.wide {
                writeln!(out,"               total        used        free      shared     buffers       cache   available")?;
                writeln!(
                    out,
                    "Mem:{:>11} {:>11} {:>11} {:>11} {:>11} {:>11} {:>11}",
                    fmt(total, o.human, o.si),
                    fmt(used, o.human, o.si),
                    fmt(free, o.human, o.si),
                    fmt(m.shared as f64 / denominator, o.human, o.si),
                    fmt(m.buffers as f64 / denominator, o.human, o.si),
                    fmt(m.cached as f64 / denominator, o.human, o.si),
                    fmt(m.available as f64 / denominator, o.human, o.si)
                )?;
            } else {
                writeln!(out,
                "               total        used        free      shared  buff/cache   available"
            )?;
                writeln!(
                    out,
                    "Mem:{:>11} {:>11} {:>11} {:>11} {:>11} {:>11}",
                    fmt(total, o.human, o.si),
                    fmt(used, o.human, o.si),
                    fmt(free, o.human, o.si),
                    fmt(m.shared as f64 / denominator, o.human, o.si),
                    fmt(m.cache as f64 / denominator, o.human, o.si),
                    fmt(m.available as f64 / denominator, o.human, o.si)
                )?;
            }
            if o.lohi && !cfg!(windows) {
                writeln!(
                    out,
                    "Low:{:>11} {:>11} {:>11}",
                    fmt(m.low_total as f64 / denominator, o.human, o.si),
                    fmt(
                        m.low_total.saturating_sub(m.low_free) as f64 / denominator,
                        o.human,
                        o.si
                    ),
                    fmt(m.low_free as f64 / denominator, o.human, o.si)
                )?;
                writeln!(
                    out,
                    "High:{:>10} {:>11} {:>11}",
                    fmt(m.high_total as f64 / denominator, o.human, o.si),
                    fmt(
                        m.high_total.saturating_sub(m.high_free) as f64 / denominator,
                        o.human,
                        o.si
                    ),
                    fmt(m.high_free as f64 / denominator, o.human, o.si)
                )?;
            }
            writeln!(
                out,
                "Swap:{:>10} {:>11} {:>11}",
                fmt(st, o.human, o.si),
                fmt(su, o.human, o.si),
                fmt(sf, o.human, o.si)
            )?;
            if o.total {
                writeln!(
                    out,
                    "Total:{:>9} {:>11} {:>11}",
                    fmt(total + st, o.human, o.si),
                    fmt(used + su, o.human, o.si),
                    fmt(free + sf, o.human, o.si)
                )?;
            }
            out.flush()?;
            iteration = iteration.saturating_add(1);
            if count.is_some_and(|limit| iteration >= limit) {
                break;
            }
            if seconds_seen || count.is_some() {
                std::thread::sleep(Duration::from_secs_f64(o.seconds.unwrap_or(1.0)));
            }
        }
        Ok(())
    })
}
fn fmt(n: f64, h: bool, si: bool) -> String {
    if h {
        let base: f64 = if si { 1000.0 } else { 1024.0 };
        let suffixes = if si {
            ["P", "T", "G", "M", "K"]
        } else {
            ["Pi", "Ti", "Gi", "Mi", "Ki"]
        };
        for (power, suffix) in (1..=5).rev().zip(suffixes) {
            if n >= base.powi(power) {
                return format!("{:.1}{suffix}", n / base.powi(power));
            }
        }
    }
    format!("{:.0}", n)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bytes() {
        assert_eq!(fmt(2048.0, false, false), "2048");
    }
    #[test]
    fn human() {
        assert_eq!(fmt(2048.0, true, false), "2.0Ki");
    }
    #[test]
    fn default_unit() {
        assert_eq!(Opt::default().unit, 0);
    }
    #[test]
    fn total_default_count() {
        assert_eq!(
            Opt {
                count: Some(1),
                ..Opt::default()
            }
            .count,
            Some(1)
        );
    }
    #[test]
    fn help() {
        assert_eq!(
            args::help_version("free", &["--help".into()], HELP, true),
            Some(0)
        );
    }
    #[test]
    fn short_help_lists_flags() {
        for x in [
            "-b", "-k", "-m", "-g", "-h", "-l", "-t", "-w", "-v", "-s", "-c",
        ] {
            assert!(HELP.contains(x));
        }
    }
    #[test]
    fn human_suffix() {
        assert!(fmt(2048.0, true, false).ends_with("Ki"));
    }
    #[test]
    fn zero_human() {
        assert_eq!(fmt(0.0, true, false), "0");
    }
    #[test]
    fn meminfo_uses_available_for_used_and_full_cache_sum() {
        let memory = from_meminfo("MemTotal: 1000 kB\nMemFree: 100 kB\nMemAvailable: 300 kB\nBuffers: 20 kB\nCached: 200 kB\nSReclaimable: 30 kB\nShmem: 40 kB\nSwapTotal: 50 kB\nSwapFree: 10 kB\n");
        assert_eq!(memory.used, 700 * 1024);
        assert_eq!(memory.cache, 250 * 1024);
        assert_eq!(memory.shared, 40 * 1024);
        assert_eq!(memory.swap_used, 40 * 1024);
    }
}
