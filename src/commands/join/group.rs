use super::{Options, Record};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const GROUP_MEMORY_LIMIT: usize = 1024 * 1024;
static NEXT_SPOOL: AtomicU64 = AtomicU64::new(0);

struct Spool {
    path: PathBuf,
    writer: Option<File>,
}

impl Spool {
    fn create() -> std::io::Result<Self> {
        let dir = std::env::var_os("TMPDIR")
            .or_else(|| std::env::var_os("TEMP"))
            .or_else(|| std::env::var_os("TMP"))
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        for _ in 0..64 {
            let id = NEXT_SPOOL.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("agentcmd-join-{}-{id}.tmp", std::process::id()));
            let mut options = OpenOptions::new();
            options.write(true).read(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(writer) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Err(error) =
                            writer.set_permissions(fs::Permissions::from_mode(0o600))
                        {
                            drop(writer);
                            let _ = fs::remove_file(&path);
                            return Err(error);
                        }
                    }
                    return Ok(Self {
                        path,
                        writer: Some(writer),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create unique temporary file",
        ))
    }

    fn write(&mut self, record: &Record) -> std::io::Result<()> {
        let writer = self.writer.as_mut().unwrap();
        writer.write_all(&record.line_number.to_le_bytes())?;
        writer.write_all(&[u8::from(record.disorder)])?;
        writer.write_all(&(record.raw.len() as u64).to_le_bytes())?;
        writer.write_all(&record.raw)
    }

    fn visit(
        &self,
        options: &Options,
        side: usize,
        mut action: impl FnMut(Record) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut handle = self.writer.as_ref().unwrap().try_clone()?;
        handle.seek(SeekFrom::Start(0))?;
        let mut reader = BufReader::new(handle);
        loop {
            let mut line_number = [0; 8];
            match reader.read_exact(&mut line_number) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                    if reader.fill_buf()?.is_empty() {
                        return Ok(());
                    }
                    return Err(error);
                }
                Err(error) => return Err(error),
            }
            let mut flags = [0; 1];
            let mut length = [0; 8];
            reader.read_exact(&mut flags)?;
            reader.read_exact(&mut length)?;
            let length = usize::try_from(u64::from_le_bytes(length)).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "spooled record too large")
            })?;
            let mut raw = vec![0; length];
            reader.read_exact(&mut raw)?;
            let mut record = Record::new(raw, u64::from_le_bytes(line_number), options, side);
            record.disorder = flags[0] != 0;
            action(record)?;
        }
    }
}

impl Drop for Spool {
    fn drop(&mut self) {
        self.writer.take();
        let _ = fs::remove_file(&self.path);
    }
}

enum GroupStorage {
    Memory(Vec<Record>),
    File(Spool),
}

pub(super) struct Group {
    side: usize,
    bytes: usize,
    storage: GroupStorage,
}

impl Group {
    pub(super) fn new(side: usize) -> Self {
        Self {
            side,
            bytes: 0,
            storage: GroupStorage::Memory(Vec::new()),
        }
    }

    pub(super) fn push(&mut self, record: Record) -> std::io::Result<()> {
        let size = record.raw.len().saturating_add(17);
        self.bytes = self.bytes.saturating_add(size);
        if matches!(self.storage, GroupStorage::Memory(_)) && self.bytes > GROUP_MEMORY_LIMIT {
            let GroupStorage::Memory(records) =
                std::mem::replace(&mut self.storage, GroupStorage::Memory(Vec::new()))
            else {
                unreachable!()
            };
            let mut spool = Spool::create()?;
            for record in records {
                spool.write(&record)?;
            }
            self.storage = GroupStorage::File(spool);
        }
        match &mut self.storage {
            GroupStorage::Memory(records) => records.push(record),
            GroupStorage::File(spool) => spool.write(&record)?,
        }
        Ok(())
    }

    pub(super) fn seal(&mut self) -> std::io::Result<()> {
        if let GroupStorage::File(spool) = &mut self.storage {
            spool.writer.as_mut().unwrap().flush()?;
        }
        Ok(())
    }

    pub(super) fn visit(
        &self,
        options: &Options,
        mut action: impl FnMut(Record) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        match &self.storage {
            GroupStorage::Memory(records) => {
                for record in records {
                    action(record.clone())?;
                }
                Ok(())
            }
            GroupStorage::File(spool) => spool.visit(options, self.side, action),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::join::FieldMode;

    #[cfg(unix)]
    #[test]
    fn spill_file_is_private_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;

        let spool = Spool::create().unwrap();
        assert_eq!(
            fs::metadata(&spool.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let path = spool.path.clone();
        drop(spool);
        assert!(!path.exists());
    }

    #[test]
    fn spool_replay_rewinds_the_open_file_handle() {
        let options = Options {
            field_mode: FieldMode::Blanks,
            delimiter: b'\n',
            join_fields: [0, 0],
            only_unpaired: [false; 2],
            include_unpaired: [false; 2],
            ignore_case: false,
            empty: None,
            output: None,
            auto_output: false,
            header: false,
            check_order: None,
            files: vec![],
        };
        let mut spool = Spool::create().unwrap();
        spool
            .write(&Record::new(b"key value".to_vec(), 7, &options, 1))
            .unwrap();
        let mut rows = Vec::new();
        spool
            .visit(&options, 1, |record| {
                rows.push(record.raw);
                Ok(())
            })
            .unwrap();
        spool
            .visit(&options, 1, |record| {
                rows.push(record.raw);
                Ok(())
            })
            .unwrap();
        assert_eq!(rows, [b"key value".to_vec(), b"key value".to_vec()]);
    }
}
