//! `mv`: move or rename files and directories. A rename is tried first; across devices the
//! tree is copied with attributes preserved and the source removed afterwards.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};
use crate::common::path::display_path;

use super::backup::{self, Control};
use super::copy::{self, Copier, Deref, Preserve, Update};
use super::helpers::{
    argmatch, ask, is_sep, os_text, print_help, same_entry, say, strip_trailing_seps, ArgGroup,
};
use super::rm::remove_tree;
use super::targets;

const HELP: &str = r#"Usage: mv [OPTION]... [-T] SOURCE DEST
  or:  mv [OPTION]... SOURCE... DIRECTORY
  or:  mv [OPTION]... -t DIRECTORY SOURCE...
Rename SOURCE to DEST, or move SOURCE(s) to DIRECTORY.

  -b                             like --backup but does not accept an argument
      --backup[=CONTROL]         make a backup of each existing destination file
  -f, --force                    do not prompt before overwriting
  -i, --interactive              prompt before overwrite
  -n, --no-clobber               do not overwrite an existing file
      --no-copy                  do not copy when moving across file systems
                                   (agentcmd extension: fail instead)
  -S, --suffix=SUFFIX            override the usual backup suffix
      --strip-trailing-slashes   remove any trailing slashes from each SOURCE
                                   argument
  -t, --target-directory=DIRECTORY  move all SOURCE arguments into DIRECTORY
  -T, --no-target-directory      treat DEST as a normal file
  -u, --update[=WHEN]            move only when the SOURCE file is newer than the
                                   destination file or when the destination file is
                                   missing
  -v, --verbose                  explain what is being done
      --help                     display this help and exit
      --version                  output version information and exit

The backup CONTROL is none (off), numbered (t), existing (nil) or simple (never).
The update WHEN is all, none, none-fail or older.
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("backup", Some('b'), None),
    OptSpec::opt("backup", None, Some("backup")),
    OptSpec::flag("force", Some('f'), Some("force")),
    OptSpec::flag("interactive", Some('i'), Some("interactive")),
    OptSpec::flag("no-clobber", Some('n'), Some("no-clobber")),
    OptSpec::flag("no-copy", None, Some("no-copy")),
    OptSpec::req("suffix", Some('S'), Some("suffix")),
    OptSpec::flag("strip", None, Some("strip-trailing-slashes")),
    OptSpec::req("target", Some('t'), Some("target-directory")),
    OptSpec::flag("no-target", Some('T'), Some("no-target-directory")),
    OptSpec::flag("update-older", Some('u'), None),
    OptSpec::opt("update", None, Some("update")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const UPDATE_GROUPS: &[ArgGroup<'_, Update>] = &[
    (&["all"], Update::All),
    (&["none"], Update::None),
    (&["none-fail"], Update::NoneFail),
    (&["older"], Update::Older),
];

/// The options that shape each move.
struct Settings {
    backup: Control,
    suffix: String,
    force: bool,
    interactive: bool,
    no_clobber: bool,
    no_copy: bool,
    update: Update,
    verbose: bool,
    strip: bool,
}

fn usage(message: String) -> i32 {
    report_usage("mv", &UsageError { message }, EXIT_FAIL)
}

/// Moves each source to its destination and keeps going after failures.
struct Mover<'a> {
    settings: &'a Settings,
}

impl Mover<'_> {
    fn fail(&self, text: &str) -> bool {
        eprintln!("mv: {text}");
        false
    }

    /// Moves one source to `dst`. Returns false if the move failed or was refused.
    fn move_one(&self, source: &str, dst: &Path) -> bool {
        let src_text = if self.settings.strip {
            strip_trailing_seps(source)
        } else {
            source
        };
        let src = Path::new(src_text);
        let dst_text = display_path(dst);
        let src_meta = match fs::symlink_metadata(src) {
            Ok(meta) => meta,
            Err(error) => {
                return self.fail(&format!("cannot stat '{src_text}': {}", os_text(&error)));
            }
        };
        let src_is_dir = src_meta.is_dir();
        if !src_is_dir && source.ends_with(is_sep) {
            return self.fail(&format!(
                "cannot move '{}' to '{dst_text}': Not a directory",
                strip_trailing_seps(source)
            ));
        }
        let mut backup_made = None;
        if fs::symlink_metadata(dst).is_ok() {
            if same_entry(src, dst) {
                return self.fail(&format!("'{src_text}' and '{dst_text}' are the same file"));
            }
            let dst_is_dir = fs::metadata(dst).is_ok_and(|meta| meta.is_dir());
            if src_is_dir && !dst_is_dir {
                return self.fail(&format!(
                    "cannot overwrite non-directory '{dst_text}' with directory '{src_text}'"
                ));
            }
            if !src_is_dir && dst_is_dir {
                return self.fail(&format!(
                    "cannot overwrite directory '{dst_text}' with non-directory"
                ));
            }
            match self.existing_decision(src, dst) {
                Ok(true) => {}
                Ok(false) => return true,
                Err(()) => return false,
            }
            if self.settings.backup != Control::None {
                let target = backup::backup_name(dst, self.settings.backup, &self.settings.suffix);
                if let Err(error) = fs::rename(dst, &target) {
                    return self.fail(&format!("cannot backup '{dst_text}': {}", os_text(&error)));
                }
                backup_made = Some(target);
            }
        }
        if src_is_dir && copy::into_itself(src, dst) {
            return self.fail(&format!(
                "cannot move '{src_text}' to a subdirectory of itself, '{dst_text}'"
            ));
        }
        let moved = self.rename_or_copy(src, src_is_dir, dst);
        if moved && self.settings.verbose {
            let suffix = backup_made
                .map(|path| format!(" (backup: '{}')", display_path(&path)))
                .unwrap_or_default();
            say(&format!("renamed '{src_text}' -> '{dst_text}'{suffix}"));
        }
        moved
    }

    /// `Ok(true)` to replace, `Ok(false)` to skip quietly, `Err` to skip and fail.
    fn existing_decision(&self, src: &Path, dst: &Path) -> Result<bool, ()> {
        let dst_text = display_path(dst);
        if self.settings.no_clobber {
            eprintln!("mv: not replacing '{dst_text}'");
            return Err(());
        }
        let src_time = fs::symlink_metadata(src).and_then(|meta| meta.modified());
        let dst_time = fs::metadata(dst).and_then(|meta| meta.modified());
        match self.settings.update {
            Update::None => return Ok(false),
            Update::NoneFail => {
                eprintln!("mv: not replacing '{dst_text}'");
                return Err(());
            }
            Update::Older => {
                if let (Ok(src_time), Ok(dst_time)) = (src_time, dst_time) {
                    if src_time <= dst_time {
                        return Ok(false);
                    }
                }
            }
            Update::All => {}
        }
        if self.settings.interactive
            && !self.settings.force
            && !ask(&format!("mv: overwrite '{dst_text}'? "))
        {
            return Err(());
        }
        Ok(true)
    }

    fn rename_or_copy(&self, src: &Path, src_is_dir: bool, dst: &Path) -> bool {
        let src_text = display_path(src);
        let dst_text = display_path(dst);
        if cfg!(windows) && src_is_dir && fs::metadata(dst).is_ok_and(|meta| meta.is_dir()) {
            // Windows will not rename over an existing directory, even an empty one. A
            // non-empty destination is reported as GNU does, not as a rename access error.
            if fs::remove_dir(dst).is_err() && has_entries(dst) {
                return self.fail(&format!(
                    "cannot move '{src_text}' to '{dst_text}': Directory not empty"
                ));
            }
        }
        match fs::rename(src, dst) {
            Ok(()) => true,
            Err(error) if is_cross_device(&error) => {
                if self.settings.no_copy {
                    return self.fail(&format!(
                        "cannot move '{src_text}' to '{dst_text}': Invalid cross-device link"
                    ));
                }
                self.copy_then_remove(src, dst)
            }
            Err(error) => self.fail(&format!(
                "cannot move '{src_text}' to '{dst_text}': {}",
                os_text(&error)
            )),
        }
    }

    /// Cross-device move: copy with every attribute kept, then remove the source tree.
    fn copy_then_remove(&self, src: &Path, dst: &Path) -> bool {
        let mut copier = Copier::new(copy::Options {
            cmd: "mv",
            recursive: true,
            deref: Deref::Never,
            preserve: Preserve {
                mode: true,
                ownership: true,
                timestamps: true,
                links: true,
            },
            hard_link: false,
            symbolic: false,
            force: true,
            remove_dest: false,
            no_clobber: false,
            interactive: false,
            update: Update::All,
            backup: Control::None,
            suffix: self.settings.suffix.clone(),
            verbose: false,
        });
        if !copier.copy_top(src, dst) {
            return false;
        }
        match remove_tree(src) {
            Ok(()) => true,
            Err(error) => self.fail(&format!(
                "cannot remove '{}': {}",
                display_path(src),
                os_text(&error)
            )),
        }
    }
}

/// True when the directory `path` has at least one entry.
fn has_entries(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
}

/// True when `rename` failed because source and destination are on different devices.
fn is_cross_device(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::CrossesDevices {
        return true;
    }
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(18) // EXDEV
    }
    #[cfg(windows)]
    {
        error.raw_os_error() == Some(17) // ERROR_NOT_SAME_DEVICE
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

#[derive(Default)]
struct Cli {
    settings: Option<Settings>,
    target: Option<String>,
    no_target: bool,
    operands: Vec<String>,
}

fn collect(items: Vec<Item>) -> Result<Cli, i32> {
    let mut cli = Cli {
        settings: Some(Settings {
            backup: Control::None,
            suffix: backup::default_suffix(),
            force: false,
            interactive: false,
            no_clobber: false,
            no_copy: false,
            update: Update::All,
            verbose: false,
            strip: false,
        }),
        ..Cli::default()
    };
    for item in items {
        let settings = cli.settings.as_mut().expect("settings are always present");
        match item {
            Item::Operand(text) => cli.operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => return Err(print_help("mv", HELP)),
                "version" => return Err(print_version("mv")),
                "backup" => {
                    settings.backup = match value {
                        None => Control::Existing,
                        Some(text) => backup::parse_control(&text).map_err(usage)?,
                    };
                }
                "force" => {
                    settings.force = true;
                    settings.interactive = false;
                    settings.no_clobber = false;
                }
                "interactive" => {
                    settings.interactive = true;
                    settings.force = false;
                }
                "no-clobber" => settings.no_clobber = true,
                "no-copy" => settings.no_copy = true,
                "suffix" => settings.suffix = value.unwrap_or_default(),
                "strip" => settings.strip = true,
                "target" => cli.target = value,
                "no-target" => cli.no_target = true,
                "update-older" => settings.update = Update::Older,
                "update" => {
                    settings.update = match value {
                        None => Update::Older,
                        Some(text) => argmatch("--update", &text, UPDATE_GROUPS).map_err(usage)?,
                    };
                }
                "verbose" => settings.verbose = true,
                _ => {}
            },
        }
    }
    Ok(cli)
}

/// Entry point for `mv`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("mv", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("mv", &error, EXIT_FAIL),
    };
    let cli = match collect(items) {
        Ok(cli) => cli,
        Err(code) => return code,
    };
    let plan = match targets::plan("mv", &cli.operands, cli.target.as_deref(), cli.no_target) {
        Ok(plan) => plan,
        Err(code) => return code,
    };
    let settings = cli.settings.expect("settings are always present");
    let mover = Mover {
        settings: &settings,
    };
    let mut ok = true;
    for source in &plan.sources {
        let dst: PathBuf = targets::target_for(source, &plan, false);
        ok &= mover.move_one(source, &dst);
    }
    if ok {
        EXIT_OK
    } else {
        EXIT_FAIL
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;

    fn settings() -> Settings {
        Settings {
            backup: Control::None,
            suffix: "~".to_string(),
            force: false,
            interactive: false,
            no_clobber: false,
            no_copy: false,
            update: Update::All,
            verbose: false,
            strip: false,
        }
    }

    #[test]
    fn renames_a_file() {
        let t = TempDir::new("mv-rename");
        let src = t.write("a", "x");
        let s = settings();
        let mover = Mover { settings: &s };
        assert!(mover.move_one(src.to_str().unwrap(), &t.at("b")));
        assert!(!src.exists());
        assert!(t.at("b").exists());
    }

    #[test]
    fn directory_into_its_own_subdirectory_is_refused() {
        let t = TempDir::new("mv-itself");
        fs::create_dir_all(t.at("a/x")).unwrap();
        let s = settings();
        let mover = Mover { settings: &s };
        assert!(!mover.move_one(t.at("a").to_str().unwrap(), &t.at("a/x/y")));
        assert!(t.at("a").is_dir());
    }

    #[test]
    fn no_clobber_refuses_to_replace() {
        let t = TempDir::new("mv-noclobber");
        let src = t.write("a", "new");
        t.write("b", "old");
        let mut s = settings();
        s.no_clobber = true;
        let mover = Mover { settings: &s };
        assert!(!mover.move_one(src.to_str().unwrap(), &t.at("b")));
        assert!(src.exists());
    }

    #[test]
    fn trailing_slash_on_a_file_is_refused() {
        let t = TempDir::new("mv-slash");
        t.write("a", "x");
        let s = settings();
        let mover = Mover { settings: &s };
        let with_slash = format!("{}/", t.at("a").display());
        assert!(!mover.move_one(&with_slash, &t.at("b")));
        assert!(t.at("a").exists());
    }

    #[test]
    fn same_entry_distinguishes_hard_links_from_symlinks() {
        let t = TempDir::new("mv-entry");
        let a = t.write("a", "x");
        fs::hard_link(&a, t.at("h")).unwrap();
        assert!(same_entry(&a, &t.at("h")));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&a, t.at("s")).unwrap();
            assert!(!same_entry(&a, &t.at("s")));
        }
    }

    #[test]
    fn cross_device_errors_are_recognised() {
        assert!(is_cross_device(&io::Error::from(
            io::ErrorKind::CrossesDevices
        )));
        assert!(!is_cross_device(&io::Error::from(io::ErrorKind::NotFound)));
    }
}
