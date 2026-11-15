use super::{
    format,
    options::{Mode, Options},
    path::{self, PathError},
    select::{self, Selections},
};
use crate::common::external::TemporaryFile;
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

#[cfg(unix)]
fn apply_mode(file: &mut fs::File, parent: &Path, mode: u32, preserve: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = if preserve {
        fs::Permissions::from_mode(mode & 0o7777)
    } else {
        let mut probe = TemporaryFile::with_mode(parent, mode & 0o777)?;
        probe.file().metadata()?.permissions()
    };
    file.set_permissions(permissions)
}
#[cfg(not(unix))]
fn apply_mode(file: &mut fs::File, _: &Path, mode: u32, _: bool) -> io::Result<()> {
    let mut permissions = file.metadata()?.permissions();
    permissions.set_readonly(mode & 0o222 == 0);
    file.set_permissions(permissions)
}
fn extract_file(entry: &mut tar::Entry<'_, impl Read>, dest: &Path, o: &Options) -> io::Result<()> {
    let parent = dest.parent().unwrap_or(Path::new("."));
    let mut temp = TemporaryFile::new(parent)?;
    let expected = entry.size();
    let copied = io::copy(&mut *entry, temp.file())?;
    if copied != expected {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "unexpected end of archive member",
        ));
    }
    apply_mode(
        temp.file(),
        parent,
        entry.header().mode().unwrap_or(0o644),
        o.preserve,
    )?;
    let mtime = entry.header().mtime().unwrap_or(0).min(i64::MAX as u64) as i64;
    filetime::set_file_handle_times(
        temp.file(),
        None,
        Some(filetime::FileTime::from_unix_time(mtime, 0)),
    )?;
    if o.keep {
        temp.persist_noclobber(dest)
    } else {
        temp.persist(dest)
    }
}
fn install_link(
    dest: &Path,
    keep: bool,
    create: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<()> {
    if keep {
        return create(dest);
    }
    let directory = dest.parent().unwrap_or(Path::new("."));
    let temp = TemporaryFile::new(directory)?;
    let path = temp.path().to_path_buf();
    drop(temp);
    create(&path)?;
    let result = fs::rename(&path, dest);
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result
}
#[cfg(unix)]
fn symlink(target: &Path, dest: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, dest)
}
#[cfg(windows)]
fn symlink(target: &Path, dest: &Path) -> io::Result<()> {
    let result = if dest
        .parent()
        .unwrap_or(Path::new("."))
        .join(target)
        .is_dir()
    {
        std::os::windows::fs::symlink_dir(target, dest)
    } else {
        std::os::windows::fs::symlink_file(target, dest)
    };
    result.map_err(|e| {
        io::Error::other(format!(
            "{} (creating symbolic links on Windows requires Developer Mode or an elevated shell)",
            crate::common::error::io_text(&e)
        ))
    })
}
#[cfg(not(any(unix, windows)))]
fn symlink(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}
fn extract_link(
    entry: &tar::Entry<'_, impl Read>,
    dest: &Path,
    base: &Path,
    o: &Options,
) -> io::Result<()> {
    let link = entry
        .link_name()?
        .ok_or_else(|| io::Error::other("missing link target"))?;
    if entry.header().entry_type().is_symlink() {
        return install_link(dest, o.keep, |path| symlink(&link, path));
    }
    let (safe, _) = path::member_path(&link, o.absolute, o.strip)
        .map_err(|_| io::Error::other("unsafe hard link target"))?;
    let source = base.join(safe);
    path::checked_existing_parent(&source)?;
    if path::link_or_reparse(&fs::symlink_metadata(&source)?) {
        return Err(io::Error::other("hard link target is a symbolic link"));
    }
    if crate::common::meta::same_file(&source, dest).unwrap_or(false) {
        return Ok(());
    }
    install_link(dest, o.keep, |path| fs::hard_link(source, path))
}
fn extract_entry(
    entry: &mut tar::Entry<'_, impl Read>,
    dest: &Path,
    base: &Path,
    o: &Options,
) -> io::Result<()> {
    path::checked_parent(dest)?;
    let kind = entry.header().entry_type();
    if kind.is_dir() {
        match fs::symlink_metadata(dest) {
            Ok(meta) if path::link_or_reparse(&meta) => {
                Err(io::Error::other("refusing to traverse a symbolic link"))
            }
            Ok(meta) if meta.is_dir() => Ok(()),
            Ok(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => fs::create_dir(dest),
            Err(e) => Err(e),
        }
    } else if kind.is_file() || kind.is_contiguous() {
        extract_file(entry, dest, o)
    } else if kind.is_symlink() || kind.is_hard_link() {
        extract_link(entry, dest, base, o)
    } else {
        Err(io::Error::other("unsupported archive member type"))
    }
}
struct DirectoryInfo {
    path: std::path::PathBuf,
    mode: u32,
    mtime: i64,
}
fn finish_directory(info: &DirectoryInfo, preserve: bool) -> io::Result<()> {
    let meta = fs::symlink_metadata(&info.path)?;
    if path::link_or_reparse(&meta) || !meta.is_dir() {
        return Err(io::Error::other("directory changed while extracting"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = if preserve {
            fs::Permissions::from_mode(info.mode & 0o7777)
        } else {
            let mut probe = TemporaryFile::with_mode(&info.path, info.mode & 0o777)?;
            probe.file().metadata()?.permissions()
        };
        fs::set_permissions(&info.path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = (preserve, info.mode);
    }
    filetime::set_file_mtime(
        &info.path,
        filetime::FileTime::from_unix_time(info.mtime, 0),
    )
}
struct State {
    failed: bool,
    warned: bool,
    directories: Vec<DirectoryInfo>,
    matched: Vec<bool>,
}
fn extract_selected(
    mut entry: tar::Entry<'_, impl Read>,
    base: &Path,
    name: &Path,
    o: &Options,
    out: &mut dyn Write,
    state: &mut State,
) -> io::Result<()> {
    let (safe, absolute) = match path::member_path(name, o.absolute, o.strip) {
        Ok(value) => value,
        Err(PathError::Empty) => return Ok(()),
        Err(PathError::Parent) => {
            eprintln!("tar: {}: Member name contains '..'", name.display());
            state.failed = true;
            return Ok(());
        }
    };
    if absolute && !state.warned {
        eprintln!("tar: Removing leading '/' from member names");
        state.warned = true;
    }
    if o.to_stdout {
        return write_stdout(&mut entry, out);
    }
    let resolved_base = fs::canonicalize(base)?;
    let dest = resolved_base.join(safe);
    if let Err(e) = extract_entry(&mut entry, &dest, &resolved_base, o) {
        eprintln!(
            "tar: {}: {}",
            name.display(),
            crate::common::error::io_text(&e)
        );
        state.failed = true;
        return Ok(());
    }
    if entry.header().entry_type().is_dir() {
        record_directory(state, dest, entry.header());
    }
    if o.verbose {
        super::format::write_name(out, name)?;
        writeln!(out)?;
    }
    Ok(())
}
fn write_stdout(entry: &mut tar::Entry<'_, impl Read>, out: &mut dyn Write) -> io::Result<()> {
    if entry.header().entry_type().is_file() {
        let expected = entry.size();
        if io::copy(entry, out)? != expected {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected end of archive member",
            ));
        }
    }
    Ok(())
}
fn record_directory(state: &mut State, path: std::path::PathBuf, header: &tar::Header) {
    state.directories.push(DirectoryInfo {
        path,
        mode: header.mode().unwrap_or(0o755),
        mtime: header.mtime().unwrap_or(0).min(i64::MAX as u64) as i64,
    });
}
fn process<R: Read>(
    reader: R,
    o: &Options,
    selections: &Selections,
    out: &mut dyn Write,
) -> io::Result<bool> {
    let mut archive = tar::Archive::new(reader);
    let mut state = State {
        failed: false,
        warned: false,
        directories: Vec::new(),
        matched: vec![false; selections.items.len()],
    };
    for entry in archive.entries()? {
        let entry = entry?;
        let name = entry.path()?.into_owned();
        if select::excluded(o, &name) {
            continue;
        }
        let Some(base) = select::selected(o, selections, &name) else {
            continue;
        };
        for (index, selection) in selections.items.iter().enumerate() {
            state.matched[index] |= select::selection_matches(o, selection, &name);
        }
        if o.mode == Mode::List {
            format::list(out, &entry, o.verbose)?;
        } else {
            extract_selected(entry, base, &name, o, out, &mut state)?;
        }
    }
    for dir in state.directories.iter().rev() {
        if let Err(e) = finish_directory(dir, o.preserve) {
            eprintln!(
                "tar: {}: {}",
                dir.path.display(),
                crate::common::error::io_text(&e)
            );
            state.failed = true;
        }
    }
    state.failed |= report_missing(selections, &state.matched);
    Ok(state.failed)
}
fn report_missing(selections: &Selections, matched: &[bool]) -> bool {
    let mut failed = false;
    for (index, seen) in matched.iter().enumerate() {
        if !seen {
            eprintln!(
                "tar: {}: Not found in archive",
                selections.items[index].name.display()
            );
            failed = true;
        }
    }
    failed
}

pub fn run(o: &Options, selections: &Selections) -> io::Result<bool> {
    let source: Box<dyn Read> = if o.archive == "-" {
        Box::new(io::stdin())
    } else {
        Box::new(fs::File::open(&o.archive)?)
    };
    let mut source = io::BufReader::new(source);
    let mut prefix = Vec::new();
    while prefix.len() < 2 {
        let mut byte = [0u8; 1];
        if source.read(&mut byte)? == 0 {
            break;
        }
        prefix.push(byte[0]);
    }
    let gzip = o.gzip || prefix == [0x1f, 0x8b];
    let source = io::Cursor::new(prefix).chain(source);
    let stdout = io::stdout();
    let mut out = io::BufWriter::with_capacity(65536, stdout.lock());
    let failed = if gzip {
        let (mut decoder, _) = crate::commands::gzip::codec::decoder(source);
        let failed = process(&mut decoder, o, selections, &mut out)?;
        io::copy(&mut decoder, &mut io::sink())?;
        if decoder.trailing_garbage() {
            return Err(io::Error::other("gzip trailing garbage"));
        }
        failed
    } else {
        process(source, o, selections, &mut out)?
    };
    out.flush()?;
    Ok(failed)
}
