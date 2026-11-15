use super::transfer;
use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, io,
};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};
const HELP: &str = "Usage: ln [OPTION]... TARGET... LINK_NAME\nCreate hard or symbolic links.\n  -b, --backup[=CONTROL]  back up destination\n  -f, --force             remove existing destination\n  -i, --interactive       prompt before replacing destination\n  -L, --logical           dereference source symlinks for hard links\n  -n, --no-dereference    treat a destination directory symlink as a file\n  -P, --physical          link the source symlink itself\n  -r, --relative          use relative symbolic link target\n  -s, --symbolic          create symbolic links\n  -S, --suffix=SUFFIX     override backup suffix\n  -t, --target-directory=DIR  create links in DIR\n  -T, --no-target-directory   treat LINK_NAME as a normal file\n  -v, --verbose           print link names\n      --help              display this help\n      --version           output version information\n";

fn relative(target: &Path, parent: &Path) -> std::io::Result<PathBuf> {
    let target = fs::canonicalize(target).or_else(|_| std::path::absolute(target))?;
    let parent = fs::canonicalize(parent).or_else(|_| std::path::absolute(parent))?;
    let a: Vec<_> = target.components().collect();
    let b: Vec<_> = parent.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return Ok(target);
    }
    let mut result = PathBuf::new();
    for c in &b[common..] {
        if matches!(c, Component::Normal(_)) {
            result.push("..");
        }
    }
    for c in &a[common..] {
        result.push(c.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    Ok(result)
}
struct Options {
    symbolic: bool,
    relative: bool,
    logical: bool,
    force: bool,
    interactive: bool,
    backup: Option<String>,
    suffix: String,
    verbose: bool,
}
fn make_link(source: &Path, dest: &Path, options: &Options) -> std::io::Result<bool> {
    if transfer::exists(dest) {
        if source == dest
            || (!options.symbolic && crate::common::meta::same_file(source, dest).unwrap_or(false))
        {
            return Err(std::io::Error::other(
                "source and destination are the same file",
            ));
        }
        if options.interactive
            && !transfer::prompt(&format!("ln: replace '{}'? ", super::display(dest)))
        {
            return Ok(false);
        }
        if let Some(control) = options
            .backup
            .as_ref()
            .filter(|value| !matches!(value.as_str(), "none" | "off"))
        {
            let backup = transfer::backup_destination(dest, control, &options.suffix);
            let aliases_source = backup == source
                || (transfer::exists(&backup)
                    && crate::common::meta::same_file(source, &backup).unwrap_or(false));
            if aliases_source && !options.symbolic {
                return link_over_source_backup(source, dest, control, options);
            }
            transfer::backup(dest, source, control, &options.suffix)?;
        } else if options.force || options.interactive {
            transfer::remove(dest)?;
        } else {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
        }
    }
    if options.symbolic {
        let target = if options.relative {
            relative(source, dest.parent().unwrap_or(Path::new(".")))?
        } else {
            source.to_owned()
        };
        let directory = if target.is_absolute() {
            target.is_dir()
        } else {
            dest.parent()
                .unwrap_or(Path::new("."))
                .join(&target)
                .is_dir()
        };
        transfer::symlink(&target, dest, directory)?;
    } else {
        let source = if options.logical {
            fs::canonicalize(source)?
        } else {
            source.to_owned()
        };
        fs::hard_link(source, dest)?;
    }
    Ok(true)
}

fn link_over_source_backup(
    source: &Path,
    dest: &Path,
    control: &str,
    options: &Options,
) -> std::io::Result<bool> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let source = if options.logical {
        fs::canonicalize(source)?
    } else {
        source.to_owned()
    };
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = None;
    for _ in 0..128 {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".agentcmd-link-{}-{sequence}", std::process::id()));
        match fs::hard_link(&source, &path) {
            Ok(()) => {
                temporary = Some(path);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    let temporary =
        temporary.ok_or_else(|| std::io::Error::other("cannot create temporary hard link"))?;
    let result = transfer::backup(dest, dest, control, &options.suffix)
        .and_then(|()| fs::hard_link(&temporary, dest));
    let cleanup = fs::remove_file(&temporary);
    result.and(cleanup).map(|()| true)
}
pub fn run(argv: &[String]) -> i32 {
    let short = |id, short| OptSpec {
        id,
        short,
        long: "",
        has_arg: HasArg::No,
    };
    let specs = [
        OptSpec::optional("backup", '\0'),
        short("backup-short", 'b'),
        OptSpec::flag("force", 'f'),
        OptSpec::flag("interactive", 'i'),
        OptSpec::flag("logical", 'L'),
        OptSpec::flag("no-dereference", 'n'),
        OptSpec::flag("physical", 'P'),
        OptSpec::flag("relative", 'r'),
        OptSpec::flag("symbolic", 's'),
        OptSpec::value("suffix", 'S'),
        OptSpec::value("target-directory", 't'),
        OptSpec::flag("no-target-directory", 'T'),
        OptSpec::flag("verbose", 'v'),
        OptSpec::flag("directory", 'd'),
        short("directory", 'F'),
    ];
    let items = match args::parse_command("ln", argv, HELP, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut options = Options {
        symbolic: false,
        relative: false,
        logical: false,
        force: false,
        interactive: false,
        backup: None,
        suffix: transfer::default_suffix(),
        verbose: false,
    };
    let (mut target, mut no_target, mut no_deref, mut operands) = (None, false, false, Vec::new());
    for item in items {
        match item {
            Item::Operand(v) => operands.push(v),
            Item::Opt("symbolic", _) => options.symbolic = true,
            Item::Opt("relative", _) => options.relative = true,
            Item::Opt("logical", _) => options.logical = true,
            Item::Opt("physical", _) => options.logical = false,
            Item::Opt("force", _) => {
                options.force = true;
                options.interactive = false;
            }
            Item::Opt("interactive", _) => {
                options.interactive = true;
                options.force = false;
            }
            Item::Opt("backup-short", _) => options.backup = Some(transfer::default_backup()),
            Item::Opt("backup", v) => {
                options.backup = Some(v.unwrap_or_else(transfer::default_backup))
            }
            Item::Opt("suffix", Some(v)) => {
                options.suffix = v;
                options.backup.get_or_insert_with(transfer::default_backup);
            }
            Item::Opt("target-directory", v) => target = v,
            Item::Opt("no-target-directory", _) => no_target = true,
            Item::Opt("no-dereference", _) => no_deref = true,
            Item::Opt("verbose", _) => options.verbose = true,
            Item::Opt("directory", _) => {
                return error::usage("ln", "option '--directory' is not supported by agentcmd", 1)
            }
            _ => unreachable!(),
        }
    }
    if options.relative && !options.symbolic {
        return error::usage("ln", "cannot do --relative without --symbolic", 1);
    }
    if options.backup.as_deref().is_some_and(|v| {
        ![
            "none", "off", "numbered", "t", "existing", "nil", "simple", "never",
        ]
        .contains(&v)
    }) {
        return error::usage("ln", "invalid backup type", 1);
    }
    if operands.is_empty() {
        return error::usage("ln", "missing file operand", 1);
    }
    if target.is_some() && no_target {
        return error::usage(
            "ln",
            "cannot combine --target-directory and --no-target-directory",
            1,
        );
    }
    let explicit_target_directory = target.is_some();
    let dest = if let Some(target) = target {
        PathBuf::from(target)
    } else if operands.len() == 1 {
        PathBuf::from(".")
    } else {
        PathBuf::from(operands.pop().unwrap())
    };
    if explicit_target_directory && !dest.is_dir() {
        return error::usage(
            "ln",
            format!("target '{}' is not a directory", super::display(&dest)),
            1,
        );
    }
    let dest_is_dir = explicit_target_directory
        || !no_target
            && dest.is_dir()
            && !(no_deref && fs::symlink_metadata(&dest).is_ok_and(|m| m.file_type().is_symlink()));
    if operands.len() > 1 && !dest_is_dir {
        return error::usage(
            "ln",
            format!("target '{}' is not a directory", super::display(&dest)),
            1,
        );
    }
    let mut failed = false;
    let status = io::output("ln", |out| {
        for source in &operands {
            let source = Path::new(source);
            let dest = match transfer::destination(source, &dest, dest_is_dir) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("ln: {}: {}", super::display(source), error::io_text(&e));
                    failed = true;
                    continue;
                }
            };
            match make_link(source, &dest, &options) {
                Ok(true) if options.verbose => writeln!(
                    out,
                    "'{}' {} '{}'",
                    super::display(&dest),
                    if options.symbolic { "->" } else { "=>" },
                    super::display(source)
                )?,
                Ok(true) => {}
                Ok(false) => failed = true,
                Err(e) => {
                    eprintln!(
                        "ln: failed to create {} link '{}': {}{}",
                        if options.symbolic { "symbolic" } else { "hard" },
                        super::display(&dest),
                        if options.symbolic {
                            transfer::symlink_error_text(&e)
                        } else {
                            error::io_text(&e)
                        },
                        if options.symbolic {
                            transfer::symlink_diagnostic(&e)
                        } else {
                            ""
                        }
                    );
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
    fn opts() -> Options {
        Options {
            symbolic: false,
            relative: false,
            logical: false,
            force: false,
            interactive: false,
            backup: None,
            suffix: transfer::default_suffix(),
            verbose: false,
        }
    }
    #[test]
    fn hard_link() {
        let s = Sandbox::new();
        s.write("a", b"x");
        assert!(make_link(&s.path("a"), &s.path("b"), &opts()).unwrap());
        assert_eq!(fs::read(s.path("b")).unwrap(), b"x");
    }
    #[test]
    fn refuses_existing() {
        let s = Sandbox::new();
        s.write("a", b"a");
        s.write("b", b"b");
        assert!(make_link(&s.path("a"), &s.path("b"), &opts()).is_err());
        assert_eq!(fs::read(s.path("b")).unwrap(), b"b");
    }
    #[test]
    fn force_replaces() {
        let s = Sandbox::new();
        s.write("a", b"a");
        s.write("b", b"b");
        let mut o = opts();
        o.force = true;
        assert!(make_link(&s.path("a"), &s.path("b"), &o).unwrap());
        assert_eq!(fs::read(s.path("b")).unwrap(), b"a");
    }
    #[test]
    fn force_same_file_is_safe() {
        let s = Sandbox::new();
        s.write("a", b"a");
        let mut o = opts();
        o.force = true;
        assert!(make_link(&s.path("a"), &s.path("a"), &o).is_err());
        assert!(s.exists("a"));
    }
    #[test]
    fn relative_path_to_sibling() {
        let s = Sandbox::new();
        s.write("a/f", b"x");
        fs::create_dir(s.path("b")).unwrap();
        assert_eq!(
            relative(&s.path("a/f"), &s.path("b")).unwrap(),
            PathBuf::from("..").join("a/f")
        );
    }
}
