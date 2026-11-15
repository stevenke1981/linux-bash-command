mod options;
use crate::common::{args, datetime, error, io, meta, path::display_path};
use crate::find_command::glob::Glob;
use options::Options;
use std::{
    cmp::Ordering,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
struct Node {
    path: PathBuf,
    name: String,
    metadata: fs::Metadata,
    directory: bool,
    link: Option<PathBuf>,
    children: Vec<Node>,
    error: Option<String>,
}
#[derive(Default)]
struct Counts {
    directories: usize,
    files: usize,
    failed: bool,
}
fn matches(patterns: &[String], name: &str, fold: bool) -> bool {
    let name = if fold {
        name.to_ascii_lowercase()
    } else {
        name.to_owned()
    };
    patterns.iter().any(|p| {
        Glob::new(
            &if fold {
                p.to_ascii_lowercase()
            } else {
                p.clone()
            },
            false,
        )
        .is_match(&name)
    })
}
fn ignored(path: &Path, directory: bool, rules: &[ignore::gitignore::Gitignore]) -> bool {
    for rule in rules.iter().rev() {
        let hit = rule.matched_path_or_any_parents(path, directory);
        if !hit.is_none() {
            return hit.is_ignore();
        }
    }
    false
}
fn load(
    path: &Path,
    name: String,
    depth: usize,
    o: &Options,
    rules: &mut Vec<ignore::gitignore::Gitignore>,
    counts: &mut Counts,
    included: bool,
) -> std::io::Result<Node> {
    let metadata = fs::symlink_metadata(path)?;
    let link = if metadata.file_type().is_symlink() {
        fs::read_link(path).ok()
    } else {
        None
    };
    let directory = metadata.is_dir();
    let mut node = Node {
        path: path.to_path_buf(),
        name,
        metadata,
        directory,
        link,
        children: vec![],
        error: None,
    };
    if !directory || depth >= o.level {
        return Ok(node);
    }
    let keep = rules.len();
    if o.gitignore {
        let mut builder = ignore::gitignore::GitignoreBuilder::new(path);
        builder.add(path.join(".gitignore"));
        if let Ok(rule) = builder.build() {
            rules.push(rule);
        }
    }
    let entries = match fs::read_dir(path) {
        Ok(v) => v,
        Err(e) => {
            node.error = Some(format!("error opening dir: {}", error::io_text(&e)));
            counts.failed = true;
            rules.truncate(keep);
            return Ok(node);
        }
    };
    let mut paths = vec![];
    for entry in entries {
        match entry {
            Ok(e) => {
                let name = e.file_name().to_string_lossy().into_owned();
                if !o.all && name.starts_with('.') {
                    continue;
                }
                let dir = e.file_type()?.is_dir();
                if o.dirs && !dir {
                    continue;
                }
                if matches(&o.exclude, &name, o.ignore_case) || ignored(&e.path(), dir, rules) {
                    continue;
                }
                paths.push((e.path(), name));
            }
            Err(e) => {
                counts.failed = true;
                node.error = Some(error::io_text(&e));
            }
        }
    }
    if paths.len() > o.limit {
        node.error = Some(format!(
            "{} entries exceeds filelimit, not opening dir",
            paths.len()
        ));
        rules.truncate(keep);
        return Ok(node);
    }
    for (path, name) in paths {
        let matched = matches(&o.include, &name, o.ignore_case);
        let include = included || o.include.is_empty() || matched;
        match load(
            &path,
            name,
            depth + 1,
            o,
            rules,
            counts,
            included || o.matchdirs && matched,
        ) {
            Ok(child) => {
                if !child.directory && !include {
                    continue;
                }
                if child.directory && o.prune && child.children.is_empty() && child.error.is_none()
                {
                    continue;
                }
                node.children.push(child);
            }
            Err(e) => {
                counts.failed = true;
                eprintln!("tree: {}: {}", display_path(&path), error::io_text(&e));
            }
        }
    }
    rules.truncate(keep);
    if o.sort != "none" || o.first != 0 {
        node.children.sort_by(|a, b| order(a, b, o));
    }
    Ok(node)
}
fn version(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let ai = i;
            let bj = j;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let x = &a[ai..i];
            let y = &b[bj..j];
            let ax = x.iter().position(|n| *n != b'0').unwrap_or(x.len());
            let by = y.iter().position(|n| *n != b'0').unwrap_or(y.len());
            let order = (x.len() - ax)
                .cmp(&(y.len() - by))
                .then_with(|| x[ax..].cmp(&y[by..]))
                .then_with(|| y.len().cmp(&x.len()));
            if !order.is_eq() {
                return order;
            }
        } else {
            let order = a[i].cmp(&b[j]);
            if !order.is_eq() {
                return order;
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}
fn changed(metadata: &fs::Metadata) -> i128 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec())
    }
    #[cfg(not(unix))]
    {
        metadata
            .created()
            .unwrap_or(std::time::UNIX_EPOCH)
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as i128
    }
}
fn order(a: &Node, b: &Node, o: &Options) -> Ordering {
    let group = match o.first {
        1 => b.directory.cmp(&a.directory),
        -1 => a.directory.cmp(&b.directory),
        _ => Ordering::Equal,
    };
    let sort = match o.sort.as_str() {
        "version" => version(a.name.as_bytes(), b.name.as_bytes()),
        "size" => b.metadata.len().cmp(&a.metadata.len()),
        "mtime" => b.metadata.modified().ok().cmp(&a.metadata.modified().ok()),
        "ctime" => changed(&b.metadata).cmp(&changed(&a.metadata)),
        "none" => Ordering::Equal,
        _ => a.name.as_bytes().cmp(b.name.as_bytes()),
    }
    .then_with(|| a.name.as_bytes().cmp(b.name.as_bytes()));
    group.then(if o.reverse { sort.reverse() } else { sort })
}
fn human(size: u64, base: u32) -> String {
    if base == 1 {
        return format!("{size:11}");
    }
    let mut n = size as f64;
    let mut unit = 0;
    while n >= f64::from(base) && unit < 6 {
        n /= f64::from(base);
        unit += 1;
    }
    if unit == 0 {
        format!("{size:4}")
    } else {
        let suffix = b" KMGTPE"[unit] as char;
        if n < 10.0 {
            format!("{n:3.1}{suffix}")
        } else {
            format!("{n:3.0}{suffix}")
        }
    }
}
fn prefix(node: &Node, o: &Options) -> String {
    let mut fields = vec![];
    if o.perms {
        fields.push(meta::mode_string(&node.path, &node.metadata));
    }
    if o.size != 0 {
        fields.push(human(node.metadata.len(), o.size));
    }
    if o.date {
        if let Ok(date) = node
            .metadata
            .modified()
            .and_then(|t| datetime::from_system_time(t).map_err(std::io::Error::other))
        {
            if let Ok(time) = datetime::format(&date, &o.timefmt) {
                fields.push(time);
            }
        }
    }
    if fields.is_empty() {
        String::new()
    } else {
        format!("[{}]  ", fields.join(" "))
    }
}
fn label(node: &Node, o: &Options, root: bool) -> String {
    let mut text = if root || o.full {
        display_path(&node.path)
    } else {
        node.name.clone()
    };
    if let Some(link) = &node.link {
        text.push_str(" -> ");
        text.push_str(&display_path(link));
    }
    if o.classify {
        if node.directory {
            text.push('/');
        } else if node.link.is_none() && meta::is_executable(&node.path, &node.metadata) {
            text.push('*');
        }
    }
    text
}
fn render(
    node: &Node,
    o: &Options,
    out: &mut dyn Write,
    branches: &mut Vec<bool>,
    root: bool,
    counts: &mut Counts,
) -> std::io::Result<()> {
    if !o.flat && !root {
        for continued in &branches[..branches.len() - 1] {
            out.write_all(if *continued {
                if o.ascii {
                    b"|   "
                } else {
                    "│   ".as_bytes()
                }
            } else {
                b"    "
            })?;
        }
        out.write_all(if *branches.last().unwrap() {
            if o.ascii {
                b"|-- "
            } else {
                "├── ".as_bytes()
            }
        } else if o.ascii {
            b"`-- "
        } else {
            "└── ".as_bytes()
        })?;
    }
    write!(out, "{}{}", prefix(node, o), label(node, o, root))?;
    if let Some(error) = &node.error {
        write!(out, "  [{error}]")?;
    }
    out.write_all(b"\n")?;
    for (i, child) in node.children.iter().enumerate() {
        if child.directory {
            counts.directories += 1;
        } else {
            counts.files += 1;
        }
        branches.push(i + 1 < node.children.len());
        render(child, o, out, branches, false, counts)?;
        branches.pop();
    }
    Ok(())
}
fn json(node: &Node, o: &Options, root: bool, counts: &mut Counts) -> serde_json::Value {
    let mut object = serde_json::json!({"type":if node.link.is_some(){"link"}else if node.directory{"directory"}else{"file"},"name":if root||o.full{display_path(&node.path)}else{node.name.clone()}});
    if let Some(target) = &node.link {
        object["target"] = display_path(target).into();
    }
    if o.size != 0 {
        object["size"] = node.metadata.len().into();
    }
    if o.perms {
        object["mode"] = format!(
            "{:04o}",
            meta::permission_bits_at(&node.path, &node.metadata) & 0o7777
        )
        .into();
        object["prot"] = meta::mode_string(&node.path, &node.metadata).into();
    }
    if let Some(error) = &node.error {
        object["error"] = error.clone().into();
    }
    if node.directory {
        object["contents"] = node
            .children
            .iter()
            .map(|child| {
                if child.directory {
                    counts.directories += 1;
                } else {
                    counts.files += 1;
                }
                json(child, o, false, counts)
            })
            .collect();
    }
    object
}
pub fn run(arguments: &[String]) -> i32 {
    let (o, paths) = match options::parse(arguments) {
        Ok(v) => v,
        Err(e) => return error::usage("tree", e, 1),
    };
    if paths.len() == 1 && matches!(paths[0].as_str(), "--help" | "--version") {
        return args::help_version("tree", &paths, options::HELP, true).unwrap();
    }
    let mut counts = Counts::default();
    let mut nodes = vec![];
    for name in paths {
        match load(
            Path::new(&name),
            name.clone(),
            0,
            &o,
            &mut vec![],
            &mut counts,
            false,
        ) {
            Ok(node) => {
                if node.directory {
                    counts.directories += 1;
                } else {
                    counts.files += 1;
                }
                nodes.push(node);
            }
            Err(e) => {
                eprintln!("tree: {name}: {}", error::io_text(&e));
                counts.failed = true;
            }
        }
    }
    io::output_result("tree", 2, |out| {
        if o.json {
            let mut data: Vec<_> = nodes
                .iter()
                .map(|n| json(n, &o, true, &mut counts))
                .collect();
            if o.report {
                data.push(serde_json::json!({"type":"report","directories":counts.directories,"files":counts.files}));
            }
            serde_json::to_writer_pretty(&mut *out, &data)?;
            out.write_all(b"\n")?;
        } else {
            for node in &nodes {
                render(node, &o, out, &mut vec![], true, &mut counts)?;
            }
            if o.report {
                if o.dirs {
                    writeln!(
                        out,
                        "\n{} director{}",
                        counts.directories,
                        if counts.directories == 1 { "y" } else { "ies" }
                    )?;
                } else {
                    writeln!(
                        out,
                        "\n{} director{}, {} file{}",
                        counts.directories,
                        if counts.directories == 1 { "y" } else { "ies" },
                        counts.files,
                        if counts.files == 1 { "" } else { "s" }
                    )?;
                }
            }
        }
        Ok(if counts.failed { 2 } else { 0 })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_numbers_sort_numerically() {
        assert!(version(b"f2", b"f10").is_lt());
        assert!(version(b"f001", b"f01").is_lt());
    }
    #[test]
    fn multiple_patterns_and_ascii_case() {
        assert!(matches(&["*.RS".into(), "file?".into()], "main.rs", true));
        assert!(!matches(&["*.RS".into()], "main.rs", false));
    }
    #[test]
    fn size_modes() {
        assert_eq!(human(1024, 1024), "1.0K");
        assert_eq!(human(1000, 1000), "1.0K");
        assert_eq!(human(7, 1), "          7");
    }
}
