use crate::common::{
    args::{parse_command, Item, OptSpec},
    error::{io_text, usage},
    io::output,
};
use crate::display_path;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Existing {
    All,
    Parent,
    Missing,
}

fn absolute(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn parts(path: &Path) -> (PathBuf, VecDeque<OsString>) {
    let mut root = PathBuf::new();
    let mut names = VecDeque::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => root.push(component.as_os_str()),
            Component::CurDir => {}
            other => names.push_back(other.as_os_str().to_owned()),
        }
    }
    (root, names)
}

pub fn lexical(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

pub fn canonicalize(
    path: &Path,
    existing: Existing,
    logical: bool,
    follow: bool,
) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    let mut require_directory = requires_directory(path);
    let mut require_existing_directory = ends_in_dot_component(path);
    let path = absolute(path)?;
    let path = if logical || !follow {
        lexical(&path)
    } else {
        path
    };
    let (mut result, mut pending) = parts(&path);
    let mut links = 0;
    while let Some(name) = pending.pop_front() {
        if name == ".." {
            result.pop();
            continue;
        }
        let candidate = result.join(&name);
        match fs::symlink_metadata(&candidate) {
            Ok(meta) if follow && meta.file_type().is_symlink() => {
                links += 1;
                if links > 40 {
                    return Err(io::Error::other("Too many levels of symbolic links"));
                }
                let target = fs::read_link(&candidate)?;
                if pending.is_empty() && requires_directory(&target) {
                    require_directory = true;
                    require_existing_directory |= ends_in_dot_component(&target);
                }
                let (root, mut names) = parts(&target);
                if target.is_absolute() {
                    result = root;
                }
                names.append(&mut pending);
                pending = names;
            }
            Ok(meta) => {
                if !pending.is_empty()
                    && !meta.is_dir()
                    && !(meta.file_type().is_symlink()
                        && !follow
                        && fs::metadata(&candidate).is_ok_and(|meta| meta.is_dir()))
                    && existing != Existing::Missing
                {
                    return Err(io::Error::from(io::ErrorKind::NotADirectory));
                }
                result = candidate;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) && (existing == Existing::Missing
                    || (existing == Existing::Parent && pending.is_empty())) =>
            {
                result = candidate;
            }
            Err(error) => return Err(error),
        }
    }
    if require_directory && existing != Existing::Missing {
        match fs::metadata(&result) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(io::Error::from(io::ErrorKind::NotADirectory)),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && existing == Existing::Parent
                    && !require_existing_directory => {}
            Err(error) => return Err(error),
        }
    }
    Ok(result)
}

fn requires_directory(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    let separator = |byte: u8| byte == b'/' || (cfg!(windows) && byte == b'\\');
    bytes.last().is_some_and(|byte| separator(*byte)) || ends_in_dot_component(path)
}

fn ends_in_dot_component(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    bytes.ends_with(b"/.") || (cfg!(windows) && bytes.ends_with(b"\\."))
}

pub fn relative(target: &Path, base: &Path) -> PathBuf {
    let target: Vec<_> = target.components().collect();
    let base: Vec<_> = base.components().collect();
    if target.first() != base.first() {
        return target.iter().collect();
    }
    let shared = target.iter().zip(&base).take_while(|(a, b)| a == b).count();
    let mut result = PathBuf::new();
    for _ in shared..base.len() {
        result.push("..");
    }
    for component in &target[shared..] {
        result.push(component.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

const REAL_HELP: &str = "Usage: realpath [OPTION]... FILE...\nPrint resolved absolute file names.\n  -e, --canonicalize-existing  all components must exist\n  -m, --canonicalize-missing   allow missing components\n  -L, --logical                resolve .. before symbolic links\n  -P, --physical               resolve symbolic links before ..\n  -q, --quiet                  suppress error diagnostics\n      --relative-to=DIR        print relative to DIR\n      --relative-base=DIR      print absolute paths outside DIR\n  -s, --strip, --no-symlinks    do not resolve symbolic links\n  -z, --zero                   end each output with NUL\n      --help                   display this help\n      --version                output version information\n";
const LINK_HELP: &str = "Usage: readlink [OPTION]... FILE...\nPrint symbolic link targets or canonical file names.\n  -f, --canonicalize           all but the final component must exist\n  -e, --canonicalize-existing  all components must exist\n  -m, --canonicalize-missing   allow missing components\n  -n, --no-newline             omit the final delimiter\n  -q, --quiet, -s, --silent    suppress errors (default)\n  -v, --verbose                report errors\n  -z, --zero                   end each output with NUL\n      --help                   display this help\n      --version                output version information\n";

pub fn run(args: &[String], readlink: bool) -> i32 {
    let command = if readlink { "readlink" } else { "realpath" };
    let mut specs = vec![
        OptSpec::flag("canonicalize-existing", 'e'),
        OptSpec::flag("canonicalize-missing", 'm'),
        OptSpec::flag("quiet", 'q'),
        OptSpec::flag("zero", 'z'),
    ];
    if readlink {
        specs.extend([
            OptSpec::flag("canonicalize", 'f'),
            OptSpec::flag("no-newline", 'n'),
            OptSpec::flag("silent", 's'),
            OptSpec::flag("verbose", 'v'),
        ]);
    } else {
        specs.extend([
            OptSpec::flag("logical", 'L'),
            OptSpec::flag("physical", 'P'),
            OptSpec::flag("strip", 's'),
            OptSpec::flag("no-symlinks", '\0'),
            OptSpec::value("relative-to", '\0'),
            OptSpec::value("relative-base", '\0'),
        ]);
    }
    let items = match parse_command(
        command,
        args,
        if readlink { LINK_HELP } else { REAL_HELP },
        &specs,
        true,
    ) {
        Ok(items) => items,
        Err(code) => return code,
    };
    let (mut existing, mut resolve, mut logical, mut follow, mut quiet, mut newline, mut delimiter) = (
        Existing::Parent,
        !readlink,
        false,
        true,
        readlink,
        true,
        b'\n',
    );
    let (mut relative_to, mut relative_base) = (None, None);
    let mut paths = Vec::new();
    for item in items {
        match item {
            Item::Operand(path) => paths.push(path),
            Item::Opt("canonicalize-existing", _) => {
                existing = Existing::All;
                resolve = true;
            }
            Item::Opt("canonicalize-missing", _) => {
                existing = Existing::Missing;
                resolve = true;
            }
            Item::Opt("canonicalize", _) => {
                existing = Existing::Parent;
                resolve = true;
            }
            Item::Opt("logical", _) => logical = true,
            Item::Opt("physical", _) => logical = false,
            Item::Opt("strip" | "no-symlinks", _) => follow = false,
            Item::Opt("quiet" | "silent", _) => quiet = true,
            Item::Opt("verbose", _) => quiet = false,
            Item::Opt("no-newline", _) => newline = false,
            Item::Opt("zero", _) => delimiter = 0,
            Item::Opt("relative-to", value) => relative_to = value,
            Item::Opt("relative-base", value) => relative_base = value,
            _ => unreachable!(),
        }
    }
    if paths.is_empty() {
        return usage(command, "missing operand", 1);
    }
    let resolve_base = |path: &String| canonicalize(Path::new(path), existing, logical, follow);
    let relative_to = match relative_to.as_ref().map(resolve_base).transpose() {
        Ok(value) => value,
        Err(error) => {
            if !quiet {
                eprintln!("{command}: {}", io_text(&error));
            }
            return 1;
        }
    };
    let relative_base = match relative_base.as_ref().map(resolve_base).transpose() {
        Ok(value) => value,
        Err(error) => {
            if !quiet {
                eprintln!("{command}: {}", io_text(&error));
            }
            return 1;
        }
    };
    let mut failed = false;
    let code = output(command, |out| {
        for path in &paths {
            let result = if resolve {
                canonicalize(Path::new(path), existing, logical, follow)
            } else {
                fs::read_link(path)
            };
            match result {
                Ok(mut target) => {
                    let base = relative_to.as_ref().or(relative_base.as_ref());
                    if relative_base
                        .as_ref()
                        .is_none_or(|base| target.starts_with(base))
                    {
                        if let Some(base) = base {
                            target = relative(&target, base);
                        }
                    }
                    out.write_all(display_path(&target).as_bytes())?;
                    if newline {
                        out.write_all(&[delimiter])?;
                    }
                }
                Err(error) => {
                    failed = true;
                    if !quiet {
                        eprintln!("{command}: {path}: {}", io_text(&error));
                    }
                }
            }
        }
        Ok(())
    });
    code.max(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_parent() {
        assert_eq!(lexical(Path::new("/a/b/../c")), Path::new("/a/c"));
    }
    #[test]
    fn lexical_root() {
        assert_eq!(lexical(Path::new("/../../a")), Path::new("/a"));
    }
    #[test]
    fn relative_sibling() {
        assert_eq!(
            relative(Path::new("/a/b"), Path::new("/a/c")),
            Path::new("../b")
        );
    }
    #[test]
    fn relative_equal() {
        assert_eq!(relative(Path::new("/a"), Path::new("/a")), Path::new("."));
    }
    #[test]
    fn relative_child() {
        assert_eq!(relative(Path::new("/a/b"), Path::new("/a")), Path::new("b"));
    }
    #[test]
    fn missing_components_are_allowed() {
        let path = std::env::temp_dir().join("agentcmd-canonical-missing/child");
        assert!(canonicalize(&path, Existing::All, false, true).is_err());
        assert_eq!(
            canonicalize(&path, Existing::Missing, false, true).unwrap(),
            path
        );
    }
}
