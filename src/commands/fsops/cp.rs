//! `cp`: copy files and directory trees. Option handling and destination naming live here;
//! the copying itself is done by [`super::copy::Copier`].

use std::fs;
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{print_version, unsupported, EXIT_FAIL, EXIT_OK};

use super::backup::{self, Control};
use super::copy::{Copier, Deref, Options, Preserve, Update};
use super::helpers::{argmatch, print_help, ArgGroup};
use super::targets;

const HELP: &str = r#"Usage: cp [OPTION]... [-T] SOURCE DEST
  or:  cp [OPTION]... SOURCE... DIRECTORY
  or:  cp [OPTION]... -t DIRECTORY SOURCE...
Copy SOURCE to DEST, or multiple SOURCE(s) to DIRECTORY.

  -a, --archive                same as -dR --preserve=all
  -b                           like --backup but does not accept an argument
      --backup[=CONTROL]       make a backup of each existing destination file
  -d                           same as --no-dereference --preserve=links
  -f, --force                  if an existing destination file cannot be
                                 opened, remove it and try again
  -H                           follow command-line symbolic links in SOURCE
  -i, --interactive            prompt before overwrite
  -l, --link                   hard link files instead of copying
  -L, --dereference            always follow symbolic links in SOURCE
  -n, --no-clobber             do not overwrite an existing file
  -P, --no-dereference         never follow symbolic links in SOURCE
  -p                           same as --preserve=mode,ownership,timestamps
      --preserve[=ATTR_LIST]   preserve the specified attributes
      --no-preserve=ATTR_LIST  don't preserve the specified attributes
      --parents                use full source file name under DIRECTORY
  -R, -r, --recursive          copy directories recursively
      --remove-destination     remove each existing destination file before
                                 attempting to open it
      --reflink[=WHEN]         control clone/CoW copies (auto, never)
      --sparse=WHEN            control creation of sparse files (never, auto,
                                 always); accepted, has no effect
  -s, --symbolic-link          make symbolic links instead of copying
  -S, --suffix=SUFFIX         override the usual backup suffix
  -t, --target-directory=DIRECTORY  copy all SOURCE arguments into DIRECTORY
  -T, --no-target-directory    treat DEST as a normal file
  -u, --update[=WHEN]          copy only when the SOURCE file is newer than the
                                 destination file or when the destination file is
                                 missing
  -v, --verbose                explain what is being done
      --help                   display this help and exit
      --version                output version information and exit

ATTR_LIST is a comma-separated list of: mode, ownership, timestamps, links,
context, xattr, all. Without ATTR_LIST, --preserve keeps mode, ownership and
timestamps. --backup CONTROL is none (off), numbered (t), existing (nil) or
simple (never). --update WHEN is all, none, none-fail or older.
Not supported: --attributes-only, --reflink=always.
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("archive", Some('a'), Some("archive")),
    OptSpec::flag("backup", Some('b'), None),
    OptSpec::opt("backup", None, Some("backup")),
    OptSpec::flag("no-deref-links", Some('d'), None),
    OptSpec::flag("force", Some('f'), Some("force")),
    OptSpec::flag("follow-cmd", Some('H'), None),
    OptSpec::flag("interactive", Some('i'), Some("interactive")),
    OptSpec::flag("link", Some('l'), Some("link")),
    OptSpec::flag("deref", Some('L'), Some("dereference")),
    OptSpec::flag("no-clobber", Some('n'), Some("no-clobber")),
    OptSpec::flag("no-deref", Some('P'), Some("no-dereference")),
    OptSpec::flag("preserve-basic", Some('p'), None),
    OptSpec::opt("preserve", None, Some("preserve")),
    OptSpec::req("no-preserve", None, Some("no-preserve")),
    OptSpec::flag("parents", None, Some("parents")),
    OptSpec::flag("recursive", Some('R'), Some("recursive")),
    OptSpec::flag("recursive", Some('r'), None),
    OptSpec::flag("remove-dest", None, Some("remove-destination")),
    OptSpec::opt("reflink", None, Some("reflink")),
    OptSpec::req("sparse", None, Some("sparse")),
    OptSpec::flag("symbolic", Some('s'), Some("symbolic-link")),
    OptSpec::req("suffix", Some('S'), Some("suffix")),
    OptSpec::req("target", Some('t'), Some("target-directory")),
    OptSpec::flag("no-target", Some('T'), Some("no-target-directory")),
    OptSpec::flag("update-older", Some('u'), None),
    OptSpec::opt("update", None, Some("update")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("attributes-only", None, Some("attributes-only")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const UPDATE_GROUPS: &[ArgGroup<'_, Update>] = &[
    (&["all"], Update::All),
    (&["none"], Update::None),
    (&["none-fail"], Update::NoneFail),
    (&["older"], Update::Older),
];

const REFLINK_GROUPS: &[ArgGroup<'_, ReflinkMode>] = &[
    (&["always"], ReflinkMode::Always),
    (&["auto"], ReflinkMode::Auto),
    (&["never"], ReflinkMode::Never),
];

const SPARSE_GROUPS: &[ArgGroup<'_, ()>] = &[(&["never"], ()), (&["auto"], ()), (&["always"], ())];

const ATTR_GROUPS: &[ArgGroup<'_, Attr>] = &[
    (&["mode"], Attr::Mode),
    (&["ownership"], Attr::Ownership),
    (&["timestamps"], Attr::Timestamps),
    (&["links"], Attr::Links),
    (&["context"], Attr::Ignored),
    (&["xattr"], Attr::Ignored),
    (&["all"], Attr::All),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReflinkMode {
    Always,
    Auto,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Attr {
    Mode,
    Ownership,
    Timestamps,
    Links,
    Ignored,
    All,
}

/// Everything the command line selected.
struct Cli {
    recursive: bool,
    deref: Option<Deref>,
    preserve: Preserve,
    hard_link: bool,
    symbolic: bool,
    force: bool,
    remove_dest: bool,
    no_clobber: bool,
    interactive: bool,
    update: Update,
    backup: Control,
    suffix: Option<String>,
    verbose: bool,
    parents: bool,
    target_dir: Option<String>,
    no_target_dir: bool,
    operands: Vec<String>,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            recursive: false,
            deref: None,
            preserve: Preserve::default(),
            hard_link: false,
            symbolic: false,
            force: false,
            remove_dest: false,
            no_clobber: false,
            interactive: false,
            update: Update::All,
            backup: Control::None,
            suffix: None,
            verbose: false,
            parents: false,
            target_dir: None,
            no_target_dir: false,
            operands: Vec::new(),
        }
    }
}

/// Applies an `--preserve` or `--no-preserve` attribute list.
fn apply_attrs(list: &str, on: bool, preserve: &mut Preserve, opt: &str) -> Result<(), String> {
    for name in list.split(',') {
        let attr = argmatch(opt, name, ATTR_GROUPS)?;
        match attr {
            Attr::Mode => preserve.mode = on,
            Attr::Ownership => preserve.ownership = on,
            Attr::Timestamps => preserve.timestamps = on,
            Attr::Links => preserve.links = on,
            Attr::Ignored => {}
            Attr::All => {
                preserve.mode = on;
                preserve.ownership = on;
                preserve.timestamps = on;
                preserve.links = on;
            }
        }
    }
    Ok(())
}

/// Backup control for a bare `-b` or `--backup`: `VERSION_CONTROL` when set, else existing.
fn implicit_backup() -> Control {
    if std::env::var_os("VERSION_CONTROL").is_some() {
        backup::control_from_env()
    } else {
        Control::Existing
    }
}

fn usage(message: String) -> i32 {
    targets::usage("cp", message)
}

/// Turns parser output into [`Cli`]. `Err` carries the exit status for an immediate return.
fn collect(items: Vec<Item>) -> Result<Cli, i32> {
    let mut cli = Cli::default();
    for item in items {
        let outcome = match item {
            Item::Operand(text) => {
                cli.operands.push(text);
                Ok(())
            }
            Item::Opt { id, value } => handle_option(&mut cli, id, value),
        };
        outcome?;
    }
    if cli.hard_link && cli.symbolic {
        return Err(usage(
            "cannot make both hard and symbolic links".to_string(),
        ));
    }
    Ok(cli)
}

fn handle_option(cli: &mut Cli, id: &str, value: Option<String>) -> Result<(), i32> {
    match id {
        "help" => Err(print_help("cp", HELP)),
        "version" => Err(print_version("cp")),
        "attributes-only" => Err(unsupported("cp", "--attributes-only")),
        "archive" => {
            cli.recursive = true;
            cli.deref = Some(Deref::Never);
            cli.preserve = Preserve {
                mode: true,
                ownership: true,
                timestamps: true,
                links: true,
            };
            Ok(())
        }
        "backup" => {
            cli.backup = match value {
                None => implicit_backup(),
                Some(text) => backup::parse_control(&text).map_err(usage)?,
            };
            Ok(())
        }
        "no-deref-links" => {
            cli.deref = Some(Deref::Never);
            cli.preserve.links = true;
            Ok(())
        }
        "force" => {
            cli.force = true;
            Ok(())
        }
        "follow-cmd" => {
            cli.deref = Some(Deref::CommandLine);
            Ok(())
        }
        "interactive" => {
            cli.interactive = true;
            Ok(())
        }
        "link" => {
            cli.hard_link = true;
            Ok(())
        }
        "deref" => {
            cli.deref = Some(Deref::Always);
            Ok(())
        }
        "no-clobber" => {
            cli.no_clobber = true;
            Ok(())
        }
        "no-deref" => {
            cli.deref = Some(Deref::Never);
            Ok(())
        }
        "preserve-basic" => {
            cli.preserve.mode = true;
            cli.preserve.ownership = true;
            cli.preserve.timestamps = true;
            Ok(())
        }
        "preserve" => {
            let list = value.unwrap_or_else(|| "mode,ownership,timestamps".to_string());
            apply_attrs(&list, true, &mut cli.preserve, "--preserve").map_err(usage)
        }
        "no-preserve" => {
            let list = value.unwrap_or_default();
            apply_attrs(&list, false, &mut cli.preserve, "--no-preserve").map_err(usage)
        }
        "parents" => {
            cli.parents = true;
            Ok(())
        }
        "recursive" => {
            cli.recursive = true;
            Ok(())
        }
        "remove-dest" => {
            cli.remove_dest = true;
            Ok(())
        }
        "reflink" => {
            let mode = match value {
                None => ReflinkMode::Auto,
                Some(text) => argmatch("--reflink", &text, REFLINK_GROUPS).map_err(usage)?,
            };
            if mode == ReflinkMode::Always {
                return Err(unsupported("cp", "--reflink=always"));
            }
            Ok(())
        }
        "sparse" => {
            let text = value.unwrap_or_default();
            argmatch("--sparse", &text, SPARSE_GROUPS).map_err(usage)?;
            Ok(())
        }
        "symbolic" => {
            cli.symbolic = true;
            Ok(())
        }
        "suffix" => {
            cli.suffix = value;
            Ok(())
        }
        "target" => {
            cli.target_dir = value;
            Ok(())
        }
        "no-target" => {
            cli.no_target_dir = true;
            Ok(())
        }
        "update-older" => {
            cli.update = Update::Older;
            Ok(())
        }
        "update" => {
            cli.update = match value {
                None => Update::Older,
                Some(text) => argmatch("--update", &text, UPDATE_GROUPS).map_err(usage)?,
            };
            Ok(())
        }
        "verbose" => {
            cli.verbose = true;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn build_options(cli: &Cli) -> Options {
    Options {
        cmd: "cp",
        recursive: cli.recursive,
        deref: cli.deref.unwrap_or(if cli.recursive {
            Deref::Never
        } else {
            Deref::Always
        }),
        preserve: cli.preserve,
        hard_link: cli.hard_link,
        symbolic: cli.symbolic,
        force: cli.force,
        remove_dest: cli.remove_dest,
        no_clobber: cli.no_clobber,
        interactive: cli.interactive,
        update: cli.update,
        backup: cli.backup,
        suffix: cli.suffix.clone().unwrap_or_else(backup::default_suffix),
        verbose: cli.verbose,
    }
}

/// Creates the missing parent directories of a `--parents` destination.
fn make_parents(dst: &Path) -> bool {
    match dst.parent() {
        Some(parent) if !parent.as_os_str().is_empty() && !parent.is_dir() => {
            match fs::create_dir_all(parent) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!(
                        "cp: cannot create directory '{}': {}",
                        parent.display(),
                        super::helpers::os_text(&error)
                    );
                    false
                }
            }
        }
        _ => true,
    }
}

fn execute(cli: &Cli) -> i32 {
    let plan = match targets::plan(
        "cp",
        &cli.operands,
        cli.target_dir.as_deref(),
        cli.no_target_dir,
    ) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    if cli.parents && !plan.dest_is_dir {
        return usage("with --parents, the destination must be a directory".to_string());
    }
    let mut copier = Copier::new(build_options(cli));
    let mut ok = true;
    for source in &plan.sources {
        let dst = targets::target_for(source, &plan, cli.parents);
        if cli.parents && !make_parents(&dst) {
            ok = false;
            continue;
        }
        ok &= copier.copy_top(Path::new(source), &dst);
    }
    if ok {
        EXIT_OK
    } else {
        EXIT_FAIL
    }
}

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("cp", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("cp", &error, EXIT_FAIL),
    };
    match collect(items) {
        Ok(cli) => execute(&cli),
        Err(code) => code,
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::copy;
    use crate::commands::fsops::helpers::testdir::TempDir;
    use std::path::PathBuf;

    fn settings(recursive: bool) -> Options {
        Options {
            cmd: "cp",
            recursive,
            deref: Deref::Never,
            preserve: Preserve::default(),
            hard_link: false,
            symbolic: false,
            force: false,
            remove_dest: false,
            no_clobber: false,
            interactive: false,
            update: Update::All,
            backup: Control::None,
            suffix: "~".to_string(),
            verbose: false,
        }
    }

    fn parsed(args: &[&str]) -> Cli {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        collect(Parser::new("cp", SPECS).parse(&args).unwrap()).unwrap()
    }

    #[test]
    fn copies_file_contents_to_a_new_name() {
        let t = TempDir::new("cp-file");
        let src = t.write("a", "hello");
        let dst = t.at("b");
        assert!(Copier::new(settings(false)).copy_top(&src, &dst));
        assert_eq!(fs::read_to_string(&dst).unwrap(), "hello");
    }

    #[test]
    fn directory_needs_recursive() {
        let t = TempDir::new("cp-dir");
        fs::create_dir(t.at("d")).unwrap();
        assert!(!Copier::new(settings(false)).copy_top(&t.at("d"), &t.at("e")));
        assert!(!t.at("e").exists());
    }

    #[test]
    fn recursive_copy_builds_the_tree() {
        let t = TempDir::new("cp-tree");
        t.write("d/s/f", "xy");
        assert!(Copier::new(settings(true)).copy_top(&t.at("d"), &t.at("e")));
        assert_eq!(fs::read_to_string(t.at("e/s/f")).unwrap(), "xy");
    }

    #[test]
    fn into_itself_detects_a_destination_inside_the_source() {
        let t = TempDir::new("cp-itself");
        fs::create_dir(t.at("d")).unwrap();
        assert!(copy::into_itself(&t.at("d"), &t.at("d/sub")));
        assert!(!copy::into_itself(&t.at("d"), &t.at("e")));
    }

    #[test]
    fn no_clobber_keeps_existing_destination() {
        let t = TempDir::new("cp-noclobber");
        let src = t.write("a", "new");
        let dst = t.write("b", "old");
        let mut opts = settings(false);
        opts.no_clobber = true;
        assert!(Copier::new(opts).copy_top(&src, &dst));
        assert_eq!(fs::read_to_string(&dst).unwrap(), "old");
    }

    #[test]
    fn simple_backup_keeps_the_old_file_with_suffix() {
        let t = TempDir::new("cp-backup");
        let src = t.write("a", "new");
        let dst = t.write("b", "old");
        let mut opts = settings(false);
        opts.backup = Control::Simple;
        assert!(Copier::new(opts).copy_top(&src, &dst));
        assert_eq!(fs::read_to_string(t.at("b~")).unwrap(), "old");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "new");
    }

    #[test]
    fn target_for_names_files_inside_a_directory() {
        let plan = targets::Plan {
            sources: vec!["x/y.txt".to_string()],
            dest: PathBuf::from("out"),
            dest_is_dir: true,
        };
        assert_eq!(
            targets::target_for("x/y.txt", &plan, false),
            PathBuf::from("out/y.txt")
        );
        assert_eq!(
            targets::target_for("x/y.txt", &plan, true),
            PathBuf::from("out/x/y.txt")
        );
    }

    #[test]
    fn archive_flag_sets_recursive_and_full_preservation() {
        let cli = parsed(&["-a", "src", "dst"]);
        let opts = build_options(&cli);
        assert!(opts.recursive);
        assert_eq!(opts.deref, Deref::Never);
        assert!(opts.preserve.mode && opts.preserve.timestamps && opts.preserve.links);
    }
}
