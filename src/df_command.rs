use std::path::PathBuf;
use sysinfo::{Disk, Disks};

const HELP: &str = "Usage: df [-h | --human-readable] [PATH...]\n\nShow filesystem capacity for mounted disks reported by the operating system. With PATH arguments, show the mounted filesystem containing each path.\n\nOptions:\n  -h, --human-readable  Show sizes using powers of 1024\n      --help            Show this help\n\nWithout -h, sizes are shown in 1 KiB blocks. Available space and used percentages can differ from GNU df because each OS reports disk capacity differently.\nExit status: 0 on success, 2 on invalid arguments or unavailable paths.\n";

pub fn run(args: &[String]) -> i32 {
    let mut human = false;
    let mut paths = Vec::new();
    let mut options = true;
    for arg in args {
        if options && arg == "--" {
            options = false;
        } else if options && arg == "--help" {
            print!("{HELP}");
            return 0;
        } else if options && matches!(arg.as_str(), "-h" | "--human-readable") {
            human = true;
        } else if options && arg.starts_with('-') && arg != "-" {
            eprintln!("df: unknown option: {arg}\nTry 'df --help' for usage.");
            return 2;
        } else {
            paths.push(PathBuf::from(arg));
        }
    }

    let disks = Disks::new_with_refreshed_list();
    let mounted = disks.list();
    if mounted.is_empty() {
        eprintln!("df: the operating system reported no mounted filesystems");
        return 2;
    }

    let (mut selected, had_error): (Vec<&Disk>, bool) = if paths.is_empty() {
        (mounted.iter().collect(), false)
    } else {
        let mut selected: Vec<&Disk> = Vec::new();
        let mut failed = false;
        for path in &paths {
            let path = match std::fs::canonicalize(path) {
                Ok(path) => path,
                Err(error) => {
                    eprintln!("df: cannot access '{}': {error}", path.display());
                    failed = true;
                    continue;
                }
            };
            let disk = mounted
                .iter()
                .filter(|disk| contains_path(disk.mount_point(), &path))
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
                    eprintln!("df: no mounted filesystem found for '{}':", path.display());
                    failed = true;
                }
            }
        }
        (selected, failed)
    };

    selected.sort_by(|left, right| left.mount_point().cmp(right.mount_point()));
    let mut rows = Vec::with_capacity(selected.len());
    for disk in selected {
        let total = disk.total_space();
        let free = disk.available_space();
        let used = total.saturating_sub(free);
        let used_percent = if total == 0 {
            0
        } else {
            ((u128::from(total.saturating_sub(free)) * 100).div_ceil(u128::from(total))) as u64
        };
        rows.push((
            disk.file_system().to_string_lossy().into_owned(),
            size(total, human),
            size(used, human),
            size(free, human),
            format!("{used_percent}%"),
            disk.mount_point().display().to_string(),
        ));
    }

    let filesystem_width = rows
        .iter()
        .map(|row| row.0.chars().count())
        .max()
        .unwrap_or("Filesystem".len())
        .max("Filesystem".len());
    println!(
        "{:<filesystem_width$} {:>10} {:>10} {:>10} {:>6}  Mounted on",
        "Filesystem", "Size", "Used", "Avail", "Use%"
    );
    for (filesystem, total, used, available, percent, mount) in rows {
        println!(
            "{filesystem:<filesystem_width$} {total:>10} {used:>10} {available:>10} {percent:>6}  {mount}"
        );
    }
    if had_error {
        2
    } else {
        0
    }
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

fn size(bytes: u64, human: bool) -> String {
    if !human {
        return bytes.div_ceil(1024).to_string();
    }
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::size;

    #[test]
    fn sizes_use_kib_blocks_and_binary_human_units() {
        assert_eq!(size(0, false), "0");
        assert_eq!(size(1, false), "1");
        assert_eq!(size(1_048_576, false), "1024");
        assert_eq!(size(1_048_576, true), "1.0M");
        assert_eq!(size(512, true), "512B");
    }
}
