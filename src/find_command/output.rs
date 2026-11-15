//! File-action destinations, including the portable standard-stream device names.
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum Destination {
    Stdout,
    Stderr,
    Null,
    File(PathBuf),
}

impl Destination {
    pub fn resolve(path: &str) -> io::Result<Self> {
        Ok(match path {
            "/dev/stdout" => Self::Stdout,
            "/dev/stderr" => Self::Stderr,
            "/dev/null" => Self::Null,
            _ => {
                let path = Path::new(path);
                let resolved = fs::canonicalize(path).or_else(|_| {
                    let path = std::path::absolute(path)?;
                    let parent = fs::canonicalize(path.parent().unwrap_or(&path))?;
                    Ok::<_, io::Error>(parent.join(path.file_name().unwrap_or_default()))
                })?;
                Self::File(resolved)
            }
        })
    }

    pub fn create(&self) -> io::Result<OutputFile> {
        Ok(match self {
            Self::Stdout => OutputFile::Stdout,
            Self::Stderr => OutputFile::Stderr,
            Self::Null => OutputFile::Null,
            Self::File(path) => OutputFile::File(File::create(path)?),
        })
    }
}

pub enum OutputFile {
    Stdout,
    Stderr,
    Null,
    File(File),
}

impl OutputFile {
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        fn send(mut out: impl Write, bytes: &[u8]) -> io::Result<()> {
            out.write_all(bytes).and_then(|()| out.flush())
        }
        match self {
            Self::Stdout => send(io::stdout().lock(), bytes),
            Self::Stderr => send(io::stderr().lock(), bytes),
            Self::Null => Ok(()),
            Self::File(file) => send(file, bytes),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_device_names_do_not_resolve_to_files() {
        assert_eq!(
            Destination::resolve("/dev/null").unwrap(),
            Destination::Null
        );
        assert_eq!(
            Destination::resolve("/dev/stdout").unwrap(),
            Destination::Stdout
        );
        assert_eq!(
            Destination::resolve("/dev/stderr").unwrap(),
            Destination::Stderr
        );
    }
}
