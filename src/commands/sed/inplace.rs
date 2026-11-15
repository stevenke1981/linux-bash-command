use super::{exec, options::Options, parse::Program};
use crate::common::{error, path::display_path};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

struct Temporary {
    path: PathBuf,
    file: Option<File>,
}
impl Temporary {
    fn new(path: &Path) -> io::Result<Self> {
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        for _ in 0..100 {
            let name = format!(".agentcmd-sed-{:016x}.tmp", rand::random::<u64>());
            let temp = directory.join(name);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&temp) {
                Ok(file) => {
                    return Ok(Self {
                        path: temp,
                        file: Some(file),
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot create temporary file",
        ))
    }
    fn finish(mut self, target: &Path, backup: Option<&Path>) -> io::Result<()> {
        self.file.take();
        if let Some(backup) = backup {
            fs::rename(target, backup)?;
        }
        if let Err(failure) = fs::rename(&self.path, target) {
            if let Some(backup) = backup {
                let _ = fs::rename(backup, target);
            }
            return Err(failure);
        }
        Ok(())
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

pub fn run(options: &Options, program: &mut Program, _stdout: &mut dyn Write) -> io::Result<i32> {
    if options.files.is_empty() {
        eprintln!("sed: no input files");
        return Ok(4);
    }
    let mut status = 0;
    let mut destinations =
        super::output::Outputs::new(program, if options.null { 0 } else { b'\n' })?;
    for name in &options.files {
        let original = Path::new(name);
        let target = if options.follow {
            match fs::canonicalize(original) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!(
                        "sed: can't read {}: {}",
                        display_path(original),
                        error::io_text(&e)
                    );
                    status = 2;
                    continue;
                }
            }
        } else {
            original.to_owned()
        };
        let metadata = match fs::metadata(&target) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => {
                eprintln!(
                    "sed: couldn't edit {}: not a regular file",
                    display_path(original)
                );
                status = 4;
                continue;
            }
            Err(e) => {
                eprintln!(
                    "sed: can't read {}: {}",
                    display_path(original),
                    error::io_text(&e)
                );
                status = 2;
                continue;
            }
        };
        let mut temp = Temporary::new(&target)?;
        let mut output = BufWriter::with_capacity(65536, temp.file.as_mut().unwrap());
        let file = name.clone();
        for instruction in &mut program.instructions {
            instruction.selection.reset();
        }
        let outcome = exec::execute(options, program, &[file], &mut output, &mut destinations)?;
        output.flush()?;
        drop(output);
        if outcome.input_failed {
            status = 2;
            continue;
        }
        temp.file.as_mut().unwrap().sync_all()?;
        #[cfg(unix)]
        temp.file
            .as_mut()
            .unwrap()
            .set_permissions(metadata.permissions())?;
        #[cfg(not(unix))]
        let _ = metadata;
        let backup = backup_name(&target, options.in_place.as_deref().unwrap_or(""));
        temp.finish(&target, backup.as_deref())?;
        if outcome.status != 0 {
            status = outcome.status;
        }
        if outcome.quit {
            break;
        }
    }
    Ok(status)
}

fn backup_name(path: &Path, suffix: &str) -> Option<PathBuf> {
    if suffix.is_empty() {
        return None;
    }
    let path = path.to_string_lossy();
    Some(PathBuf::from(if suffix.contains('*') {
        suffix.replace('*', &path)
    } else {
        format!("{path}{suffix}")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suffix_backup() {
        assert_eq!(
            backup_name(Path::new("file"), ".bak"),
            Some(PathBuf::from("file.bak"))
        );
    }
    #[test]
    fn wildcard_backup() {
        assert_eq!(
            backup_name(Path::new("file"), "old-*-*"),
            Some(PathBuf::from("old-file-file"))
        );
    }
    #[test]
    fn no_backup() {
        assert_eq!(backup_name(Path::new("file"), ""), None);
    }
}
