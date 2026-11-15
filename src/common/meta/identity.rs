use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId {
    pub dev: u64,
    pub inode: u64,
    pub nlink: u64,
}

/// A native identity when available, otherwise a resolved pathname. Zero inode
/// values denote unavailable identities and must never identify unrelated files.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FileKey {
    Native(u64, u64),
    Path(PathBuf),
}

impl FileId {
    pub fn key(self, path: &Path, metadata: &Metadata) -> io::Result<FileKey> {
        if self.inode != 0 {
            return Ok(FileKey::Native(self.dev, self.inode));
        }
        let resolved = if metadata.file_type().is_symlink() {
            // Resolve the parent only: lstat metadata describes the link, not its target.
            let absolute = std::path::absolute(path)?;
            let parent = fs::canonicalize(absolute.parent().unwrap_or(&absolute))?;
            parent.join(absolute.file_name().unwrap_or_default())
        } else {
            fs::canonicalize(path)?
        };
        Ok(FileKey::Path(resolved))
    }
}

pub fn file_key(path: &Path, metadata: &Metadata) -> io::Result<FileKey> {
    file_identity(path, metadata).key(path, metadata)
}

#[cfg(unix)]
pub fn file_identity(_path: &Path, metadata: &Metadata) -> FileId {
    use std::os::unix::fs::MetadataExt;
    FileId {
        dev: metadata.dev(),
        inode: metadata.ino(),
        nlink: metadata.nlink(),
    }
}

#[cfg(windows)]
pub fn file_identity(path: &Path, metadata: &Metadata) -> FileId {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let flags = FILE_FLAG_BACKUP_SEMANTICS
        | if metadata.file_type().is_symlink() {
            FILE_FLAG_OPEN_REPARSE_POINT
        } else {
            0
        };
    let result = fs::OpenOptions::new()
        .read(true)
        .access_mode(0)
        .share_mode(7)
        .custom_flags(flags)
        .open(path);
    if let Ok(file) = result {
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: the live file owns the handle and info points to enough writable storage.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } != 0 {
            // SAFETY: the successful API call initialized every field.
            let info = unsafe { info.assume_init() };
            return FileId {
                dev: u64::from(info.dwVolumeSerialNumber),
                inode: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
                nlink: u64::from(info.nNumberOfLinks),
            };
        }
    }
    FileId {
        dev: 0,
        inode: 0,
        nlink: 1,
    }
}

pub fn same_file(left: &Path, right: &Path) -> io::Result<bool> {
    let left_id = file_identity(left, &fs::metadata(left)?);
    let right_id = file_identity(right, &fs::metadata(right)?);
    if left_id.inode == 0 || right_id.inode == 0 {
        return Ok(fs::canonicalize(left)? == fs::canonicalize(right)?);
    }
    Ok((left_id.dev, left_id.inode) == (right_id.dev, right_id.inode))
}

pub fn change_time(path: &Path, metadata: &Metadata) -> Option<std::time::SystemTime> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        system_time(
            i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec()),
        )
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileBasicInfo, GetFileInformationByHandleEx, FILE_BASIC_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        let flags = FILE_FLAG_BACKUP_SEMANTICS
            | if metadata.file_type().is_symlink() {
                FILE_FLAG_OPEN_REPARSE_POINT
            } else {
                0
            };
        let file = fs::OpenOptions::new()
            .read(true)
            .access_mode(0)
            .share_mode(7)
            .custom_flags(flags)
            .open(path)
            .ok()?;
        let mut info = std::mem::MaybeUninit::<FILE_BASIC_INFO>::uninit();
        // SAFETY: FileBasicInfo selects the structure and both size and handle are valid.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileBasicInfo,
                info.as_mut_ptr().cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return None;
        }
        let info = unsafe { info.assume_init() };
        system_time((i128::from(info.ChangeTime) - 116444736000000000) * 100)
    }
}

fn system_time(nanos: i128) -> Option<std::time::SystemTime> {
    let duration = std::time::Duration::new(
        (nanos.unsigned_abs() / 1_000_000_000).try_into().ok()?,
        (nanos.unsigned_abs() % 1_000_000_000) as u32,
    );
    if nanos < 0 {
        std::time::UNIX_EPOCH.checked_sub(duration)
    } else {
        std::time::UNIX_EPOCH.checked_add(duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hard_links_share_identity() {
        let root = std::env::temp_dir().join(format!("agentcmd-identity-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("one");
        let second = root.join("two");
        fs::write(&first, b"x").unwrap();
        fs::hard_link(&first, &second).unwrap();
        let identity = file_identity(&first, &fs::metadata(&first).unwrap());
        assert_ne!(identity.inode, 0);
        assert_eq!(identity.nlink, 2);
        assert!(same_file(&first, &second).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_identity_is_an_error_for_comparisons() {
        let missing =
            std::env::temp_dir().join(format!("agentcmd-missing-identity-{}", std::process::id()));
        assert!(same_file(&missing, &missing).is_err());
    }

    #[test]
    fn unavailable_identities_use_distinct_resolved_paths() {
        let root =
            std::env::temp_dir().join(format!("agentcmd-zero-identity-{}", std::process::id()));
        fs::create_dir_all(root.join("child")).unwrap();
        let missing = FileId {
            dev: 0,
            inode: 0,
            nlink: 1,
        };
        let key = missing.key(&root, &fs::metadata(&root).unwrap()).unwrap();
        let child = root.join("child");
        assert_ne!(
            key,
            missing.key(&child, &fs::metadata(&child).unwrap()).unwrap()
        );
        assert_eq!(
            key,
            missing
                .key(&root.join("."), &fs::metadata(&root).unwrap())
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fallback_preserves_the_identity_of_a_symlink() {
        let root =
            std::env::temp_dir().join(format!("agentcmd-link-identity-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("file"), b"x").unwrap();
        std::os::unix::fs::symlink("file", root.join("link")).unwrap();
        let missing = FileId {
            dev: 0,
            inode: 0,
            nlink: 1,
        };
        let file = missing
            .key(
                &root.join("file"),
                &fs::metadata(root.join("file")).unwrap(),
            )
            .unwrap();
        let link = missing
            .key(
                &root.join("link"),
                &fs::symlink_metadata(root.join("link")).unwrap(),
            )
            .unwrap();
        assert_ne!(file, link);
        assert_eq!(
            file,
            missing
                .key(
                    &root.join("link"),
                    &fs::metadata(root.join("link")).unwrap()
                )
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
