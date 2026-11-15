use super::{
    options::Options,
    select::{self, Selections},
};
use crate::common::{external::TemporaryFile, meta};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

struct CreateState<'a> {
    options: &'a Options,
    excluded_paths: Vec<PathBuf>,
    ancestors: HashSet<PathBuf>,
    hardlinks: HashMap<(u64, u64), PathBuf>,
    warned_absolute: bool,
}
fn archive_name(path: &Path, options: &Options, warned: &mut bool) -> PathBuf {
    if !path.is_absolute() || options.absolute {
        return path.to_path_buf();
    }
    if !*warned {
        eprintln!("tar: Removing leading '/' from member names");
        *warned = true;
    }
    path.components()
        .filter_map(|c| {
            if matches!(c, Component::RootDir | Component::Prefix(_)) {
                None
            } else {
                Some(c.as_os_str())
            }
        })
        .collect()
}
fn append<W: Write>(
    builder: &mut tar::Builder<W>,
    source: &Path,
    name: &Path,
    state: &mut CreateState<'_>,
) -> io::Result<()> {
    let absolute_source = std::path::absolute(source)?;
    if state
        .excluded_paths
        .iter()
        .any(|p| p == &absolute_source || meta::same_file(source, p).unwrap_or(false))
        || select::excluded(state.options, name)
    {
        return Ok(());
    }
    let metadata = if state.options.dereference {
        fs::metadata(source)?
    } else {
        fs::symlink_metadata(source)?
    };
    let name = archive_name(name, state.options, &mut state.warned_absolute);
    if state.options.verbose {
        verbose_name(&name, metadata.is_dir(), state.options.archive == "-")?;
    }
    let id = meta::file_identity(source, &metadata);
    if metadata.is_file() && id.nlink > 1 && id.inode != 0 {
        let key = (id.dev, id.inode);
        if let Some(previous) = state.hardlinks.get(&key) {
            let mut header = tar::Header::new_gnu();
            header.set_metadata(&metadata);
            header.set_entry_type(tar::EntryType::Link);
            header.set_size(0);
            header.set_cksum();
            return builder.append_link(&mut header, &name, previous);
        }
        state.hardlinks.insert(key, name.clone());
    }
    builder.append_path_with_name(source, &name)?;
    if metadata.is_dir() {
        append_directory(builder, source, &name, state)?;
    }
    Ok(())
}
fn verbose_name(name: &Path, directory: bool, stderr: bool) -> io::Result<()> {
    let mut out: Box<dyn Write> = if stderr {
        Box::new(io::stderr())
    } else {
        Box::new(io::stdout())
    };
    super::format::write_name(&mut out, name)?;
    if directory {
        out.write_all(b"/")?;
    }
    out.write_all(b"\n")
}
fn append_directory<W: Write>(
    builder: &mut tar::Builder<W>,
    source: &Path,
    name: &Path,
    state: &mut CreateState<'_>,
) -> io::Result<()> {
    let canonical = fs::canonicalize(source)?;
    if !state.ancestors.insert(canonical.clone()) {
        return Err(io::Error::other("directory cycle detected"));
    }
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        append(builder, &entry.path(), &name.join(entry.file_name()), state)?;
    }
    state.ancestors.remove(&canonical);
    Ok(())
}
fn write_archive<W: Write>(
    writer: W,
    o: &Options,
    selections: &Selections,
    excluded_paths: Vec<PathBuf>,
) -> io::Result<W> {
    let mut builder = tar::Builder::new(writer);
    builder.follow_symlinks(o.dereference);
    builder.preserve_absolute(o.absolute);
    let mut state = CreateState {
        options: o,
        excluded_paths,
        ancestors: HashSet::new(),
        hardlinks: HashMap::new(),
        warned_absolute: false,
    };
    for selected in &selections.items {
        append(
            &mut builder,
            &selected.directory.join(&selected.name),
            &selected.name,
            &mut state,
        )?;
    }
    builder.finish()?;
    builder.into_inner()
}
fn content<W: Write>(
    writer: W,
    o: &Options,
    selections: &Selections,
    excluded: Vec<PathBuf>,
) -> io::Result<()> {
    if o.gzip {
        let encoder = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
        write_archive(encoder, o, selections, excluded)?.finish()?;
    } else {
        write_archive(writer, o, selections, excluded)?;
    }
    Ok(())
}
pub fn run(o: &Options, selections: &Selections) -> io::Result<()> {
    if selections.items.is_empty() {
        return Err(io::Error::other(
            "Cowardly refusing to create an empty archive",
        ));
    }
    if o.archive == "-" {
        let stdout = io::stdout();
        let mut writer = io::BufWriter::with_capacity(65536, stdout.lock());
        content(&mut writer, o, selections, Vec::new())?;
        return writer.flush();
    }
    let path = Path::new(&o.archive);
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = TemporaryFile::new(directory)?;
    let excluded = vec![
        std::path::absolute(path)?,
        std::path::absolute(temp.path())?,
    ];
    content(temp.file(), o, selections, excluded)?;
    temp.persist(path)
}
