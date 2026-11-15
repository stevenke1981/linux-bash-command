use super::options::{Event, Options};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Selection {
    pub name: PathBuf,
    pub directory: PathBuf,
}
#[derive(Debug)]
pub struct Selections {
    pub items: Vec<Selection>,
    pub last_directory: PathBuf,
}
#[cfg(unix)]
fn os_name(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes)
}
#[cfg(not(unix))]
fn os_name(bytes: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&bytes).into_owned())
}
fn list_file(path: &Path, null: bool) -> io::Result<Vec<PathBuf>> {
    let source: Box<dyn BufRead> = if path == Path::new("-") {
        Box::new(BufReader::new(io::stdin()))
    } else {
        Box::new(BufReader::new(File::open(path)?))
    };
    let mut out = Vec::new();
    for entry in source.split(if null { 0 } else { b'\n' }) {
        let value = entry?;
        if !value.is_empty() {
            out.push(PathBuf::from(os_name(value)));
        }
    }
    Ok(out)
}
pub fn resolve(o: &Options) -> io::Result<Selections> {
    let mut directory = PathBuf::from(".");
    let mut items = Vec::new();
    for event in &o.events {
        match event {
            Event::Directory(path) => directory.push(path),
            Event::Operand(path) => items.push(Selection {
                name: path.into(),
                directory: directory.clone(),
            }),
            Event::FileList(path) => {
                let path = PathBuf::from(path);
                for name in list_file(&path, o.null)? {
                    items.push(Selection {
                        name,
                        directory: directory.clone(),
                    });
                }
            }
        }
    }
    Ok(Selections {
        items,
        last_directory: directory,
    })
}
fn glob_matches(pattern: &str, path: &Path) -> bool {
    #[cfg(unix)]
    let bytes: std::borrow::Cow<'_, [u8]> = {
        use std::os::unix::ffi::OsStrExt;
        std::borrow::Cow::Borrowed(path.as_os_str().as_bytes())
    };
    #[cfg(not(unix))]
    let bytes: std::borrow::Cow<'_, [u8]> =
        std::borrow::Cow::Owned(path.to_string_lossy().into_owned().into_bytes());
    let pat: String = pattern.as_bytes().iter().copied().map(char::from).collect();
    let name: String = bytes.iter().copied().map(char::from).collect();
    crate::find_command::glob::Glob::new(&pat, false).is_match(&name)
}
pub fn excluded(o: &Options, name: &Path) -> bool {
    o.excludes.iter().any(|pat| {
        glob_matches(pat, name)
            || name
                .components()
                .any(|part| glob_matches(pat, Path::new(part.as_os_str())))
    })
}
pub fn selected<'a>(o: &Options, selections: &'a Selections, name: &Path) -> Option<&'a Path> {
    if selections.items.is_empty() {
        return Some(&selections.last_directory);
    }
    selections
        .items
        .iter()
        .find(|sel| selection_matches(o, sel, name))
        .map(|sel| sel.directory.as_path())
}
pub fn selection_matches(o: &Options, sel: &Selection, name: &Path) -> bool {
    if sel.name == Path::new(".") {
        return true;
    }
    if o.wildcards {
        glob_matches(&sel.name.to_string_lossy(), name)
    } else {
        name == sel.name || name.starts_with(&sel.name)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_components() {
        let mut o = super::super::options::parse(&["-c".into()]).unwrap();
        o.excludes.push("*.tmp".into());
        assert!(excluded(&o, Path::new("d/a.tmp")));
        assert!(!excluded(&o, Path::new("d/a.txt")));
    }
    #[test]
    fn selector_prefix() {
        let o = super::super::options::parse(&["-t".into(), "dir".into()]).unwrap();
        let s = resolve(&o).unwrap();
        assert!(selected(&o, &s, Path::new("dir/a")).is_some());
        assert!(selected(&o, &s, Path::new("other")).is_none());
    }
    #[test]
    fn directories_are_positional() {
        let o = super::super::options::parse(&[
            "-c".into(),
            "-C".into(),
            "a".into(),
            "x".into(),
            "-C".into(),
            "b".into(),
            "y".into(),
        ])
        .unwrap();
        let s = resolve(&o).unwrap();
        assert_eq!(s.items[0].directory, PathBuf::from("./a"));
        assert_eq!(s.items[1].directory, PathBuf::from("./a/b"));
    }
}
