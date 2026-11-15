use super::transfer;
use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, io, meta,
};
use filetime::{set_file_times, set_symlink_file_times, FileTime};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
const HELP: &str = "Usage: cp [OPTION]... SOURCE... DEST\nCopy SOURCE to DEST.\n  -a, --archive            preserve attributes and copy recursively\n  -b, --backup[=CONTROL]   make a backup of each existing destination\n  -d                       preserve links and do not dereference symlinks\n  -f, --force              remove existing destination when needed\n  -H                       follow command-line symlinks\n  -i, --interactive        prompt before overwrite\n  -l, --link               hard link files\n  -L, --dereference        always follow symlinks\n  -n, --no-clobber         do not overwrite\n  -P, --no-dereference     never follow symlinks\n  -p                       preserve mode, ownership and timestamps\n      --preserve[=ATTR]    preserve mode, ownership, timestamps or links\n      --no-preserve=ATTR   omit selected preservation\n      --parents            append source path to destination directory\n  -R, -r, --recursive      copy directories recursively\n      --remove-destination remove destination before opening it\n      --reflink[=WHEN]     auto or never\n      --sparse=WHEN        accepted\n  -s, --symbolic-link      make symbolic links\n  -S, --suffix=SUFFIX      override backup suffix\n  -t, --target-directory=DIR  copy all sources into DIR\n  -T, --no-target-directory   treat DEST as a normal file\n  -u, --update[=WHEN]      control replacement of existing files\n  -v, --verbose            print copied paths\n      --help               display this help\n      --version            output version information\n";
#[derive(Clone, Copy, PartialEq)]
enum Follow {
    Never,
    Arguments,
    Always,
}
#[derive(Clone, Copy, PartialEq)]
enum Update {
    All,
    None,
    NoneFail,
    Older,
}
#[derive(Clone)]
pub(crate) struct CopyOptions {
    recursive: bool,
    follow: Follow,
    force: bool,
    interactive: bool,
    update: Update,
    remove_dest: bool,
    hard: bool,
    symbolic: bool,
    preserve_mode: bool,
    preserve_time: bool,
    preserve_owner: bool,
    preserve_links: bool,
    backup: Option<String>,
    suffix: String,
    verbose: bool,
}
impl Default for CopyOptions {
    fn default() -> Self {
        Self {
            recursive: false,
            follow: Follow::Always,
            force: false,
            interactive: false,
            update: Update::All,
            remove_dest: false,
            hard: false,
            symbolic: false,
            preserve_mode: false,
            preserve_time: false,
            preserve_owner: false,
            preserve_links: false,
            backup: None,
            suffix: transfer::default_suffix(),
            verbose: false,
        }
    }
}
impl CopyOptions {
    pub(crate) fn move_fallback() -> Self {
        Self {
            recursive: true,
            follow: Follow::Never,
            preserve_mode: true,
            preserve_time: true,
            preserve_owner: true,
            preserve_links: true,
            ..Self::default()
        }
    }
    #[cfg(unix)]
    fn preserve_owner(meta: &fs::Metadata, dest: &Path, symlink: bool) -> std::io::Result<()> {
        use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
        let path = std::ffi::CString::new(dest.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        // SAFETY: the path is NUL-terminated, and uid/gid come from filesystem metadata.
        let result = unsafe {
            if symlink {
                libc::lchown(path.as_ptr(), meta.uid(), meta.gid())
            } else {
                libc::chown(path.as_ptr(), meta.uid(), meta.gid())
            }
        };
        if result == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(not(unix))]
    fn preserve_owner(_: &fs::Metadata, _: &Path, _: bool) -> std::io::Result<()> {
        Ok(())
    }

    fn preserve(&mut self, list: &str, enabled: bool) -> Result<(), String> {
        for word in list.split(',') {
            match word {
                "mode" => self.preserve_mode = enabled,
                "timestamps" => self.preserve_time = enabled,
                "ownership" => self.preserve_owner = enabled,
                "links" => self.preserve_links = enabled,
                "all" => {
                    self.preserve_mode = enabled;
                    self.preserve_time = enabled;
                    self.preserve_owner = enabled;
                    self.preserve_links = enabled;
                }
                _ => return Err(format!("invalid preservation attribute '{word}'")),
            }
        }
        Ok(())
    }
}
struct State {
    links: HashMap<meta::FileId, PathBuf>,
    active: HashSet<meta::FileId>,
}
impl State {
    fn new() -> Self {
        Self {
            links: HashMap::new(),
            active: HashSet::new(),
        }
    }
}
fn metadata(path: &Path, follow: bool) -> std::io::Result<fs::Metadata> {
    if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
}
fn preserve(
    src_meta: &fs::Metadata,
    dest: &Path,
    symlink: bool,
    opts: &CopyOptions,
) -> std::io::Result<()> {
    if opts.preserve_owner {
        CopyOptions::preserve_owner(src_meta, dest, symlink)?;
    }
    if opts.preserve_mode && !symlink {
        fs::set_permissions(dest, src_meta.permissions())?;
    }
    if opts.preserve_time {
        let a = FileTime::from_last_access_time(src_meta);
        let m = FileTime::from_last_modification_time(src_meta);
        if symlink {
            set_symlink_file_times(dest, a, m)?;
        } else {
            set_file_times(dest, a, m)?;
        }
    }
    Ok(())
}
fn symlink_error(error: std::io::Error) -> std::io::Error {
    let hint = transfer::symlink_diagnostic(&error);
    if hint.is_empty() {
        error
    } else {
        std::io::Error::other(format!("{}{hint}", transfer::symlink_error_text(&error)))
    }
}
fn create_symlink(target: &Path, dest: &Path, directory: bool) -> std::io::Result<()> {
    transfer::symlink(target, dest, directory).map_err(symlink_error)
}
fn copy_data(source: &Path, dest: &Path, meta: &fs::Metadata) -> std::io::Result<()> {
    let mut input = fs::File::open(source)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(meta.permissions().mode() & 0o777);
    }
    #[cfg(not(unix))]
    let _ = meta;
    let mut output = options.open(dest)?;
    std::io::copy(&mut input, &mut output)?;
    Ok(())
}
fn should_copy(source: &fs::Metadata, dest: &Path, opts: &CopyOptions) -> std::io::Result<bool> {
    if !transfer::exists(dest) {
        return Ok(true);
    }
    match opts.update {
        Update::None => return Ok(false),
        Update::NoneFail => return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists)),
        Update::Older => {
            if let Ok(m) = fs::metadata(dest) {
                if source.modified()? <= m.modified()? {
                    return Ok(false);
                }
            }
        }
        Update::All => {}
    }
    if opts.interactive && !transfer::prompt(&format!("cp: overwrite '{}'? ", super::display(dest)))
    {
        return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
    }
    Ok(true)
}
#[cfg(unix)]
fn make_directory(dest: &Path, source: &fs::Metadata) -> std::io::Result<Option<fs::Permissions>> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let source_mode = source.permissions().mode() & 0o777;
    fs::DirBuilder::new()
        .mode(source_mode | 0o700)
        .create(dest)?;
    let actual = fs::metadata(dest)?.permissions().mode();
    Ok(Some(fs::Permissions::from_mode(actual & source_mode)))
}
#[cfg(not(unix))]
fn make_directory(dest: &Path, _: &fs::Metadata) -> std::io::Result<Option<fs::Permissions>> {
    fs::create_dir(dest)?;
    Ok(None)
}
fn copy_inner(
    source: &Path,
    dest: &Path,
    opts: &CopyOptions,
    state: &mut State,
    arg: bool,
    out: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let follow = opts.follow == Follow::Always || arg && opts.follow == Follow::Arguments;
    let md = metadata(source, follow)?;
    let is_symlink = md.file_type().is_symlink();
    if md.is_dir() {
        if !opts.recursive {
            return Err(std::io::Error::other(format!(
                "-r not specified; omitting directory '{}'",
                super::display(source)
            )));
        }
        if transfer::inside(source, dest) {
            return Err(std::io::Error::other(format!(
                "cannot copy a directory, '{}', into itself, '{}'",
                super::display(source),
                super::display(dest)
            )));
        }
        let id = meta::file_identity(source, &md);
        if id.inode != 0 && !state.active.insert(id) {
            return Err(std::io::Error::other("cannot copy cyclic symbolic link"));
        }
        let created_mode = if !dest.exists() {
            make_directory(dest, &md)?
        } else if !dest.is_dir() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory));
        } else {
            None
        };
        if opts.verbose {
            writeln!(
                out,
                "'{}' -> '{}'",
                super::display(source),
                super::display(dest)
            )?;
        }
        for item in fs::read_dir(source)? {
            let item = item?;
            copy_inner(
                &item.path(),
                &dest.join(item.file_name()),
                opts,
                state,
                false,
                out,
            )?;
        }
        preserve(&md, dest, false, opts)?;
        if !opts.preserve_mode {
            if let Some(mode) = created_mode {
                fs::set_permissions(dest, mode)?;
            }
        }
        state.active.remove(&id);
        return Ok(());
    }
    if transfer::exists(dest) && meta::same_file(source, dest).unwrap_or(false) {
        return Err(std::io::Error::other(format!(
            "'{}' and '{}' are the same file",
            super::display(source),
            super::display(dest)
        )));
    }
    if !should_copy(&md, dest, opts)? {
        return Ok(());
    }
    if let Some(control) = opts
        .backup
        .as_ref()
        .filter(|value| !matches!(value.as_str(), "none" | "off"))
    {
        transfer::backup(dest, source, control, &opts.suffix)?;
    } else if transfer::exists(dest)
        && (opts.remove_dest || is_symlink || opts.hard || opts.symbolic)
    {
        transfer::remove(dest)?;
    }
    let id = meta::file_identity(source, &md);
    if opts.preserve_links && id.inode != 0 && id.nlink > 1 {
        if let Some(existing) = state.links.get(&id) {
            if transfer::exists(dest) {
                transfer::remove(dest)?;
            }
            fs::hard_link(existing, dest)?;
            return Ok(());
        }
    }
    if opts.symbolic {
        create_symlink(source, dest, source.is_dir())?;
    } else if opts.hard {
        fs::hard_link(source, dest)?;
    } else if is_symlink {
        let target = fs::read_link(source)?;
        create_symlink(&target, dest, source.is_dir())?;
    } else if let Err(e) = copy_data(source, dest, &md) {
        if opts.force && transfer::exists(dest) {
            transfer::remove(dest)?;
            copy_data(source, dest, &md)?;
        } else {
            return Err(e);
        }
    }
    if !opts.hard && !opts.symbolic {
        preserve(&md, dest, is_symlink, opts)?;
    }
    if opts.preserve_links && id.inode != 0 && id.nlink > 1 {
        state.links.insert(id, dest.to_owned());
    }
    if opts.verbose {
        writeln!(
            out,
            "'{}' -> '{}'",
            super::display(source),
            super::display(dest)
        )?;
    }
    Ok(())
}
pub(crate) fn copy_tree(
    source: &Path,
    dest: &Path,
    opts: &CopyOptions,
    out: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    copy_inner(source, dest, opts, &mut State::new(), true, out)
}

pub fn run(argv: &[String]) -> i32 {
    let flag = |id, short| OptSpec {
        id,
        short,
        long: "",
        has_arg: HasArg::No,
    };
    let specs = [
        OptSpec::flag("archive", 'a'),
        flag("backup-short", 'b'),
        OptSpec::optional("backup", '\0'),
        flag("no-deref-preserve", 'd'),
        OptSpec::flag("force", 'f'),
        flag("follow-arguments", 'H'),
        OptSpec::flag("interactive", 'i'),
        OptSpec::flag("link", 'l'),
        OptSpec::flag("dereference", 'L'),
        OptSpec::flag("no-clobber", 'n'),
        OptSpec::flag("no-dereference", 'P'),
        flag("preserve-default", 'p'),
        OptSpec::optional("preserve", '\0'),
        OptSpec::value("no-preserve", '\0'),
        OptSpec::flag("parents", '\0'),
        OptSpec::flag("recursive", 'R'),
        flag("recursive", 'r'),
        OptSpec::flag("remove-destination", '\0'),
        OptSpec::optional("reflink", '\0'),
        OptSpec::value("sparse", '\0'),
        OptSpec::flag("symbolic-link", 's'),
        OptSpec::value("suffix", 'S'),
        OptSpec::value("target-directory", 't'),
        OptSpec::flag("no-target-directory", 'T'),
        OptSpec::flag("update-short", 'u'),
        OptSpec::optional("update", '\0'),
        OptSpec::flag("verbose", 'v'),
        OptSpec::flag("attributes-only", '\0'),
        OptSpec::optional("context", 'Z'),
        OptSpec::flag("copy-contents", '\0'),
        OptSpec::flag("debug", '\0'),
        OptSpec::flag("one-file-system", 'x'),
        OptSpec::flag("strip-trailing-slashes", '\0'),
    ];
    let items = match args::parse_command("cp", argv, HELP, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let (mut options, mut target, mut no_target, mut parents, mut follow_specified, mut operands) = (
        CopyOptions::default(),
        None,
        false,
        false,
        false,
        Vec::new(),
    );
    for item in items {
        match item {
            Item::Operand(v) => operands.push(v),
            Item::Opt("archive", _) => {
                options.recursive = true;
                options.follow = Follow::Never;
                follow_specified = true;
                options.preserve("all", true).unwrap();
            }
            Item::Opt("backup-short", _) => options.backup = Some(transfer::default_backup()),
            Item::Opt("backup", v) => {
                options.backup = Some(v.unwrap_or_else(transfer::default_backup))
            }
            Item::Opt("no-deref-preserve", _) => {
                options.follow = Follow::Never;
                follow_specified = true;
                options.preserve_links = true;
            }
            Item::Opt("force", _) => options.force = true,
            Item::Opt("follow-arguments", _) => {
                options.follow = Follow::Arguments;
                follow_specified = true;
            }
            Item::Opt("interactive", _) => {
                options.interactive = true;
                options.update = Update::All;
            }
            Item::Opt("link", _) => options.hard = true,
            Item::Opt("dereference", _) => {
                options.follow = Follow::Always;
                follow_specified = true;
            }
            Item::Opt("no-clobber", _) => {
                options.update = Update::None;
                options.interactive = false;
            }
            Item::Opt("no-dereference", _) => {
                options.follow = Follow::Never;
                follow_specified = true;
            }
            Item::Opt("preserve-default", _) => {
                options.preserve_mode = true;
                options.preserve_owner = true;
                options.preserve_time = true;
            }
            Item::Opt("preserve", v) => {
                if let Err(e) =
                    options.preserve(v.as_deref().unwrap_or("mode,ownership,timestamps"), true)
                {
                    return error::usage("cp", e, 1);
                }
            }
            Item::Opt("no-preserve", Some(v)) => {
                if let Err(e) = options.preserve(&v, false) {
                    return error::usage("cp", e, 1);
                }
            }
            Item::Opt("parents", _) => parents = true,
            Item::Opt("recursive", _) => options.recursive = true,
            Item::Opt("remove-destination", _) => options.remove_dest = true,
            Item::Opt("reflink", v) => match v.as_deref().unwrap_or("always") {
                "auto" | "never" => {}
                "always" => {
                    return error::usage(
                        "cp",
                        "option '--reflink=always' is not supported by agentcmd",
                        1,
                    )
                }
                value => {
                    return error::usage(
                        "cp",
                        format!("invalid argument '{value}' for '--reflink'"),
                        1,
                    )
                }
            },
            Item::Opt("sparse", Some(v)) => {
                if !["auto", "always", "never"].contains(&v.as_str()) {
                    return error::usage("cp", format!("invalid argument '{v}' for '--sparse'"), 1);
                }
            }
            Item::Opt("symbolic-link", _) => options.symbolic = true,
            Item::Opt("suffix", Some(v)) => {
                options.suffix = v;
                options.backup.get_or_insert_with(transfer::default_backup);
            }
            Item::Opt("target-directory", v) => target = v,
            Item::Opt("no-target-directory", _) => no_target = true,
            Item::Opt("update-short", _) => options.update = Update::Older,
            Item::Opt("update", v) => {
                options.update = match v.as_deref().unwrap_or("older") {
                    "all" => Update::All,
                    "none" => Update::None,
                    "none-fail" => Update::NoneFail,
                    "older" => Update::Older,
                    v => {
                        return error::usage(
                            "cp",
                            format!("invalid argument '{v}' for '--update'"),
                            1,
                        )
                    }
                }
            }
            Item::Opt("verbose", _) => options.verbose = true,
            Item::Opt(
                id @ ("attributes-only"
                | "context"
                | "copy-contents"
                | "debug"
                | "one-file-system"
                | "strip-trailing-slashes"),
                _,
            ) => {
                return error::usage(
                    "cp",
                    format!("option '--{id}' is not supported by agentcmd"),
                    1,
                );
            }
            _ => unreachable!(),
        }
    }
    if options.recursive && !follow_specified {
        options.follow = Follow::Never;
    }
    if options.hard && options.symbolic {
        return error::usage("cp", "cannot make both hard and symbolic links", 1);
    }
    if options.backup.as_deref().is_some_and(|v| {
        ![
            "none", "off", "numbered", "t", "existing", "nil", "simple", "never",
        ]
        .contains(&v)
    }) {
        return error::usage("cp", "invalid backup type", 1);
    }
    if target.is_some() && no_target {
        return error::usage(
            "cp",
            "cannot combine --target-directory and --no-target-directory",
            1,
        );
    }
    if operands.is_empty() {
        return error::usage("cp", "missing file operand", 1);
    }
    let explicit_target_directory = target.is_some();
    let dest = if let Some(target) = target {
        PathBuf::from(target)
    } else if operands.len() < 2 {
        return error::usage("cp", "missing destination file operand", 1);
    } else {
        PathBuf::from(operands.pop().unwrap())
    };
    if explicit_target_directory && !dest.is_dir() {
        return error::usage(
            "cp",
            format!("target '{}' is not a directory", super::display(&dest)),
            1,
        );
    }
    let dest_is_dir = !no_target && dest.is_dir();
    if (operands.len() > 1 || parents) && !dest_is_dir {
        return error::usage(
            "cp",
            format!("target '{}' is not a directory", super::display(&dest)),
            1,
        );
    }
    let mut failed = false;
    let mut state = State::new();
    let status = io::output("cp", |out| {
        for source in &operands {
            let source = Path::new(source);
            let target = if parents {
                let rel: PathBuf = source
                    .components()
                    .filter_map(|c| match c {
                        std::path::Component::Normal(x) => Some(x),
                        std::path::Component::ParentDir | std::path::Component::CurDir => {
                            Some(c.as_os_str())
                        }
                        _ => None,
                    })
                    .collect();
                let target = dest.join(rel);
                if let Some(parent) = target.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        eprintln!(
                            "cp: cannot create directory '{}': {}",
                            super::display(parent),
                            error::io_text(&e)
                        );
                        failed = true;
                        continue;
                    }
                }
                target
            } else {
                match transfer::destination(source, &dest, dest_is_dir) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("cp: {}", error::io_text(&e));
                        failed = true;
                        continue;
                    }
                }
            };
            match copy_inner(source, &target, &options, &mut state, true, out) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => return Err(e),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => failed = true,
                Err(e) => {
                    eprintln!("cp: {}", error::io_text(&e));
                    failed = true;
                }
            }
        }
        Ok(())
    });
    status.max(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::Sandbox;
    use super::*;
    #[test]
    fn copies_file() {
        let s = Sandbox::new();
        s.write("a", b"abc");
        copy_tree(
            &s.path("a"),
            &s.path("b"),
            &CopyOptions::default(),
            &mut std::io::sink(),
        )
        .unwrap();
        assert_eq!(fs::read(s.path("b")).unwrap(), b"abc");
    }
    #[test]
    fn rejects_directory_without_recursive() {
        let s = Sandbox::new();
        s.write("a/f", b"x");
        assert!(copy_tree(
            &s.path("a"),
            &s.path("b"),
            &CopyOptions::default(),
            &mut std::io::sink()
        )
        .is_err());
    }
    #[test]
    fn recursively_copies() {
        let s = Sandbox::new();
        s.write("a/f", b"x");
        let o = CopyOptions {
            recursive: true,
            ..CopyOptions::default()
        };
        copy_tree(&s.path("a"), &s.path("b"), &o, &mut std::io::sink()).unwrap();
        assert_eq!(fs::read(s.path("b/f")).unwrap(), b"x");
    }
    #[test]
    fn rejects_self_copy() {
        let s = Sandbox::new();
        s.write("a", b"x");
        assert!(copy_tree(
            &s.path("a"),
            &s.path("a"),
            &CopyOptions::default(),
            &mut std::io::sink()
        )
        .is_err());
        assert_eq!(fs::read(s.path("a")).unwrap(), b"x");
    }
    #[test]
    fn rejects_recursive_copy_into_self() {
        let s = Sandbox::new();
        s.write("a/f", b"x");
        let o = CopyOptions {
            recursive: true,
            ..CopyOptions::default()
        };
        assert!(copy_tree(&s.path("a"), &s.path("a/b"), &o, &mut std::io::sink()).is_err());
    }
    #[test]
    fn no_clobber() {
        let s = Sandbox::new();
        s.write("a", b"a");
        s.write("b", b"b");
        let o = CopyOptions {
            update: Update::None,
            ..CopyOptions::default()
        };
        copy_tree(&s.path("a"), &s.path("b"), &o, &mut std::io::sink()).unwrap();
        assert_eq!(fs::read(s.path("b")).unwrap(), b"b");
    }
    #[cfg(windows)]
    #[test]
    fn symlink_permission_error_has_windows_hint() {
        let error = symlink_error(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let text = error::io_text(&error);
        assert!(text.contains("Permission denied"));
        assert!(text.contains("Developer Mode or an elevated shell"));
    }
    #[cfg(windows)]
    #[test]
    fn symlink_privilege_not_held_has_windows_hint() {
        let error = symlink_error(std::io::Error::from_raw_os_error(1314));
        assert!(error::io_text(&error).contains("Permission denied (creating symbolic links on Windows requires Developer Mode or an elevated shell)"));
    }
}
