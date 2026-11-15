//! Stable external sorting of opaque byte records with bounded merge fan-in.
use std::{
    cmp::Ordering,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

pub struct TemporaryFile {
    file: Option<File>,
    path: PathBuf,
}
impl TemporaryFile {
    pub fn new(directory: &Path) -> io::Result<Self> {
        Self::with_mode(directory, 0o600)
    }
    /// The OS applies the process umask when creating an atomic output file.
    pub fn with_mode(directory: &Path, mode: u32) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..128 {
            let sequence = NEXT.fetch_add(1, AtomicOrdering::Relaxed);
            let nonce: u64 = rand::random();
            let path = directory.join(format!(
                ".agentcmd-{}-{nonce:016x}-{sequence}.tmp",
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(mode);
            }
            #[cfg(not(unix))]
            let _ = mode;
            match options.open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        file: Some(file),
                        path,
                    })
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot create temporary file",
        ))
    }
    pub fn file(&mut self) -> &mut File {
        self.file.as_mut().unwrap()
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn reader(&mut self) -> io::Result<BufReader<&mut File>> {
        self.file().seek(SeekFrom::Start(0))?;
        Ok(BufReader::with_capacity(
            super::io::BUFFER_SIZE,
            self.file(),
        ))
    }
    /// Close and atomically replace a destination only after a successful write.
    pub fn persist(mut self, destination: &Path) -> io::Result<()> {
        self.file().sync_all()?;
        drop(self.file.take());
        #[cfg(not(windows))]
        fs::rename(&self.path, destination)?;
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            let wide = |p: &Path| {
                p.as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>()
            };
            let source = wide(&self.path);
            let target = wide(destination);
            if unsafe {
                MoveFileExW(
                    source.as_ptr(),
                    target.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
    /// Publish only if the destination does not exist, without a check/create race.
    pub fn persist_noclobber(mut self, destination: &Path) -> io::Result<()> {
        self.file().sync_all()?;
        drop(self.file.take());
        #[cfg(not(windows))]
        fs::hard_link(&self.path, destination)?;
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
            let wide = |p: &Path| {
                p.as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>()
            };
            let source = wide(&self.path);
            let target = wide(destination);
            if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            // A failed publish may leave copied read-only attributes on our
            // private staging file. Clear them so cleanup can remove it.
            let metadata = self
                .file
                .as_ref()
                .and_then(|file| file.metadata().ok())
                .or_else(|| fs::symlink_metadata(&self.path).ok());
            if let Some(metadata) = metadata.filter(|m| m.is_file() && m.permissions().readonly()) {
                let mut permissions = metadata.permissions();
                // This branch is Windows-only: it changes an attribute, not Unix mode bits.
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                if let Some(file) = self.file.as_ref() {
                    let _ = file.set_permissions(permissions);
                } else {
                    let _ = fs::set_permissions(&self.path, permissions);
                }
            }
        }
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Clone)]
pub struct Config {
    pub memory_limit: usize,
    pub temporary_dirs: Vec<PathBuf>,
    pub batch_size: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            memory_limit: 256 * 1024 * 1024,
            temporary_dirs: vec![std::env::temp_dir()],
            batch_size: 16,
        }
    }
}
struct Run {
    temporary: TemporaryFile,
    level: u32,
}

pub struct Sorter<'a, F: Fn(&[u8], &[u8]) -> Ordering + ?Sized> {
    config: Config,
    compare: &'a F,
    records: Vec<Vec<u8>>,
    bytes: usize,
    runs: Vec<Run>,
    unique: bool,
    directory: usize,
}
impl<'a, F: Fn(&[u8], &[u8]) -> Ordering + ?Sized> Sorter<'a, F> {
    pub fn new(mut config: Config, compare: &'a F) -> Self {
        config.memory_limit = config.memory_limit.max(1);
        config.batch_size = config.batch_size.max(2);
        if config.temporary_dirs.is_empty() {
            config.temporary_dirs.push(std::env::temp_dir());
        }
        Self {
            config,
            compare,
            records: Vec::new(),
            bytes: 0,
            runs: Vec::new(),
            unique: false,
            directory: 0,
        }
    }
    pub fn set_unique(&mut self, unique: bool) {
        self.unique = unique;
    }
    fn temporary(&mut self) -> io::Result<TemporaryFile> {
        let path = &self.config.temporary_dirs[self.directory % self.config.temporary_dirs.len()];
        self.directory = self.directory.wrapping_add(1);
        TemporaryFile::new(path)
    }
    pub fn push(&mut self, record: Vec<u8>) -> io::Result<()> {
        // Account for the record plus the growable and stable-sort pointer arrays.
        let bytes = record
            .len()
            .saturating_add(3 * std::mem::size_of::<Vec<u8>>());
        if !self.records.is_empty() && self.bytes.saturating_add(bytes) > self.config.memory_limit {
            self.spill()?;
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.records.push(record);
        if self.bytes >= self.config.memory_limit {
            self.spill()?;
        }
        Ok(())
    }
    fn spill(&mut self) -> io::Result<()> {
        if self.records.is_empty() {
            return Ok(());
        }
        let mut rows = std::mem::take(&mut self.records);
        self.bytes = 0;
        rows.sort_by(|a, b| (self.compare)(a, b));
        let mut temporary = self.temporary()?;
        {
            let mut output = BufWriter::with_capacity(super::io::BUFFER_SIZE, temporary.file());
            emit_rows(&rows, self.compare, self.unique, &mut |row| {
                write_record(&mut output, row)
            })?;
            output.flush()?;
        }
        self.runs.push(Run {
            temporary,
            level: 0,
        });
        self.compact_tail()
    }
    fn compact_tail(&mut self) -> io::Result<()> {
        let batch = self.config.batch_size;
        while self.runs.len() >= batch {
            let start = self.runs.len() - batch;
            let level = self.runs[start].level;
            if self.runs[start..].iter().any(|run| run.level != level) {
                break;
            }
            let inputs = self.runs.split_off(start);
            let run = self.merge_to_run(inputs, level + 1)?;
            self.runs.push(run);
        }
        Ok(())
    }
    fn merge_to_run(&mut self, mut inputs: Vec<Run>, level: u32) -> io::Result<Run> {
        let mut temporary = self.temporary()?;
        {
            let mut output = BufWriter::with_capacity(super::io::BUFFER_SIZE, temporary.file());
            merge(&mut inputs, self.compare, self.unique, &mut |row| {
                write_record(&mut output, row)
            })?;
            output.flush()?;
        }
        Ok(Run { temporary, level })
    }
    /// Preserve an already-sorted input for `sort -m`, consuming it before output
    /// can truncate an aliased input file. Its internal order is left untouched.
    pub fn add_sorted_input(&mut self, input: &mut dyn BufRead, delimiter: u8) -> io::Result<()> {
        let mut row = Vec::new();
        if input.read_until(delimiter, &mut row)? == 0 {
            return Ok(());
        }
        let mut temporary = self.temporary()?;
        {
            let mut output = BufWriter::with_capacity(super::io::BUFFER_SIZE, temporary.file());
            loop {
                if row.last() == Some(&delimiter) {
                    row.pop();
                }
                write_record(&mut output, &row)?;
                row.clear();
                if input.read_until(delimiter, &mut row)? == 0 {
                    break;
                }
            }
            output.flush()?;
        }
        self.runs.push(Run {
            temporary,
            level: 0,
        });
        self.compact_tail()
    }
    pub fn finish(mut self, emit: &mut dyn FnMut(&[u8]) -> io::Result<()>) -> io::Result<()> {
        if self.runs.is_empty() {
            self.records.sort_by(|a, b| (self.compare)(a, b));
            return emit_rows(&self.records, self.compare, self.unique, emit);
        }
        self.spill()?;
        while self.runs.len() > self.config.batch_size {
            let mut rest = self.runs.split_off(self.config.batch_size);
            let inputs = std::mem::take(&mut self.runs);
            let level = inputs.iter().map(|run| run.level).max().unwrap_or(0) + 1;
            let run = self.merge_to_run(inputs, level)?;
            self.runs.push(run);
            self.runs.append(&mut rest);
        }
        merge(&mut self.runs, self.compare, self.unique, emit)
    }
}

fn emit_rows<F: Fn(&[u8], &[u8]) -> Ordering + ?Sized>(
    rows: &[Vec<u8>],
    compare: &F,
    unique: bool,
    emit: &mut dyn FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut previous: Option<&[u8]> = None;
    for row in rows {
        if !unique || previous.is_none_or(|old| !compare(old, row).is_eq()) {
            emit(row)?;
            previous = Some(row);
        }
    }
    Ok(())
}
fn write_record(output: &mut dyn Write, record: &[u8]) -> io::Result<()> {
    output.write_all(&(record.len() as u64).to_le_bytes())?;
    output.write_all(record)
}
fn read_record(input: &mut dyn Read) -> io::Result<Option<Vec<u8>>> {
    let mut size = [0; 8];
    loop {
        match input.read(&mut size[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    input.read_exact(&mut size[1..])?;
    let size = usize::try_from(u64::from_le_bytes(size)).map_err(|_| io::ErrorKind::InvalidData)?;
    let mut record = vec![0; size];
    input.read_exact(&mut record)?;
    Ok(Some(record))
}
fn merge<F: Fn(&[u8], &[u8]) -> Ordering + ?Sized>(
    runs: &mut [Run],
    compare: &F,
    unique: bool,
    emit: &mut dyn FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut readers = runs
        .iter_mut()
        .map(|run| run.temporary.reader())
        .collect::<io::Result<Vec<_>>>()?;
    let mut heads = readers
        .iter_mut()
        .map(|r| read_record(r))
        .collect::<io::Result<Vec<_>>>()?;
    let mut previous: Option<Vec<u8>> = None;
    loop {
        let next = heads
            .iter()
            .enumerate()
            .filter_map(|(i, row)| row.as_ref().map(|row| (i, row)))
            .min_by(|(a, left), (b, right)| compare(left, right).then_with(|| a.cmp(b)))
            .map(|(i, _)| i);
        let Some(index) = next else {
            break;
        };
        let row = heads[index].take().unwrap();
        if !unique
            || previous
                .as_ref()
                .is_none_or(|old| !compare(old, &row).is_eq())
        {
            emit(&row)?;
            if unique {
                previous = Some(row);
            }
        }
        heads[index] = read_record(&mut readers[index])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Sandbox(PathBuf);
    impl Sandbox {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "agentcmd-external-{}-{:016x}",
                std::process::id(),
                rand::random::<u64>()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn config(&self, limit: usize) -> Config {
            Config {
                memory_limit: limit,
                temporary_dirs: vec![self.0.clone()],
                batch_size: 2,
            }
        }
        fn empty(&self) -> bool {
            fs::read_dir(&self.0).unwrap().next().is_none()
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn spills_sort_stably_and_clean_all_temporary_runs() {
        let sandbox = Sandbox::new();
        let compare = |a: &[u8], b: &[u8]| a[0].cmp(&b[0]);
        let mut sorter = Sorter::new(sandbox.config(96), &compare);
        for n in 0..100 {
            sorter
                .push(format!("{}-{n:03}", if n % 2 == 0 { 'b' } else { 'a' }).into_bytes())
                .unwrap();
        }
        assert!(!sandbox.empty());
        let mut rows = Vec::new();
        sorter
            .finish(&mut |row| {
                rows.push(row.to_vec());
                Ok(())
            })
            .unwrap();
        let expected: Vec<_> = (0..100)
            .filter(|n| n % 2 == 1)
            .chain((0..100).filter(|n| n % 2 == 0))
            .map(|n| format!("{}-{n:03}", if n % 2 == 0 { 'b' } else { 'a' }).into_bytes())
            .collect();
        assert_eq!(rows, expected);
        assert!(sandbox.empty());
    }
    #[test]
    fn memory_unique_and_sorted_inputs_preserve_first_records() {
        let sandbox = Sandbox::new();
        let compare = |a: &[u8], b: &[u8]| a[0].cmp(&b[0]);
        let mut sorter = Sorter::new(sandbox.config(4096), &compare);
        sorter.set_unique(true);
        for row in [b"b0", b"a1", b"a2"] {
            sorter.push(row.to_vec()).unwrap();
        }
        let mut rows = Vec::new();
        sorter
            .finish(&mut |row| {
                rows.push(row.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(rows, [b"a1", b"b0"]);
        let mut sorter = Sorter::new(sandbox.config(1), &compare);
        sorter.set_unique(true);
        sorter
            .add_sorted_input(&mut &b"a1\nb1\n"[..], b'\n')
            .unwrap();
        sorter
            .add_sorted_input(&mut &b"a2\nb2\nc2"[..], b'\n')
            .unwrap();
        let mut rows = Vec::new();
        sorter
            .finish(&mut |row| {
                rows.push(row.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(rows, [b"a1", b"b1", b"c2"]);
        assert!(sandbox.empty());
    }
    #[test]
    fn failures_remove_spills_and_propagate_output_errors() {
        let sandbox = Sandbox::new();
        let compare = |a: &[u8], b: &[u8]| a.cmp(b);
        let mut sorter = Sorter::new(sandbox.config(1), &compare);
        for row in [b"a", b"b", b"c"] {
            sorter.push(row.to_vec()).unwrap();
        }
        assert_eq!(
            sorter
                .finish(&mut |_| Err(io::ErrorKind::BrokenPipe.into()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        assert!(sandbox.empty());
        let file = sandbox.0.join("regular");
        fs::write(&file, b"unchanged").unwrap();
        let mut sorter = Sorter::new(
            Config {
                temporary_dirs: vec![file.clone()],
                ..sandbox.config(1)
            },
            &compare,
        );
        assert!(sorter.push(b"a".to_vec()).is_err());
        assert_eq!(fs::read(file).unwrap(), b"unchanged");
    }
    #[test]
    fn no_clobber_publish_keeps_existing_files_and_cleans_the_staging_name() {
        let sandbox = Sandbox::new();
        let destination = sandbox.0.join("destination");
        fs::write(&destination, b"original").unwrap();
        let mut temporary = TemporaryFile::new(&sandbox.0).unwrap();
        let staging = temporary.path().to_owned();
        temporary.file().write_all(b"replacement").unwrap();
        assert!(temporary.persist_noclobber(&destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        assert!(!staging.exists());
        let destination = sandbox.0.join("new");
        let mut temporary = TemporaryFile::new(&sandbox.0).unwrap();
        let staging = temporary.path().to_owned();
        temporary.file().write_all(b"new contents").unwrap();
        temporary.persist_noclobber(&destination).unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"new contents");
        assert!(!staging.exists());
    }
    #[cfg(windows)]
    #[test]
    fn read_only_staging_files_are_removed_after_drop_and_failed_publish() {
        let sandbox = Sandbox::new();
        for publish in [false, true] {
            let mut temporary = TemporaryFile::new(&sandbox.0).unwrap();
            let staging = temporary.path().to_owned();
            temporary.file().write_all(b"private data").unwrap();
            let mut permissions = temporary.file().metadata().unwrap().permissions();
            permissions.set_readonly(true);
            temporary.file().set_permissions(permissions).unwrap();
            let destination = sandbox.0.join("destination");
            if publish {
                fs::write(&destination, b"original").unwrap();
                assert!(temporary.persist_noclobber(&destination).is_err());
                assert_eq!(fs::read(&destination).unwrap(), b"original");
            } else {
                drop(temporary);
            }
            assert!(!staging.exists());
        }
    }
}
