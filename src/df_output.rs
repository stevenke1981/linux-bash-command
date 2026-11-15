use super::{device_name, format_size, headers as default_headers, Options, Size};
use sysinfo::Disk;

const FIELDS: &[&str] = &[
    "source", "fstype", "itotal", "iused", "iavail", "ipcent", "size", "used", "avail", "pcent",
    "file", "target",
];
pub fn parse_fields(value: Option<&str>) -> Result<Vec<String>, String> {
    let fields: Vec<_> = value.map_or_else(
        || FIELDS.iter().map(|field| field.to_string()).collect(),
        |value| value.split(',').map(str::to_owned).collect(),
    );
    if let Some(field) = fields
        .iter()
        .find(|field| !FIELDS.contains(&field.as_str()))
    {
        return Err(format!("field '{field}' unknown"));
    }
    let mut seen = std::collections::HashSet::new();
    for field in &fields {
        if !seen.insert(field) {
            return Err(format!("field '{field}' used more than once"));
        }
    }
    Ok(fields)
}
pub fn headers(fields: &[String], options: &Options) -> Vec<String> {
    fields
        .iter()
        .map(|field| match field.as_str() {
            "source" => "Filesystem".into(),
            "fstype" => "Type".into(),
            "itotal" => "Inodes".into(),
            "iused" => "IUsed".into(),
            "iavail" => "IFree".into(),
            "ipcent" => "IUse%".into(),
            "size" => {
                if matches!(options.size, Size::Human(_)) {
                    "Size".into()
                } else {
                    default_headers(options)[1].clone()
                }
            }
            "used" => "Used".into(),
            "avail" => "Avail".into(),
            "pcent" => "Use%".into(),
            "file" => "File".into(),
            "target" => "Mounted on".into(),
            _ => unreachable!(),
        })
        .collect()
}
fn percentage(used: u64, total: u64) -> String {
    if total == 0 {
        "-".into()
    } else {
        format!("{}%", (u128::from(used) * 100).div_ceil(u128::from(total)))
    }
}
pub fn row(disk: &Disk, options: &Options, operand: Option<&std::path::Path>) -> Vec<String> {
    let info = crate::commands::stat::filesystem::query(disk.mount_point()).ok();
    let inodes = info.as_ref().and_then(|info| info.files);
    let free_inodes = info.as_ref().and_then(|info| info.free_files);
    let total = disk.total_space();
    let free = disk.available_space();
    let used = total.saturating_sub(free);
    options
        .output
        .as_ref()
        .unwrap()
        .iter()
        .map(|field| match field.as_str() {
            "source" => device_name(disk),
            "fstype" => disk.file_system().to_string_lossy().into_owned(),
            "itotal" => inodes.map_or_else(|| "-".into(), |value| value.to_string()),
            "iavail" => free_inodes.map_or_else(|| "-".into(), |value| value.to_string()),
            "iused" => inodes.zip(free_inodes).map_or_else(
                || "-".into(),
                |(total, free)| total.saturating_sub(free).to_string(),
            ),
            "ipcent" => inodes.zip(free_inodes).map_or_else(
                || "-".into(),
                |(total, free)| percentage(total.saturating_sub(free), total),
            ),
            "size" => format_size(total, options.size),
            "used" => format_size(used, options.size),
            "avail" => format_size(free, options.size),
            "pcent" => percentage(used, total),
            "file" => operand.map_or_else(|| "-".into(), crate::display_path),
            "target" => crate::display_path(disk.mount_point()),
            _ => unreachable!(),
        })
        .collect()
}
pub fn total(disks: &[&Disk], options: &Options) -> Vec<String> {
    let total = disks.iter().map(|disk| disk.total_space()).sum::<u64>();
    let free = disks.iter().map(|disk| disk.available_space()).sum::<u64>();
    let used = total.saturating_sub(free);
    if let Some(fields) = &options.output {
        let infos: Vec<_> = disks
            .iter()
            .filter_map(|disk| crate::commands::stat::filesystem::query(disk.mount_point()).ok())
            .collect();
        let inodes = infos
            .iter()
            .map(|info| info.files)
            .collect::<Option<Vec<_>>>()
            .map(|values| values.iter().sum::<u64>());
        let free_inodes = infos
            .iter()
            .map(|info| info.free_files)
            .collect::<Option<Vec<_>>>()
            .map(|values| values.iter().sum::<u64>());
        return fields
            .iter()
            .map(|field| match field.as_str() {
                "source" => "total".into(),
                "size" => format_size(total, options.size),
                "used" => format_size(used, options.size),
                "avail" => format_size(free, options.size),
                "pcent" => percentage(used, total),
                "itotal" => inodes.map_or_else(|| "-".into(), |value| value.to_string()),
                "iavail" => free_inodes.map_or_else(|| "-".into(), |value| value.to_string()),
                "iused" => inodes.zip(free_inodes).map_or_else(
                    || "-".into(),
                    |(total, free)| total.saturating_sub(free).to_string(),
                ),
                "ipcent" => inodes.zip(free_inodes).map_or_else(
                    || "-".into(),
                    |(total, free)| percentage(total.saturating_sub(free), total),
                ),
                _ => "-".into(),
            })
            .collect();
    }
    let mut cells = vec!["total".into()];
    if options.print_type {
        cells.push("-".into());
    }
    cells.extend([
        format_size(total, options.size),
        format_size(used, options.size),
        format_size(free, options.size),
        percentage(used, total),
        "-".into(),
    ]);
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_defaults_to_every_field() {
        assert_eq!(parse_fields(None).unwrap().len(), 12);
    }
    #[test]
    fn rejects_unknown_fields() {
        assert!(parse_fields(Some("size,unknown")).is_err());
    }
    #[test]
    fn rejects_empty_field() {
        assert!(parse_fields(Some("")).is_err());
    }
    #[test]
    fn rejects_duplicate_fields() {
        assert_eq!(
            parse_fields(Some("source,source")).unwrap_err(),
            "field 'source' used more than once"
        );
    }
    #[test]
    fn percentage_rounds_up() {
        assert_eq!(percentage(1, 3), "34%");
    }
}
