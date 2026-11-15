use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub fn default_backup() -> String {
    std::env::var("VERSION_CONTROL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "existing".into())
}
pub fn default_suffix() -> String {
    std::env::var("SIMPLE_BACKUP_SUFFIX")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "~".into())
}

pub fn prompt(message: &str) -> bool {
    eprint!("{message}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin().read_line(&mut line).is_ok() && line.starts_with(['y', 'Y'])
}
pub fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
pub fn remove(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        Err(io::Error::from(io::ErrorKind::IsADirectory))
    } else {
        #[cfg(windows)]
        if meta.file_type().is_symlink() && path.is_dir() {
            return fs::remove_dir(path);
        }
        fs::remove_file(path)
    }
}
#[cfg(unix)]
pub fn symlink(target: &Path, link: &Path, _directory: bool) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
#[cfg(windows)]
pub fn symlink(target: &Path, link: &Path, directory: bool) -> io::Result<()> {
    if directory {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}
#[cfg(not(any(unix, windows)))]
pub fn symlink(_target: &Path, _link: &Path, _directory: bool) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symbolic links are unsupported",
    ))
}

pub fn backup_destination(path: &Path, control: &str, suffix: &str) -> PathBuf {
    let mut simple = path.as_os_str().to_os_string();
    simple.push(suffix);
    let simple = PathBuf::from(simple);
    let numbered = matches!(control, "numbered" | "t")
        || matches!(control, "existing" | "nil")
            && (1..=1000).any(|n| numbered_path(path, n).exists());
    if numbered {
        (1..)
            .map(|n| numbered_path(path, n))
            .find(|p| !exists(p))
            .unwrap()
    } else {
        simple
    }
}

pub fn backup(path: &Path, source: &Path, control: &str, suffix: &str) -> io::Result<()> {
    if !exists(path) || matches!(control, "none" | "off") {
        return Ok(());
    }
    let target = backup_destination(path, control, suffix);
    if target == path
        || target == source
        || (exists(&target) && crate::common::meta::same_file(source, &target).unwrap_or(false))
    {
        return Err(io::Error::other(format!(
            "backing up '{}' would overwrite source '{}'",
            crate::display_path(path),
            crate::display_path(source)
        )));
    }
    if exists(&target) {
        remove(&target)?;
    }
    fs::rename(path, target)
}
fn numbered_path(path: &Path, n: u64) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".~{n}~"));
    PathBuf::from(name)
}

pub fn destination(source: &Path, dest: &Path, into_directory: bool) -> io::Result<PathBuf> {
    if into_directory {
        let name = source
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid source name"))?;
        Ok(dest.join(name))
    } else {
        Ok(dest.to_path_buf())
    }
}

pub fn inside(source: &Path, dest: &Path) -> bool {
    let Ok(root) = fs::canonicalize(source) else {
        return false;
    };
    let mut ancestor = dest;
    loop {
        if let Ok(parent) = fs::canonicalize(ancestor) {
            return parent.starts_with(&root);
        }
        match ancestor.parent() {
            Some(p) if p != ancestor && !p.as_os_str().is_empty() => ancestor = p,
            _ => return false,
        }
    }
}

pub fn symlink_diagnostic(error: &io::Error) -> &'static str {
    #[cfg(windows)]
    if error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1314) {
        return " (creating symbolic links on Windows requires Developer Mode or an elevated shell)";
    }
    let _ = error;
    ""
}

pub fn symlink_error_text(error: &io::Error) -> String {
    if symlink_diagnostic(error).is_empty() {
        crate::common::error::io_text(error)
    } else {
        "Permission denied".to_owned()
    }
}
