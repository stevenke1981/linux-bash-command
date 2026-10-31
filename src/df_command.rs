use crate::common::color;
use crate::common::size::human;
use std::io::{self, Write};
use std::path::PathBuf;
use sysinfo::{Disk, Disks};

const HELP: &str = "Usage: df [OPTION]... [FILE]...

Show filesystem capacity for mounted disks reported by the operating system.
With FILE arguments, show the mounted filesystem that contains each file.

Size options (last one wins):
  -k                      1K blocks (default)
  -m                      1M blocks
  -B, --block-size=SIZE   Blocks of SIZE: N, or K, M, G, T, P (powers of 1024),
                          or KB, MB, GB, TB, PB (powers of 1000)
  -h, --human-readable    Powers of 1024 (e.g. 1.5G)
  -H, --si                Powers of 1000 (e.g. 1.6G)

Other options:
  -T, --print-type        Include the filesystem type column
  -P, --portability       POSIX output: 1024-blocks style header, Capacity column
  -l, --local             Only local filesystems (common network types are excluded)
  -a, --all               Accepted; only mounted storage that the OS reports is listed
  -t, --type=TYPE         Only filesystems of TYPE (repeatable)
  -x, --exclude-type=TYPE Exclude filesystems of TYPE (repeatable)
      --help              Show this help

Not supported: -i (inode usage), because the OS interface does not report inode counts.
Available space and percentages can differ from GNU df because each OS reports
capacity differently. The Filesystem column shows the device name on Unix and the
drive letter on Windows.
Exit status: 0 on success, 1 on invalid arguments or unavailable paths.
";

/// File system types treated as remote by `-l`.
const NETWORK_TYPES: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smbfs",
    "smb3",
    "sshfs",
    "fuse.sshfs",
    "afs",
    "ncpfs",
    "davfs",
    "webdav",
    "9p",
    "vboxsf",
    "vmhgfs",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Size {
    /// Fixed block size in bytes.
    Blocks(u64),
    /// Human readable with the given base (1024 or 1000).
    Human(u64),
}

struct Options {
    size: Size,
    portability: bool,
    print_type: bool,
    local: bool,
    include_types: Vec<String>,
    exclude_types: Vec<String>,
    paths: Vec<PathBuf>,
}

enum Parsed {
    Help,
    Run(Options),
}

pub fn run(args: &[String]) -> i32 {
    let options = match parse(args) {
        Ok(Parsed::Run(options)) => options,
        Ok(Parsed::Help) => {
            print!("{HELP}");
            return 0;
        }
        Err(error) => {
            eprintln!("df: {error}\nTry 'df --help' for more information.");
            return 1;
        }
    };

    let disks = Disks::new_with_refreshed_list();
    let mounted = disks.list();
    if mounted.is_empty() {
        eprintln!("df: the operating system reported no mounted filesystems");
        return 1;
    }

    let (selected, had_error) = select(mounted, &options.paths);
    let mut selected: Vec<&Disk> = selected
        .into_iter()
        .filter(|disk| keeps(disk, &options))
        .collect();
    selected.sort_by(|left, right| left.mount_point().cmp(right.mount_point()));

    let rows: Vec<Vec<String>> = selected.iter().map(|disk| row(disk, &options)).collect();
    let output_status = print_table(&headers(&options), &rows, &options);
    if had_error || output_status != 0 {
        1
    } else {
        0
    }
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut size = Size::Blocks(1024);
    let mut options = Options {
        size: Size::Blocks(1024),
        portability: false,
        print_type: false,
        local: false,
        include_types: Vec::new(),
        exclude_types: Vec::new(),
        paths: Vec::new(),
    };
    let mut index = 0;
    let mut operands_only = false;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if operands_only || arg == "-" || !arg.starts_with('-') {
            options.paths.push(PathBuf::from(arg));
        } else if arg == "--" {
            operands_only = true;
        } else if arg == "--help" {
            return Ok(Parsed::Help);
        } else if let Some(long) = arg.strip_prefix("--") {
            parse_long(long, args, &mut index, &mut size, &mut options)?;
        } else {
            for (position, letter) in arg[1..].char_indices() {
                if matches!(letter, 'B' | 't' | 'x') {
                    let rest = &arg[1 + position + letter.len_utf8()..];
                    let value = value_or_next(rest, args, &mut index, &letter.to_string())?;
                    apply_value(letter, &value, &mut size, &mut options)?;
                    break;
                }
                apply_flag(letter, &mut size, &mut options)?;
            }
        }
    }
    options.size = size;
    Ok(Parsed::Run(options))
}

fn apply_flag(letter: char, size: &mut Size, options: &mut Options) -> Result<(), String> {
    match letter {
        'k' => *size = Size::Blocks(1024),
        'm' => *size = Size::Blocks(1024 * 1024),
        'h' => *size = Size::Human(1024),
        'H' => *size = Size::Human(1000),
        'T' => options.print_type = true,
        'P' => options.portability = true,
        'l' => options.local = true,
        'a' => {}
        'i' => {
            return Err(
                "-i (inode usage) is not available: the OS interface does not report inode counts"
                    .to_string(),
            )
        }
        other => return Err(format!("unknown option: -{other}")),
    }
    Ok(())
}

fn apply_value(
    letter: char,
    value: &str,
    size: &mut Size,
    options: &mut Options,
) -> Result<(), String> {
    match letter {
        'B' => *size = Size::Blocks(parse_block_size(value)?),
        't' => options.include_types.push(value.to_string()),
        _ => options.exclude_types.push(value.to_string()),
    }
    Ok(())
}

fn value_or_next(
    inline: &str,
    args: &[String],
    index: &mut usize,
    name: &str,
) -> Result<String, String> {
    if !inline.is_empty() {
        return Ok(inline.to_string());
    }
    let value = args
        .get(*index)
        .cloned()
        .ok_or_else(|| format!("option requires an argument -- '{name}'"))?;
    *index += 1;
    Ok(value)
}

fn parse_long(
    long: &str,
    args: &[String],
    index: &mut usize,
    size: &mut Size,
    options: &mut Options,
) -> Result<(), String> {
    let (name, inline) = match long.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (long, None),
    };
    match name {
        "human-readable" => *size = Size::Human(1024),
        "si" => *size = Size::Human(1000),
        "print-type" => options.print_type = true,
        "portability" => options.portability = true,
        "local" => options.local = true,
        "all" => {}
        "inodes" => return apply_flag('i', size, options),
        "block-size" | "type" | "exclude-type" => {
            let value = match inline {
                Some(value) => value.to_string(),
                None => {
                    let value = args
                        .get(*index)
                        .cloned()
                        .ok_or_else(|| format!("option '--{name}' requires an argument"))?;
                    *index += 1;
                    value
                }
            };
            let letter = match name {
                "block-size" => 'B',
                "type" => 't',
                _ => 'x',
            };
            apply_value(letter, &value, size, options)?;
        }
        _ => return Err(format!("unknown option: --{name}")),
    }
    Ok(())
}

/// Parses `N`, `K`, `M`, `G`, `T`, `P` (powers of 1024), or `KB`, `MB`, ... (powers of 1000),
/// with an optional leading count.
fn parse_block_size(text: &str) -> Result<u64, String> {
    const SUFFIXES: [(&str, u64); 10] = [
        ("PB", 1_000_000_000_000_000),
        ("TB", 1_000_000_000_000),
        ("GB", 1_000_000_000),
        ("MB", 1_000_000),
        ("KB", 1_000),
        ("P", 1 << 50),
        ("T", 1 << 40),
        ("G", 1 << 30),
        ("M", 1 << 20),
        ("K", 1 << 10),
    ];
    let invalid = || format!("invalid block size: '{text}'");
    let (number, factor) = SUFFIXES
        .iter()
        .find_map(|(suffix, factor)| text.strip_suffix(suffix).map(|rest| (rest, *factor)))
        .unwrap_or((text, 1));
    let count = if number.is_empty() {
        1
    } else {
        number.parse::<u64>().map_err(|_| invalid())?
    };
    let bytes = count.checked_mul(factor).ok_or_else(invalid)?;
    if bytes == 0 {
        return Err(invalid());
    }
    Ok(bytes)
}

/// Returns the mounts to report and whether a path could not be resolved.
fn select<'a>(mounted: &'a [Disk], paths: &[PathBuf]) -> (Vec<&'a Disk>, bool) {
    if paths.is_empty() {
        return (mounted.iter().collect(), false);
    }
    let mut selected: Vec<&Disk> = Vec::new();
    let mut failed = false;
    for path in paths {
        let canonical = match std::fs::canonicalize(path) {
            Ok(path) => path,
            Err(error) => {
                eprintln!("df: cannot access '{}': {error}", path.display());
                failed = true;
                continue;
            }
        };
        let disk = mounted
            .iter()
            .filter(|disk| contains_path(disk.mount_point(), &canonical))
            .max_by_key(|disk| disk.mount_point().components().count());
        match disk {
            Some(disk)
                if !selected
                    .iter()
                    .any(|item| item.mount_point() == disk.mount_point()) =>
            {
                selected.push(disk);
            }
            Some(_) => {}
            None => {
                eprintln!("df: no mounted filesystem found for '{}'", path.display());
                failed = true;
            }
        }
    }
    (selected, failed)
}

fn keeps(disk: &Disk, options: &Options) -> bool {
    let fs_type = disk.file_system().to_string_lossy();
    if options.local && NETWORK_TYPES.contains(&fs_type.as_ref()) {
        return false;
    }
    if !options.include_types.is_empty()
        && !options
            .include_types
            .iter()
            .any(|wanted| wanted == fs_type.as_ref())
    {
        return false;
    }
    !options
        .exclude_types
        .iter()
        .any(|unwanted| unwanted == fs_type.as_ref())
}

fn headers(options: &Options) -> Vec<String> {
    let size = match options.size {
        Size::Human(_) => "Size".to_string(),
        Size::Blocks(block) if options.portability => format!("{block}-blocks"),
        Size::Blocks(block) => format!("{}-blocks", block_label(block)),
    };
    let (used, avail, percent) = if options.portability {
        ("Used", "Available", "Capacity")
    } else {
        ("Used", "Avail", "Use%")
    };
    let mut headers = vec!["Filesystem".to_string()];
    if options.print_type {
        headers.push("Type".to_string());
    }
    headers.extend([
        size,
        used.to_string(),
        avail.to_string(),
        percent.to_string(),
    ]);
    headers.push("Mounted on".to_string());
    headers
}

/// `1K`, `1M`, `1G` for exact powers of 1024, otherwise the byte count.
fn block_label(bytes: u64) -> String {
    for (suffix, factor) in [("G", 1u64 << 30), ("M", 1 << 20), ("K", 1 << 10)] {
        if bytes >= factor && bytes.is_multiple_of(factor) {
            return format!("{}{suffix}", bytes / factor);
        }
    }
    bytes.to_string()
}

fn row(disk: &Disk, options: &Options) -> Vec<String> {
    let total = disk.total_space();
    let free = disk.available_space();
    let used = total.saturating_sub(free);
    let percent = if total == 0 {
        0
    } else {
        ((u128::from(used) * 100).div_ceil(u128::from(total))) as u64
    };
    let mut cells = vec![device_name(disk)];
    if options.print_type {
        cells.push(disk.file_system().to_string_lossy().into_owned());
    }
    cells.push(format_size(total, options.size));
    cells.push(format_size(used, options.size));
    cells.push(format_size(free, options.size));
    cells.push(format!("{percent}%"));
    cells.push(crate::display_path(disk.mount_point()));
    cells
}

/// The device name on Unix. Windows reports a volume label there instead, so the drive
/// root (for example `C:`) is shown.
fn device_name(disk: &Disk) -> String {
    let name = disk.name().to_string_lossy().into_owned();
    let mount = crate::display_path(disk.mount_point());
    let trimmed = mount.trim_end_matches('/');
    if cfg!(windows) || name.is_empty() {
        if trimmed.is_empty() {
            "/".to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        name
    }
}

fn format_size(bytes: u64, size: Size) -> String {
    match size {
        Size::Blocks(block) => bytes.div_ceil(block).to_string(),
        Size::Human(base) => human(bytes, base),
    }
}

fn print_table(headers: &[String], rows: &[Vec<String>], options: &Options) -> i32 {
    let last = headers.len() - 1;
    let widths: Vec<usize> = (0..headers.len())
        .map(|index| {
            rows.iter()
                .map(|row| row[index].chars().count())
                .fold(headers[index].chars().count(), usize::max)
        })
        .collect();
    let colored = color::enabled(None);
    let use_index = headers.iter().position(|header| header == "Use%");
    let mut out = String::new();
    // Padding is computed from the plain text; only the text itself is wrapped in SGR.
    let mut push = |cells: &[String], is_header: bool| {
        let line: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let sgr = if is_header {
                    BOLD
                } else if Some(index) == use_index {
                    usage_sgr(cell)
                } else {
                    ""
                };
                let shown = color::paint(colored, sgr, cell);
                if index == last {
                    return shown;
                }
                let pad = " ".repeat(widths[index].saturating_sub(cell.chars().count()));
                if is_text_column(index, options) {
                    format!("{shown}{pad}")
                } else {
                    format!("{pad}{shown}")
                }
            })
            .collect();
        out.push_str(line.join(" ").trim_end());
        out.push('\n');
    };
    push(headers, true);
    for row in rows {
        push(row, false);
    }
    match io::stdout().lock().write_all(out.as_bytes()) {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            eprintln!("df: cannot write output: {error}");
            1
        }
    }
}

/// Bold table header.
const BOLD: &str = "1";

/// Color for a Use% cell: green below 70%, yellow below 90%, red from 90%.
fn usage_sgr(cell: &str) -> &'static str {
    match cell.trim_end_matches('%').parse::<u64>() {
        Ok(percent) if percent >= 90 => "31",
        Ok(percent) if percent >= 70 => "33",
        Ok(_) => "32",
        Err(_) => "",
    }
}

/// Filesystem and Type columns are left-aligned; all numbers are right-aligned.
fn is_text_column(index: usize, options: &Options) -> bool {
    index == 0 || (options.print_type && index == 1)
}

fn contains_path(mount_point: &std::path::Path, path: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        fn normalized(path: &std::path::Path) -> String {
            let value = path.to_string_lossy().replace('/', "\\");
            let value = if value.to_ascii_lowercase().starts_with("\\\\?\\unc\\") {
                format!("\\\\{}", &value[8..])
            } else if let Some(stripped) = value.strip_prefix("\\\\?\\") {
                stripped.to_string()
            } else {
                value
            };
            value.to_lowercase()
        }
        let mount = normalized(mount_point);
        let path = normalized(path);
        path == mount || path.starts_with(&mount)
    }
    #[cfg(not(windows))]
    {
        path.starts_with(mount_point)
    }
}

#[cfg(test)]
mod tests {
    use super::{block_label, human, parse, parse_block_size, Parsed, Size};

    fn owned(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    fn size_of(args: &[&str]) -> Size {
        match parse(&owned(args)) {
            Ok(Parsed::Run(options)) => options.size,
            _ => panic!("expected options for {args:?}"),
        }
    }

    #[test]
    fn human_sizes_round_up_like_gnu() {
        assert_eq!(human(512, 1024), "512");
        assert_eq!(human(1024, 1024), "1.0K");
        assert_eq!(human(1_610_612_736, 1024), "1.5G");
        assert_eq!(human(10 * (1 << 30), 1024), "10G");
        assert_eq!(human(1_500_000_000, 1000), "1.5G");
        assert_eq!(human(1_000_000_000_001, 1000), "1.1T");
    }

    #[test]
    fn block_sizes_accept_units() {
        assert_eq!(parse_block_size("M").unwrap(), 1 << 20);
        assert_eq!(parse_block_size("2G").unwrap(), 2 << 30);
        assert_eq!(parse_block_size("MB").unwrap(), 1_000_000);
        assert_eq!(parse_block_size("512").unwrap(), 512);
        assert!(parse_block_size("0").is_err());
        assert!(parse_block_size("xM").is_err());
    }

    #[test]
    fn labels_blocks_by_unit() {
        assert_eq!(block_label(1024), "1K");
        assert_eq!(block_label(1 << 20), "1M");
        assert_eq!(block_label(512), "512");
    }

    #[test]
    fn bundles_flags_and_attached_values() {
        assert_eq!(size_of(&["-Ph"]), Size::Human(1024));
        assert_eq!(size_of(&["-hH"]), Size::Human(1000));
        assert_eq!(size_of(&["-BM"]), Size::Blocks(1 << 20));
        assert_eq!(size_of(&["-m", "-k"]), Size::Blocks(1024));
    }

    #[test]
    fn rejects_inode_report() {
        assert!(parse(&owned(&["-i"])).is_err());
    }
}
