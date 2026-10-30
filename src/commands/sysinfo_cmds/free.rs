//! `free`: memory and swap usage in the procps-ng 4 layout.
//!
//! Used memory is `total - available`, as procps reports it. On Linux the shared and
//! buff/cache figures come from `/proc/meminfo`; elsewhere they are reported as zero.

use std::io::Write;
use std::time::Duration;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::io::{is_broken_pipe, stdout_writer};

const NAME: &str = "free";

const HELP: &str = "\
Usage:
 free [options]

Options:
 -b, --bytes         show output in bytes
     --kilo          show output in kilobytes
     --mega          show output in megabytes
     --giga          show output in gigabytes
     --tera          show output in terabytes
     --peta          show output in petabytes
 -k, --kibi          show output in kibibytes
 -m, --mebi          show output in mebibytes
 -g, --gibi          show output in gibibytes
     --tebi          show output in tebibytes
     --pebi          show output in pebibytes
 -h, --human         show human-readable output
     --si            use powers of 1000 not 1024
 -l, --lohi          show detailed low and high memory statistics
 -t, --total         show total for RAM + swap
 -s N, --seconds N   repeat printing every N seconds
 -c N, --count N     repeat printing N times, then exit
 -w, --wide          wide output

     --help     display this help and exit
 -V, --version  output version information and exit

For more details see free(1).
";

const OPTIONS: &[OptSpec] = &[
    OptSpec::flag("bytes", Some('b'), Some("bytes")),
    OptSpec::flag("kilo", None, Some("kilo")),
    OptSpec::flag("mega", None, Some("mega")),
    OptSpec::flag("giga", None, Some("giga")),
    OptSpec::flag("tera", None, Some("tera")),
    OptSpec::flag("peta", None, Some("peta")),
    OptSpec::flag("kibi", Some('k'), Some("kibi")),
    OptSpec::flag("mebi", Some('m'), Some("mebi")),
    OptSpec::flag("gibi", Some('g'), Some("gibi")),
    OptSpec::flag("tebi", None, Some("tebi")),
    OptSpec::flag("pebi", None, Some("pebi")),
    OptSpec::flag("human", Some('h'), Some("human")),
    OptSpec::flag("si", None, Some("si")),
    OptSpec::flag("lohi", Some('l'), Some("lohi")),
    OptSpec::flag("total", Some('t'), Some("total")),
    OptSpec::req("seconds", Some('s'), Some("seconds")),
    OptSpec::req("count", Some('c'), Some("count")),
    OptSpec::flag("wide", Some('w'), Some("wide")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", Some('V'), Some("version")),
    // Recognised so they report "not supported" rather than "invalid option".
    OptSpec::flag("committed", Some('v'), Some("committed")),
    OptSpec::flag("line", Some('L'), Some("line")),
];

/// How values are scaled and labelled.
#[derive(Clone, Copy)]
enum Scale {
    /// Divide by this many bytes.
    Fixed(u64),
    /// Human-readable, with powers of 1024 or 1000.
    Human { si: bool },
}

/// The figures for one reporting pass, in bytes.
#[derive(Clone, Copy, Default)]
struct Memory {
    total: u64,
    used: u64,
    free: u64,
    shared: u64,
    buffers: u64,
    cache: u64,
    available: u64,
    swap_total: u64,
    swap_used: u64,
    swap_free: u64,
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(NAME, OPTIONS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(NAME, &error, EXIT_FAIL),
    };
    let mut scale = Scale::Fixed(1024);
    let mut si = false;
    let mut human = false;
    let mut lohi = false;
    let mut total_row = false;
    let mut wide = false;
    let mut seconds: Option<f64> = None;
    let mut count: Option<u64> = None;
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(NAME),
            Item::Opt {
                id: "committed", ..
            } => return unsupported(NAME, "-v"),
            Item::Opt { id: "line", .. } => return unsupported(NAME, "-L"),
            Item::Opt { id: "si", .. } => si = true,
            Item::Opt { id: "human", .. } => human = true,
            Item::Opt { id: "lohi", .. } => lohi = true,
            Item::Opt { id: "total", .. } => total_row = true,
            Item::Opt { id: "wide", .. } => wide = true,
            Item::Opt {
                id: "seconds",
                value,
            } => {
                let text = value.unwrap_or_default();
                match text.parse::<f64>() {
                    Ok(secs) if secs.is_finite() && secs > 0.0 => seconds = Some(secs),
                    _ => {
                        eprintln!("{NAME}: failed to parse seconds argument: '{text}'");
                        return EXIT_FAIL;
                    }
                }
            }
            Item::Opt { id: "count", value } => {
                let text = value.unwrap_or_default();
                match text.parse::<u64>() {
                    Ok(0) => {
                        eprintln!(
                            "{NAME}: failed to parse count argument: '{text}': Numerical result out of range"
                        );
                        return EXIT_FAIL;
                    }
                    Ok(n) => count = Some(n),
                    Err(_) => {
                        eprintln!(
                            "{NAME}: failed to parse count argument: '{text}': Invalid argument"
                        );
                        return EXIT_FAIL;
                    }
                }
            }
            Item::Opt { id, .. } => {
                if let Some(divisor) = unit_divisor(id) {
                    scale = Scale::Fixed(divisor);
                }
            }
            Item::Operand(extra) => {
                let error = UsageError {
                    message: format!("extra operand '{extra}'"),
                };
                return report_usage(NAME, &error, EXIT_FAIL);
            }
        }
    }
    if human {
        scale = Scale::Human { si };
    }
    let layout = Layout {
        scale,
        lohi,
        total_row,
        wide,
    };
    let mut out = stdout_writer();
    let mut remaining = count;
    let interval = match (seconds, count) {
        (Some(secs), _) => Duration::from_secs_f64(secs),
        (None, Some(_)) => Duration::from_secs(1),
        (None, None) => Duration::ZERO,
    };
    let repeat = seconds.is_some() || count.is_some();
    let mut first = true;
    loop {
        if !first && out.write_all(b"\n").is_err() {
            return EXIT_OK;
        }
        first = false;
        let memory = read_memory();
        let text = render(&layout, &memory);
        if let Err(error) = out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
            if is_broken_pipe(&error) {
                return EXIT_OK;
            }
            eprintln!(
                "{NAME}: write error: {}",
                crate::common::error::io_error_text(&error)
            );
            return EXIT_FAIL;
        }
        if !repeat {
            break;
        }
        if let Some(left) = remaining.as_mut() {
            *left -= 1;
            if *left == 0 {
                break;
            }
        }
        std::thread::sleep(interval);
    }
    crate::common::io::finish_output(NAME, &mut out)
}

/// The divisor for a unit flag, or `None` when the id is not a unit option.
fn unit_divisor(id: &str) -> Option<u64> {
    const K: u64 = 1024;
    Some(match id {
        "bytes" => 1,
        "kilo" => 1000,
        "mega" => 1000u64.pow(2),
        "giga" => 1000u64.pow(3),
        "tera" => 1000u64.pow(4),
        "peta" => 1000u64.pow(5),
        "kibi" => K,
        "mebi" => K.pow(2),
        "gibi" => K.pow(3),
        "tebi" => K.pow(4),
        "pebi" => K.pow(5),
        _ => return None,
    })
}

struct Layout {
    scale: Scale,
    lohi: bool,
    total_row: bool,
    wide: bool,
}

/// Renders the header and rows. Columns: a 6-wide label, a 14-wide first column and
/// 12-wide others, matching procps.
fn render(layout: &Layout, memory: &Memory) -> String {
    let mut out = String::new();
    let headers: Vec<&str> = if layout.wide {
        vec![
            "total",
            "used",
            "free",
            "shared",
            "buffers",
            "cache",
            "available",
        ]
    } else {
        vec!["total", "used", "free", "shared", "buff/cache", "available"]
    };
    let header_cells: Vec<String> = headers.iter().map(|h| (*h).to_owned()).collect();
    out.push_str(&row("", &header_cells));
    let mem_cells = if layout.wide {
        vec![
            memory.total,
            memory.used,
            memory.free,
            memory.shared,
            memory.buffers,
            memory.cache,
            memory.available,
        ]
    } else {
        vec![
            memory.total,
            memory.used,
            memory.free,
            memory.shared,
            memory.buffers + memory.cache,
            memory.available,
        ]
    };
    out.push_str(&row("Mem:", &cells(&mem_cells, layout.scale)));
    if layout.lohi {
        let low = [
            memory.total,
            memory.total.saturating_sub(memory.free),
            memory.free,
        ];
        out.push_str(&row("Low:", &cells(&low, layout.scale)));
        out.push_str(&row("High:", &cells(&[0, 0, 0], layout.scale)));
    }
    let swap = [memory.swap_total, memory.swap_used, memory.swap_free];
    out.push_str(&row("Swap:", &cells(&swap, layout.scale)));
    if layout.total_row {
        let total = [
            memory.total + memory.swap_total,
            memory.used + memory.swap_used,
            memory.free + memory.swap_free,
        ];
        out.push_str(&row("Total:", &cells(&total, layout.scale)));
    }
    out
}

/// One line: the label padded to 6, then cells. The first cell is 14 wide and the rest
/// 12.
fn row(label: &str, cells: &[String]) -> String {
    let mut line = format!("{label:<6}");
    for (index, cell) in cells.iter().enumerate() {
        let width = if index == 0 { 14 } else { 12 };
        line.push_str(&format!("{cell:>width$}"));
    }
    line.push('\n');
    line
}

fn cells(values: &[u64], scale: Scale) -> Vec<String> {
    values
        .iter()
        .map(|&bytes| match scale {
            Scale::Fixed(divisor) => (bytes / divisor).to_string(),
            Scale::Human { si } => human(bytes, si),
        })
        .collect()
}

/// `15Gi`, `969Mi`, `7.7Gi`, `0B`: one decimal below 10, whole units above, truncated.
fn human(bytes: u64, si: bool) -> String {
    let base: f64 = if si { 1000.0 } else { 1024.0 };
    let units: [&str; 6] = if si {
        ["B", "k", "M", "G", "T", "P"]
    } else {
        ["B", "Ki", "Mi", "Gi", "Ti", "Pi"]
    };
    if bytes < base as u64 {
        return format!("{bytes}B");
    }
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= base && index < units.len() - 1 {
        value /= base;
        index += 1;
    }
    if value < 10.0 {
        format!("{}{}", truncate(value, 10.0), units[index])
    } else {
        format!("{}{}", value.trunc() as u64, units[index])
    }
}

fn truncate(value: f64, scale: f64) -> String {
    let floored = (value * scale).floor() / scale;
    format!("{floored:.1}")
}

/// Reads totals from sysinfo and, on Linux, shared and cache figures from `/proc/meminfo`.
fn read_memory() -> Memory {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let total = system.total_memory();
    let available = system.available_memory();
    let free = system.free_memory();
    let mut memory = Memory {
        total,
        used: total.saturating_sub(available),
        free,
        available,
        swap_total: system.total_swap(),
        swap_used: system.used_swap(),
        swap_free: system.free_swap(),
        ..Memory::default()
    };
    apply_platform_extras(&mut memory);
    memory
}

#[cfg(target_os = "linux")]
fn apply_platform_extras(memory: &mut Memory) {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return;
    };
    let field = |key: &str| -> u64 {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|kb| kb.parse::<u64>().ok())
            .map_or(0, |kb| kb * 1024)
    };
    memory.shared = field("Shmem");
    memory.buffers = field("Buffers");
    memory.cache = field("Cached") + field("SReclaimable");
}

#[cfg(not(target_os = "linux"))]
fn apply_platform_extras(_memory: &mut Memory) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_units_follow_procps_shape() {
        assert_eq!(human(0, false), "0B");
        assert_eq!(human(969 * 1024 * 1024, false), "969Mi");
        assert_eq!(
            human(7 * 1024 * 1024 * 1024 + 700 * 1024 * 1024, false),
            "7.6Gi"
        );
        assert_eq!(human(16 * 1024 * 1024 * 1024, false), "16Gi");
        assert_eq!(human(1000, true), "1.0k");
    }

    #[test]
    fn unit_flags_map_to_divisors() {
        assert_eq!(unit_divisor("bytes"), Some(1));
        assert_eq!(unit_divisor("mebi"), Some(1024 * 1024));
        assert_eq!(unit_divisor("mega"), Some(1_000_000));
        assert_eq!(unit_divisor("human"), None);
    }

    #[test]
    fn rows_have_procps_widths() {
        let line = row("Mem:", &["1".to_owned(), "2".to_owned()]);
        let expected = format!("Mem:{}1{}2\n", " ".repeat(15), " ".repeat(11));
        assert_eq!(line, expected);
    }

    #[test]
    fn live_memory_is_consistent() {
        let memory = read_memory();
        assert!(memory.total > 0);
        assert!(memory.used <= memory.total);
    }
}
