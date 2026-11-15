use super::{
    cp::{self, CopyOptions},
    transfer,
};
use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, io, meta,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
const HELP: &str = "Usage: mv [OPTION]... SOURCE... DEST\nRename SOURCE to DEST, or move sources to a directory.\n  -b, --backup[=CONTROL]  back up existing destinations\n  -f, --force             do not prompt before overwriting\n  -i, --interactive       prompt before overwriting\n  -n, --no-clobber        do not overwrite\n      --strip-trailing-slashes  remove trailing slashes from sources\n  -S, --suffix=SUFFIX     override backup suffix\n  -t, --target-directory=DIR  move all sources into DIR\n  -T, --no-target-directory   treat DEST as a normal file\n  -u, --update[=WHEN]     control replacement of existing files\n  -v, --verbose           explain what is being done\n      --no-copy           do not fall back to copying across devices\n      --help              display this help\n      --version           output version information\n";
#[derive(Clone, Copy)]
enum Overwrite {
    All,
    Never,
    NoClobber,
    NeverFail,
    Older,
    Ask,
}
struct Options {
    overwrite: Overwrite,
    backup: Option<String>,
    suffix: String,
    no_copy: bool,
    verbose: bool,
}
fn relocate(
    source: &Path,
    dest: &Path,
    opts: &Options,
    out: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let source_meta = fs::symlink_metadata(source)?;
    if transfer::exists(dest) {
        let case_only = cfg!(windows)
            && source != dest
            && source
                .to_string_lossy()
                .eq_ignore_ascii_case(&dest.to_string_lossy());
        if !case_only && meta::same_file(source, dest).unwrap_or(false) {
            return Err(std::io::Error::other(format!(
                "'{}' and '{}' are the same file",
                super::display(source),
                super::display(dest)
            )));
        }
        match opts.overwrite {
            Overwrite::Never => return Ok(()),
            Overwrite::NoClobber => {
                return Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
            }
            Overwrite::NeverFail => {
                return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
            }
            Overwrite::Older => {
                if let Ok(dest_meta) = fs::metadata(dest) {
                    if source_meta.modified()? <= dest_meta.modified()? {
                        return Ok(());
                    }
                }
            }
            Overwrite::Ask => {
                if !transfer::prompt(&format!("mv: overwrite '{}'? ", super::display(dest))) {
                    return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                }
            }
            Overwrite::All => {}
        }
        if let Some(control) = opts
            .backup
            .as_ref()
            .filter(|value| !matches!(value.as_str(), "none" | "off"))
        {
            transfer::backup(dest, source, control, &opts.suffix)?;
        } else if source_meta.is_dir() && dest.is_dir() && fs::read_dir(dest)?.next().is_some() {
            return Err(std::io::Error::from(std::io::ErrorKind::DirectoryNotEmpty));
        }
    }
    match fs::rename(source, dest) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices && !opts.no_copy => {
            cp::copy_tree(
                source,
                dest,
                &CopyOptions::move_fallback(),
                &mut std::io::sink(),
            )?;
            if source_meta.is_dir() {
                fs::remove_dir_all(source)?;
            } else {
                transfer::remove(source)?;
            }
        }
        Err(e) => return Err(e),
    }
    if opts.verbose {
        writeln!(
            out,
            "renamed '{}' -> '{}'",
            super::display(source),
            super::display(dest)
        )?;
    }
    Ok(())
}
pub fn run(argv: &[String]) -> i32 {
    let flag = |id, short| OptSpec {
        id,
        short,
        long: "",
        has_arg: HasArg::No,
    };
    let specs = [
        flag("backup-short", 'b'),
        OptSpec::optional("backup", '\0'),
        OptSpec::flag("force", 'f'),
        OptSpec::flag("interactive", 'i'),
        OptSpec::flag("no-clobber", 'n'),
        OptSpec::flag("strip-trailing-slashes", '\0'),
        OptSpec::value("suffix", 'S'),
        OptSpec::value("target-directory", 't'),
        OptSpec::flag("no-target-directory", 'T'),
        flag("update-short", 'u'),
        OptSpec::optional("update", '\0'),
        OptSpec::flag("verbose", 'v'),
        OptSpec::flag("no-copy", '\0'),
        OptSpec::flag("context", 'Z'),
        OptSpec::flag("debug", '\0'),
    ];
    let items = match args::parse_command("mv", argv, HELP, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut options = Options {
        overwrite: Overwrite::All,
        backup: None,
        suffix: transfer::default_suffix(),
        no_copy: false,
        verbose: false,
    };
    let (mut strip, mut target, mut no_target, mut operands) = (false, None, false, Vec::new());
    for item in items {
        match item {
            Item::Operand(v) => operands.push(v),
            Item::Opt("backup-short", _) => options.backup = Some(transfer::default_backup()),
            Item::Opt("backup", v) => {
                options.backup = Some(v.unwrap_or_else(transfer::default_backup))
            }
            Item::Opt("force", _) => options.overwrite = Overwrite::All,
            Item::Opt("interactive", _) => options.overwrite = Overwrite::Ask,
            Item::Opt("no-clobber", _) => options.overwrite = Overwrite::NoClobber,
            Item::Opt("strip-trailing-slashes", _) => strip = true,
            Item::Opt("suffix", Some(v)) => {
                options.suffix = v;
                options.backup.get_or_insert_with(transfer::default_backup);
            }
            Item::Opt("target-directory", v) => target = v,
            Item::Opt("no-target-directory", _) => no_target = true,
            Item::Opt("update-short", _) => options.overwrite = Overwrite::Older,
            Item::Opt("update", v) => {
                options.overwrite = match v.as_deref().unwrap_or("older") {
                    "all" => Overwrite::All,
                    "none" => Overwrite::Never,
                    "none-fail" => Overwrite::NeverFail,
                    "older" => Overwrite::Older,
                    v => {
                        return error::usage(
                            "mv",
                            format!("invalid argument '{v}' for '--update'"),
                            1,
                        )
                    }
                }
            }
            Item::Opt("verbose", _) => options.verbose = true,
            Item::Opt("no-copy", _) => options.no_copy = true,
            Item::Opt(id @ ("context" | "debug"), _) => {
                return error::usage(
                    "mv",
                    format!("option '--{id}' is not supported by agentcmd"),
                    1,
                )
            }
            _ => unreachable!(),
        }
    }
    if options.backup.as_deref().is_some_and(|v| {
        ![
            "none", "off", "numbered", "t", "existing", "nil", "simple", "never",
        ]
        .contains(&v)
    }) {
        return error::usage("mv", "invalid backup type", 1);
    }
    if target.is_some() && no_target {
        return error::usage(
            "mv",
            "cannot combine --target-directory and --no-target-directory",
            1,
        );
    }
    if operands.is_empty() {
        return error::usage("mv", "missing file operand", 1);
    }
    let explicit_target_directory = target.is_some();
    let dest = if let Some(target) = target {
        PathBuf::from(target)
    } else if operands.len() < 2 {
        return error::usage("mv", "missing destination file operand", 1);
    } else {
        PathBuf::from(operands.pop().unwrap())
    };
    if explicit_target_directory && !dest.is_dir() {
        return error::usage(
            "mv",
            format!("target '{}' is not a directory", super::display(&dest)),
            1,
        );
    }
    let dest_is_dir = !no_target && dest.is_dir();
    if operands.len() > 1 && !dest_is_dir {
        return error::usage(
            "mv",
            format!("target '{}' is not a directory", super::display(&dest)),
            1,
        );
    }
    let mut failed = false;
    let status = io::output("mv", |out| {
        for source in &operands {
            let source = if strip && source.len() > 1 {
                source.trim_end_matches(['/', '\\'])
            } else {
                source
            };
            let source = Path::new(source);
            let target = match transfer::destination(source, &dest, dest_is_dir) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("mv: {}", error::io_text(&e));
                    failed = true;
                    continue;
                }
            };
            match relocate(source, &target, &options, out) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => return Err(e),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => failed = true,
                Err(e) => {
                    eprintln!(
                        "mv: cannot move '{}' to '{}': {}",
                        super::display(source),
                        super::display(&target),
                        error::io_text(&e)
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
            overwrite: Overwrite::All,
            backup: None,
            suffix: transfer::default_suffix(),
            no_copy: false,
            verbose: false,
        }
    }
    #[test]
    fn moves_file() {
        let s = Sandbox::new();
        s.write("a", b"x");
        relocate(&s.path("a"), &s.path("b"), &opts(), &mut std::io::sink()).unwrap();
        assert!(!s.exists("a"));
        assert_eq!(fs::read(s.path("b")).unwrap(), b"x");
    }
    #[test]
    fn missing_source() {
        let s = Sandbox::new();
        assert!(relocate(&s.path("a"), &s.path("b"), &opts(), &mut std::io::sink()).is_err());
    }
    #[test]
    fn same_file() {
        let s = Sandbox::new();
        s.write("a", b"x");
        assert!(relocate(&s.path("a"), &s.path("a"), &opts(), &mut std::io::sink()).is_err());
        assert!(s.exists("a"));
    }
    #[test]
    fn no_clobber() {
        let s = Sandbox::new();
        s.write("a", b"a");
        s.write("b", b"b");
        let mut o = opts();
        o.overwrite = Overwrite::Never;
        relocate(&s.path("a"), &s.path("b"), &o, &mut std::io::sink()).unwrap();
        assert!(s.exists("a"));
        assert_eq!(fs::read(s.path("b")).unwrap(), b"b");
    }
    #[test]
    fn backup_existing() {
        let s = Sandbox::new();
        s.write("a", b"a");
        s.write("b", b"b");
        let mut o = opts();
        o.backup = Some("simple".into());
        relocate(&s.path("a"), &s.path("b"), &o, &mut std::io::sink()).unwrap();
        assert_eq!(fs::read(s.path("b~")).unwrap(), b"b");
    }
}
