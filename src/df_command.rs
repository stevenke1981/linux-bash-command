use crate::common::args::prepare_legacy;
use crate::common::error::EXIT_FAIL;
use crate::common::size::human;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
      --output[=FIELD_LIST]
                          Show the fields in FIELD_LIST, in order: source, fstype,
                          size, used, avail, pcent, file, target. Without a list the
                          default fields are shown. Mutually exclusive with -T and -P
      --total             Append a grand total row
  -l, --local             Only local filesystems (common network types are excluded)
  -a, --all               Accepted; only mounted storage that the OS reports is listed
  -t, --type=TYPE         Only filesystems of TYPE (repeatable)
  -x, --exclude-type=TYPE Exclude filesystems of TYPE (repeatable)
      --help              Show this help

Not supported: -i and --inodes (inode usage), and the itotal, iused, iavail and
ipcent fields of --output, because the OS interface does not report inode counts.
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

/// Fields of `--output`, in GNU order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Source,
    FsType,
    Size,
    Used,
    Avail,
    Pcent,
    File,
    Target,
}

/// The fields shown when `--output` has no list (inode fields are not available).
const DEFAULT_OUTPUT: &[Field] = &[
    Field::Source,
    Field::FsType,
    Field::Size,
    Field::Used,
    Field::Avail,
    Field::Pcent,
    Field::Target,
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
    /// `Some` when `--output` was given (possibly with the default list).
    output: Option<Vec<Field>>,
    total: bool,
    paths: Vec<PathBuf>,
}

enum Parsed {
    Help,
    Run(Options),
}

const DF_LONGS: &[&str] = &[
    "all",
    "block-size",
    "exclude-type",
    "help",
    "human-readable",
    "inodes",
    "local",
    "output",
    "portability",
    "print-type",
    "si",
    "total",
    "type",
];

pub fn run(args: &[String]) -> i32 {
    let expanded = match prepare_legacy(
        "df",
        args,
        DF_LONGS,
        &["block-size", "exclude-type", "type"],
        "Btx",
        EXIT_FAIL,
    ) {
        Ok(expanded) => expanded,
        Err(code) => return code,
    };
    let options = match parse(&expanded) {
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
    let mut selected: Vec<(&Disk, Option<String>)> = selected
        .into_iter()
        .filter(|(disk, _)| keeps(disk, &options))
        .collect();
    selected.sort_by(|left, right| left.0.mount_point().cmp(right.0.mount_point()));

    let fields = active_fields(&options);
    let mut rows: Vec<Vec<String>> = selected
        .iter()
        .map(|(disk, file)| cells(&Metrics::of(disk, file.clone()), &fields, &options))
        .collect();
    if options.total {
        let metrics: Vec<Metrics> = selected
            .iter()
            .map(|(disk, _)| Metrics::of(disk, None))
            .collect();
        rows.push(cells(&Metrics::total(&metrics), &fields, &options));
    }
    let text = fields
        .iter()
        .map(|field| is_text(*field))
        .collect::<Vec<_>>();
    let headers: Vec<String> = fields
        .iter()
        .map(|field| header(*field, &options))
        .collect();
    print_table(&headers, &rows, &text);
    if had_error {
        1
    } else {
        0
    }
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut options = Options {
        size: Size::Blocks(1024),
        portability: false,
        print_type: false,
        local: false,
        include_types: Vec::new(),
        exclude_types: Vec::new(),
        output: None,
        total: false,
        paths: Vec::new(),
    };
    let mut size = Size::Blocks(1024);
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
    if options.output.is_some() && (options.print_type || options.portability) {
        let other = if options.print_type { "-T" } else { "-P" };
        return Err(format!(
            "options '--output' and '{other}' are mutually exclusive"
        ));
    }
    Ok(Parsed::Run(options))
}

fn unsupported_inode(option: &str) -> String {
    format!("option '{option}' is not supported by agentcmd")
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
        'i' => return Err(unsupported_inode("-i")),
        other => return Err(format!("invalid option -- '{other}'")),
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
        "total" => options.total = true,
        "inodes" => return Err(unsupported_inode("--inodes")),
        "output" => options.output = Some(parse_output(inline)?),
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
        _ => return Err(format!("unrecognized option '--{name}'")),
    }
    Ok(())
}

/// Parses the `--output=FIELD_LIST` value. Without a list, the default fields are used.
fn parse_output(list: Option<&str>) -> Result<Vec<Field>, String> {
    let Some(list) = list else {
        return Ok(DEFAULT_OUTPUT.to_vec());
    };
    let mut fields: Vec<Field> = Vec::new();
    for name in list.split(',') {
        let field = match name {
            "source" => Field::Source,
            "fstype" => Field::FsType,
            "size" => Field::Size,
            "used" => Field::Used,
            "avail" => Field::Avail,
            "pcent" => Field::Pcent,
            "file" => Field::File,
            "target" => Field::Target,
            "itotal" | "iused" | "iavail" | "ipcent" => {
                return Err(format!(
                    "option '--output' field '{name}' is not supported by agentcmd"
                ))
            }
            _ => return Err(format!("option '--output': field '{name}' is unknown")),
        };
        if fields.contains(&field) {
            return Err(format!(
                "option '--output': field '{name}' is used more than once"
            ));
        }
        fields.push(field);
    }
    Ok(fields)
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

/// Returns the mounts to report, each with the path operand that selected it, and whether a
/// path could not be resolved.
fn select<'a>(mounted: &'a [Disk], paths: &[PathBuf]) -> (Vec<(&'a Disk, Option<String>)>, bool) {
    if paths.is_empty() {
        return (mounted.iter().map(|disk| (disk, None)).collect(), false);
    }
    let mut selected: Vec<(&Disk, Option<String>)> = Vec::new();
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
                    .any(|(item, _)| item.mount_point() == disk.mount_point()) =>
            {
                selected.push((disk, Some(crate::display_path(path))));
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

/// The columns to print: the `--output` list, or the default set (with Type under `-T`).
fn active_fields(options: &Options) -> Vec<Field> {
    if let Some(fields) = &options.output {
        return fields.clone();
    }
    let mut fields = vec![Field::Source];
    if options.print_type {
        fields.push(Field::FsType);
    }
    fields.extend([
        Field::Size,
        Field::Used,
        Field::Avail,
        Field::Pcent,
        Field::Target,
    ]);
    fields
}

fn is_text(field: Field) -> bool {
    matches!(
        field,
        Field::Source | Field::FsType | Field::File | Field::Target
    )
}

fn header(field: Field, options: &Options) -> String {
    match field {
        Field::Source => "Filesystem".to_string(),
        Field::FsType => "Type".to_string(),
        Field::Size => match options.size {
            Size::Human(_) => "Size".to_string(),
            Size::Blocks(block) if options.portability => format!("{block}-blocks"),
            Size::Blocks(block) => format!("{}-blocks", block_label(block)),
        },
        Field::Used => "Used".to_string(),
        Field::Avail if options.portability => "Available".to_string(),
        Field::Avail => "Avail".to_string(),
        Field::Pcent if options.portability => "Capacity".to_string(),
        Field::Pcent => "Use%".to_string(),
        Field::File => "File".to_string(),
        Field::Target => "Mounted on".to_string(),
    }
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

/// The values of one row (one mount, or the grand total).
struct Metrics {
    source: String,
    fstype: String,
    total: u64,
    used: u64,
    free: u64,
    file: Option<String>,
    target: String,
}

impl Metrics {
    fn of(disk: &Disk, file: Option<String>) -> Self {
        let total = disk.total_space();
        let free = disk.available_space();
        Self {
            source: device_name(disk),
            fstype: disk.file_system().to_string_lossy().into_owned(),
            total,
            used: total.saturating_sub(free),
            free,
            file,
            target: crate::display_path(disk.mount_point()),
        }
    }

    /// The sums of the given mounts, labelled `total`.
    fn total(rows: &[Metrics]) -> Self {
        let sum = |pick: fn(&Metrics) -> u64| rows.iter().map(pick).sum::<u64>();
        Self {
            source: "total".to_string(),
            fstype: String::new(),
            total: sum(|row| row.total),
            used: sum(|row| row.used),
            free: sum(|row| row.free),
            file: None,
            target: String::new(),
        }
    }

    fn percent(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            ((u128::from(self.used) * 100).div_ceil(u128::from(self.total))) as u64
        }
    }
}

fn cells(metrics: &Metrics, fields: &[Field], options: &Options) -> Vec<String> {
    fields
        .iter()
        .map(|field| match field {
            Field::Source => metrics.source.clone(),
            Field::FsType => metrics.fstype.clone(),
            Field::Size => format_size(metrics.total, options.size),
            Field::Used => format_size(metrics.used, options.size),
            Field::Avail => format_size(metrics.free, options.size),
            Field::Pcent => format!("{}%", metrics.percent()),
            Field::File => metrics.file.clone().unwrap_or_else(|| "-".to_string()),
            Field::Target => metrics.target.clone(),
        })
        .collect()
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

fn print_table(headers: &[String], rows: &[Vec<String>], text: &[bool]) -> i32 {
    let last = headers.len() - 1;
    let widths: Vec<usize> = (0..headers.len())
        .map(|index| {
            rows.iter()
                .map(|row| row[index].chars().count())
                .fold(headers[index].chars().count(), usize::max)
        })
        .collect();
    let mut out = String::new();
    let mut push = |cells: &[String]| {
        let line: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                if index == last {
                    cell.clone()
                } else if text[index] {
                    format!("{cell:<width$}", width = widths[index])
                } else {
                    format!("{cell:>width$}", width = widths[index])
                }
            })
            .collect();
        out.push_str(line.join(" ").trim_end());
        out.push('\n');
    };
    push(headers);
    for row in rows {
        push(row);
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

fn contains_path(mount_point: &Path, path: &Path) -> bool {
    #[cfg(windows)]
    {
        fn normalized(path: &Path) -> String {
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
    use super::{block_label, human, parse, parse_block_size, parse_output, Field, Parsed, Size};

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
        assert!(parse(&owned(&["--inodes"])).is_err());
    }

    #[test]
    fn output_fields_parse_in_order_and_reject_inode_fields() {
        assert_eq!(
            parse_output(Some("target,size,pcent")).unwrap(),
            [Field::Target, Field::Size, Field::Pcent]
        );
        assert_eq!(parse_output(None).unwrap().len(), 7);
        assert!(parse_output(Some("iused")).is_err());
        assert!(parse_output(Some("size,size")).is_err());
        assert!(parse_output(Some("bogus")).is_err());
    }

    #[test]
    fn output_conflicts_with_type_and_portability() {
        assert!(parse(&owned(&["--output=size", "-T"])).is_err());
        assert!(parse(&owned(&["--output=size", "-P"])).is_err());
        assert!(parse(&owned(&["--output=size", "--total"])).is_ok());
    }
}
