use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
pub enum PathError {
    Parent,
    Empty,
}
pub fn link_or_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}
pub fn member_path(
    name: &Path,
    absolute: bool,
    strip: usize,
) -> Result<(PathBuf, bool), PathError> {
    let mut warned = false;
    let mut prefix = PathBuf::new();
    let mut components = Vec::new();
    for part in name.components() {
        match part {
            Component::ParentDir => return Err(PathError::Parent),
            Component::RootDir | Component::Prefix(_) if absolute => prefix.push(part.as_os_str()),
            Component::RootDir | Component::Prefix(_) => warned = true,
            Component::CurDir => {}
            Component::Normal(v) => components.push(v.to_os_string()),
        }
    }
    if components.len() <= strip {
        return Err(PathError::Empty);
    }
    for component in components.into_iter().skip(strip) {
        prefix.push(component);
    }
    Ok((prefix, warned))
}
pub fn checked_parent(path: &Path) -> io::Result<()> {
    walk_parent(path, true)
}
pub fn checked_existing_parent(path: &Path) -> io::Result<()> {
    walk_parent(path, false)
}
fn walk_parent(path: &Path, create: bool) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut walked = PathBuf::new();
    for part in parent.components() {
        walked.push(part.as_os_str());
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&walked) {
            Ok(meta) if link_or_reparse(&meta) => {
                return Err(io::Error::other("refusing to traverse a symbolic link"))
            }
            Ok(meta) if !meta.is_dir() => {
                return Err(io::Error::from(io::ErrorKind::NotADirectory))
            }
            Ok(_) => {}
            Err(e) if create && e.kind() == io::ErrorKind::NotFound => fs::create_dir(&walked)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_parent_before_stripping() {
        assert_eq!(
            member_path(Path::new("a/../b"), false, 2),
            Err(PathError::Parent)
        );
    }
    #[test]
    fn strip_components() {
        assert_eq!(
            member_path(Path::new("a/b/c"), false, 1).unwrap().0,
            PathBuf::from("b/c")
        );
        assert_eq!(member_path(Path::new("a"), false, 1), Err(PathError::Empty));
    }
    #[cfg(unix)]
    #[test]
    fn strips_absolute_by_default() {
        assert_eq!(
            member_path(Path::new("/a/b"), false, 0).unwrap(),
            (PathBuf::from("a/b"), true)
        );
        assert_eq!(
            member_path(Path::new("/a/b"), true, 0).unwrap().0,
            PathBuf::from("/a/b")
        );
    }
}
