mod apply;
mod options;
mod parse;
use crate::common::{args, error, external::TemporaryFile, io, path::display_path};
use apply::Image;
use options::Options;
use parse::{Hunk, Patch};
use std::{
    collections::HashSet,
    fs,
    io::{BufWriter, Write},
    path::{Component, Path, PathBuf},
};

fn stripped(path: &Path, count: usize) -> Result<PathBuf, String> {
    let name = display_path(path);
    if path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("unsafe filename '{name}'"));
    }
    let components: Vec<_> = path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    if components.len() <= count {
        return Err(format!("not enough path components in '{name}'"));
    }
    Ok(components[count..].iter().collect())
}
fn selection(patch: &Patch, o: &Options) -> Result<(PathBuf, PathBuf, bool, bool), String> {
    let (old, new) = if o.reverse {
        (&patch.new, &patch.old)
    } else {
        (&patch.old, &patch.new)
    };
    let creation = old == Path::new("/dev/null");
    let deletion = new == Path::new("/dev/null");
    let relative = if let Some(original) = &o.original {
        PathBuf::from(original)
    } else {
        let candidate = |name: &Path| -> Result<Option<PathBuf>, String> {
            if name == Path::new("/dev/null") {
                return Ok(None);
            }
            let checked = stripped(name, 0)?;
            let full = if let Some(n) = o.strip {
                match stripped(name, n) {
                    Ok(path) => path,
                    Err(_) => return Ok(None),
                }
            } else {
                checked
            };
            Ok(Some(if o.strip.is_some() {
                full
            } else {
                PathBuf::from(full.file_name().ok_or("missing filename")?)
            }))
        };
        let old_name = candidate(old)?;
        let new_name = candidate(new)?;
        let candidates = [old_name.as_ref(), new_name.as_ref()];
        let chosen = candidates
            .into_iter()
            .flatten()
            .find(|p| o.directory.join(p).exists())
            .or(if creation {
                new_name.as_ref()
            } else {
                old_name.as_ref().or(new_name.as_ref())
            })
            .ok_or("missing filename")?
            .clone();
        chosen
    };
    let source = o.directory.join(&relative);
    // Reject intermediate symlinks before any directory creation or output.
    let mut parent = o.directory.clone();
    let parts: Vec<_> = relative.components().collect();
    for component in parts.iter().take(parts.len().saturating_sub(1)) {
        parent.push(component.as_os_str());
        if fs::symlink_metadata(&parent)
            .ok()
            .is_some_and(|m| m.file_type().is_symlink())
        {
            return Err(format!(
                "refusing symbolic-link directory '{}'",
                display_path(&parent)
            ));
        }
    }
    if fs::symlink_metadata(&source)
        .ok()
        .is_some_and(|m| m.file_type().is_symlink())
    {
        return Err(format!(
            "refusing symbolic-link file '{}'",
            display_path(&source)
        ));
    }
    Ok((relative, source, creation, deletion))
}
struct Applied {
    rejected: Vec<Hunk>,
    mismatch: bool,
    ignored: usize,
    succeeded: usize,
}
fn execute(
    image: &mut Image,
    hunks: &[Hunk],
    o: &Options,
    out: &mut dyn Write,
    report: &mut dyn Write,
    creation_conflict: bool,
) -> std::io::Result<Applied> {
    if let Some(first) = hunks
        .first()
        .filter(|_| !o.force && (o.forward || o.batch) && !creation_conflict)
    {
        let forward = apply::locate(image, first, 0, 0, o.fuzz, o.loose)?.is_some();
        let reverse = parse::reverse(first);
        if !forward && apply::locate(image, &reverse, 0, 0, o.fuzz, o.loose)?.is_some() {
            if !o.quiet {
                writeln!(
                    report,
                    "Reversed (or previously applied) patch detected!  Skipping patch."
                )?;
            }
            image.copy(out, 0, image.len())?;
            return Ok(Applied {
                rejected: hunks.to_vec(),
                mismatch: true,
                ignored: hunks.len(),
                succeeded: 0,
            });
        }
    }
    let (mut cursor, mut offset) = (0, 0i128);
    let mut rejected = vec![];
    let mut mismatch = false;
    let mut ignored = 0;
    let mut succeeded = 0;
    for (index, hunk) in hunks.iter().enumerate() {
        if creation_conflict {
            if !o.quiet {
                writeln!(
                    report,
                    "Hunk #{} FAILED at {}.",
                    index + 1,
                    hunk.new_start.max(1)
                )?;
            }
            rejected.push(hunk.clone());
            mismatch = true;
            continue;
        }
        if let Some(found) = apply::locate(image, hunk, cursor, offset, o.fuzz, o.loose)? {
            succeeded += 1;
            image.copy(out, cursor, found.at)?;
            cursor = apply::write_hunk(image, hunk, &found, out)?;
            offset = found.offset;
            if found.offset != 0 || found.fuzz != 0 {
                mismatch = true;
                if !o.quiet {
                    let line = (hunk.new_start as i128 + found.offset).max(1);
                    write!(report, "Hunk #{} succeeded at {line}", index + 1)?;
                    if found.fuzz != 0 {
                        write!(report, " with fuzz {}", found.fuzz)?;
                    }
                    if found.offset != 0 {
                        write!(
                            report,
                            " (offset {} line{})",
                            found.offset,
                            if found.offset.abs() == 1 { "" } else { "s" }
                        )?;
                    }
                    writeln!(report, ".")?;
                }
            }
        } else {
            let reverse = parse::reverse(hunk);
            let already = !o.force
                && (o.forward || o.batch)
                && apply::locate(image, &reverse, cursor, offset, o.fuzz, o.loose)?.is_some();
            if !o.quiet {
                if already {
                    writeln!(
                        report,
                        "Reversed (or previously applied) patch detected!  Skipping patch."
                    )?;
                } else {
                    writeln!(
                        report,
                        "Hunk #{} FAILED at {}.",
                        index + 1,
                        (hunk.new_start as i128 + offset).max(1)
                    )?;
                }
            }
            if already {
                ignored += 1;
            }
            rejected.push(hunk.clone());
            mismatch = true;
        }
    }
    image.copy(out, cursor, image.len())?;
    Ok(Applied {
        rejected,
        mismatch,
        ignored,
        succeeded,
    })
}
fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}
fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}
fn backup(image: &mut Image, path: &Path) -> std::io::Result<()> {
    let mut file = TemporaryFile::new(parent(path))?;
    image.copy(file.file(), 0, image.len())?;
    if let Some(mode) = &image.permissions {
        file.file().set_permissions(mode.clone())?;
    }
    file.persist(path)
}
fn output_permissions(temporary: &mut TemporaryFile, target: &Path) -> std::io::Result<()> {
    let permissions = if let Ok(metadata) = fs::metadata(target) {
        metadata.permissions()
    } else {
        // The empty probe obtains the process umask without exposing file data.
        let probe = TemporaryFile::with_mode(parent(target), 0o666)?;
        fs::metadata(probe.path())?.permissions()
    };
    temporary.file().set_permissions(permissions)
}
#[cfg(unix)]
fn apply_mode(temporary: &mut TemporaryFile, target: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let probe = TemporaryFile::with_mode(parent(target), 0o777)?;
    let mask = fs::metadata(probe.path())?.permissions().mode();
    temporary
        .file()
        .set_permissions(fs::Permissions::from_mode(mode & mask & 0o7777))
}
#[cfg(not(unix))]
fn apply_mode(_: &mut TemporaryFile, _: &Path, _: u32) -> std::io::Result<()> {
    Ok(())
}
fn reject_file(path: &Path, patch: &Patch, hunks: &[Hunk], append: bool) -> std::io::Result<()> {
    let mut output = TemporaryFile::new(parent(path))?;
    if append {
        std::io::copy(&mut fs::File::open(path)?, output.file())?;
    }
    apply::reject(output.file(), &patch.old, &patch.new, hunks)?;
    output_permissions(&mut output, path)?;
    output.persist(path)
}
fn per_file(
    patch: &Patch,
    o: &Options,
    report: &mut dyn Write,
    merged: Option<&mut dyn Write>,
    rejected_paths: &mut HashSet<PathBuf>,
) -> std::io::Result<i32> {
    let (relative, path, creation, deletion) =
        selection(patch, o).map_err(std::io::Error::other)?;
    let name = display_path(&relative);
    let mut image = Image::open(if creation && !path.exists() {
        None
    } else {
        Some(&path)
    })?;
    let hunks: Vec<_> = patch
        .hunks
        .iter()
        .map(|h| {
            if o.reverse {
                parse::reverse(h)
            } else {
                h.clone()
            }
        })
        .collect();
    let empty_creation_exists = creation && path.exists() && image.len() == 0 && hunks.is_empty();
    if empty_creation_exists {
        if !o.quiet {
            writeln!(report, "The next patch would empty out the file {name},")?;
            writeln!(
                report,
                "which is already empty!  {}",
                if o.force {
                    "Applying it anyway."
                } else {
                    "Skipping patch."
                }
            )?;
        }
        if !o.force {
            return Ok(1);
        }
    }
    let creation_conflict = creation && path.exists() && image.len() != 0;
    if creation_conflict && !o.force {
        if !o.quiet {
            writeln!(report, "The next patch would create the file {name},")?;
            writeln!(report, "which already exists!  Skipping patch.")?;
            if !hunks.is_empty() {
                writeln!(
                    report,
                    "{} out of {} hunk{} ignored",
                    hunks.len(),
                    hunks.len(),
                    if hunks.len() == 1 { "" } else { "s" }
                )?;
            }
        }
        return Ok(1);
    }
    if creation_conflict && o.force && !o.quiet {
        writeln!(report, "The next patch would create the file {name},")?;
        writeln!(report, "which already exists!  Applying it anyway.")?;
    }
    if !o.quiet {
        let action = if o.dry { "checking" } else { "patching" };
        if let Some(output) = &o.output {
            writeln!(
                report,
                "{action} file {} (read from {name})",
                display_path(output)
            )?;
        } else {
            writeln!(report, "{action} file {name}")?;
        }
    }
    if deletion && hunks.is_empty() && image.len() != 0 && merged.is_none() {
        if !o.quiet {
            writeln!(
                report,
                "Not deleting file {name} as content differs from patch"
            )?;
        }
        return Ok(1);
    }
    if let Some(output) = merged {
        let result = execute(&mut image, &hunks, o, output, report, creation_conflict)?;
        return finish_rejects(patch, o, &relative, result, report, rejected_paths);
    }
    if !o.dry {
        fs::create_dir_all(parent(&path))?;
    }
    let directory = if o.dry && !parent(&path).exists() {
        std::env::temp_dir()
    } else {
        parent(&path).to_path_buf()
    };
    let mut temporary = TemporaryFile::new(&directory)?;
    let result = {
        let mut out = BufWriter::with_capacity(io::BUFFER_SIZE, temporary.file());
        let result = execute(&mut image, &hunks, o, &mut out, report, creation_conflict)?;
        out.flush()?;
        result
    };
    if !o.dry && !(result.ignored > 0 && result.succeeded == 0) {
        if let Some(mode) = &image.permissions {
            temporary.file().set_permissions(mode.clone())?;
        } else {
            output_permissions(&mut temporary, &path)?;
        }
        if (!creation || path.exists()) && (o.backup || result.mismatch && !o.no_mismatch_backup) {
            backup(&mut image, &suffixed(&path, ".orig"))?;
        }
        let git_mode = if o.reverse {
            patch.old_mode
        } else {
            patch.new_mode
        };
        if creation && !creation_conflict {
            if let Some(mode) = git_mode {
                apply_mode(&mut temporary, &path, mode)?;
            }
        }
        let empty = temporary.file().metadata()?.len() == 0;
        // Release the original handle before replacing/removing its directory
        // entry, including on Windows filesystems with stricter sharing rules.
        drop(image);
        if empty && (deletion || o.remove) {
            if path.exists() {
                fs::remove_file(&path)?;
            }
        } else {
            temporary.persist(&path)?;
        }
    }
    finish_rejects(patch, o, &relative, result, report, rejected_paths)
}
fn finish_rejects(
    patch: &Patch,
    o: &Options,
    name: &Path,
    result: Applied,
    report: &mut dyn Write,
    rejected_paths: &mut HashSet<PathBuf>,
) -> std::io::Result<i32> {
    if result.rejected.is_empty() {
        return Ok(0);
    }
    let display_reject = o.reject.clone().unwrap_or_else(|| {
        o.output
            .as_ref()
            .map_or_else(|| suffixed(name, ".rej"), |p| suffixed(p, ".rej"))
    });
    let discard = display_reject == Path::new("-") || display_reject == Path::new("/dev/null");
    let reject = if display_reject.is_absolute() {
        display_reject.clone()
    } else {
        o.directory.join(&display_reject)
    };
    if !o.dry && !discard {
        let append = rejected_paths.contains(&reject);
        reject_file(&reject, patch, &result.rejected, append)?;
        rejected_paths.insert(reject.clone());
    }
    if !o.quiet {
        write!(
            report,
            "{} out of {} hunk{} {}",
            result.rejected.len(),
            patch.hunks.len(),
            if patch.hunks.len() == 1 { "" } else { "s" },
            if result.ignored == result.rejected.len() {
                "ignored"
            } else {
                "FAILED"
            }
        )?;
        if !o.dry && !discard {
            writeln!(
                report,
                " -- saving rejects to file {}",
                display_path(&display_reject)
            )?;
        } else {
            writeln!(report)?;
        }
    }
    Ok(1)
}
fn run_patches(patches: &[Patch], o: &Options, report: &mut dyn Write) -> std::io::Result<i32> {
    let mut code = 0;
    let mut rejected_paths = HashSet::new();
    if let Some(path) = &o.output {
        if path == Path::new("-") {
            for patch in patches {
                code = code.max(per_file(
                    patch,
                    o,
                    &mut std::io::stderr().lock(),
                    Some(report),
                    &mut rejected_paths,
                )?);
            }
            return Ok(code);
        }
        let actual = if path.is_absolute() {
            path.clone()
        } else {
            o.directory.join(path)
        };
        let mut output = TemporaryFile::new(parent(&actual))?;
        for patch in patches {
            code = code.max(per_file(
                patch,
                o,
                report,
                Some(output.file()),
                &mut rejected_paths,
            )?);
        }
        if !o.dry {
            output_permissions(&mut output, &actual)?;
            output.persist(&actual)?;
        }
    } else {
        for patch in patches {
            code = code.max(per_file(patch, o, report, None, &mut rejected_paths)?);
        }
    }
    Ok(code)
}
pub fn run(arguments: &[String]) -> i32 {
    let (o, info) = match options::parse(arguments) {
        Ok(v) => v,
        Err(e) => return error::usage("patch", e, 2),
    };
    if let Some(info) = info {
        return args::help_version("patch", &[info], options::HELP, true).unwrap();
    }
    let input_path = if o.input == "-" || Path::new(&o.input).is_absolute() {
        PathBuf::from(&o.input)
    } else {
        o.directory.join(&o.input)
    };
    let mut input = match io::input(&input_path.to_string_lossy()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("patch: {}: {}", o.input, error::io_text(&e));
            return 2;
        }
    };
    let patches = match parse::parse(&mut input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("patch: **** {e}");
            return 2;
        }
    };
    io::output_result("patch", 2, |out| run_patches(&patches, &o, out))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strip_counts_and_unsafe_names() {
        assert_eq!(
            stripped(Path::new("a/dir/file"), 1).unwrap(),
            Path::new("dir/file")
        );
        for s in ["../file", "/file", "a/../file"] {
            assert!(stripped(Path::new(s), 0).is_err());
        }
        assert!(stripped(Path::new("file"), 1).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn staged_output_is_private_and_failed_work_keeps_original() {
        use std::{io::Write as _, os::unix::fs::PermissionsExt};
        let directory = std::env::temp_dir().join(format!("patch-staging-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let target = directory.join("file");
        std::fs::write(&target, b"original").unwrap();
        let mut temp = TemporaryFile::new(&directory).unwrap();
        temp.file().write_all(b"sensitive partial output").unwrap();
        assert_eq!(
            temp.file().metadata().unwrap().permissions().mode() & 0o077,
            0
        );
        drop(temp);
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
