//! `rm`: remove files and directories. Symbolic links are removed themselves, never followed.
//! `.`, `..` and (for `-r`) filesystem roots are refused unless `--no-preserve-root` is given.

use std::fs;
use std::io;
use std::path::Path;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

use super::helpers::{
    argmatch, ask, clear_readonly, device_of, link_is_dir, os_text, print_help, remove_link, say,
    stdin_is_tty, ArgGroup,
};
use super::safety::{refuse_reason, Refusal};

const HELP: &str = r#"Usage: rm [OPTION]... [FILE]...
Remove (unlink) the FILE(s).

  -f, --force           ignore nonexistent files and arguments, never prompt
  -i                    prompt before every removal
  -I                    prompt once before removing more than three files, or
                          when removing recursively; less intrusive than -i,
                          while still giving protection against most mistakes
      --interactive[=WHEN]  prompt according to WHEN: never, once (-I), or
                          always (-i); without WHEN, prompt always
      --one-file-system  when removing a hierarchy recursively, skip any
                          directory that is on a file system different from
                          that of the corresponding command line argument
      --no-preserve-root  do not treat '/' specially
      --preserve-root[=all]  do not remove '/' (default); with 'all', reject
                          any command line argument on a separate device
  -r, -R, --recursive   remove directories and their contents recursively
  -d, --dir             remove empty directories
  -v, --verbose         explain what is being done
      --help            display this help and exit
      --version         output version information and exit

By default, rm does not remove directories.  Use the --recursive (-r or -R)
option to remove each listed directory, too, along with all of its contents.

Any attempt to remove a file whose last file name component is '.' or '..'
is rejected with a diagnostic.
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("force", Some('f'), Some("force")),
    OptSpec::flag("always", Some('i'), None),
    OptSpec::flag("once", Some('I'), None),
    OptSpec::opt("interactive", None, Some("interactive")),
    OptSpec::flag("one-fs", None, Some("one-file-system")),
    OptSpec::flag("no-preserve-root", None, Some("no-preserve-root")),
    OptSpec::opt("preserve-root", None, Some("preserve-root")),
    OptSpec::flag("recursive", Some('r'), Some("recursive")),
    OptSpec::flag("recursive", Some('R'), None),
    OptSpec::flag("dir", Some('d'), Some("dir")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const WHEN_GROUPS: &[ArgGroup<'_, Interactive>] = &[
    (&["never", "no", "none"], Interactive::Never),
    (&["once"], Interactive::Once),
    (&["always", "yes"], Interactive::Always),
];

/// When to ask before removing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interactive {
    Never,
    Once,
    Always,
}

/// Parsed options.
#[derive(Clone, Debug)]
pub struct Opts {
    pub force: bool,
    pub interactive: Interactive,
    pub recursive: bool,
    pub dir: bool,
    pub verbose: bool,
    pub one_file_system: bool,
    pub preserve_root: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            force: false,
            interactive: Interactive::Never,
            recursive: false,
            dir: false,
            verbose: false,
            one_file_system: false,
            preserve_root: true,
        }
    }
}

/// Removes one top-level operand and everything below it, reporting as it goes.
struct Remover {
    opts: Opts,
    failed: bool,
    /// Device of the current top-level operand, for `--one-file-system`.
    root_dev: Option<u64>,
}

impl Remover {
    fn error(&mut self, text: &str) {
        eprintln!("rm: {text}");
        self.failed = true;
    }

    fn remove_operand(&mut self, operand: &str) {
        let path = Path::new(operand);
        match refuse_reason(path) {
            Some(Refusal::DotOrDotDot) => {
                self.error(&format!(
                    "refusing to remove '.' or '..' directory: skipping '{operand}'"
                ));
                return;
            }
            Some(Refusal::Root) if self.opts.recursive && self.opts.preserve_root => {
                self.error(&format!(
                    "it is dangerous to operate recursively on '{operand}'"
                ));
                self.error("use --no-preserve-root to override this failsafe");
                return;
            }
            _ => {}
        }
        self.root_dev = fs::symlink_metadata(path)
            .ok()
            .and_then(|meta| device_of(&meta));
        self.remove_path(path, operand);
    }

    /// Removes `path`, displayed as `shown`. Returns nothing: failures set `self.failed`.
    fn remove_path(&mut self, path: &Path, shown: &str) {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) => {
                if error.kind() == io::ErrorKind::NotFound && self.opts.force {
                    return;
                }
                self.error(&format!("cannot remove '{shown}': {}", os_text(&error)));
                return;
            }
        };
        let kind = meta.file_type();
        if kind.is_dir() {
            self.remove_dir_entry(path, shown, &meta);
        } else {
            self.remove_file_entry(path, shown, &meta);
        }
    }

    fn remove_file_entry(&mut self, path: &Path, shown: &str, meta: &fs::Metadata) {
        let kind = meta.file_type();
        if self.opts.interactive == Interactive::Always {
            let what = if kind.is_symlink() {
                "symbolic link".to_string()
            } else if meta.len() == 0 && kind.is_file() {
                "regular empty file".to_string()
            } else if kind.is_file() {
                "regular file".to_string()
            } else {
                "file".to_string()
            };
            if !ask(&format!("rm: remove {what} '{shown}'? ")) {
                return;
            }
        } else if self.is_write_protected_prompt(meta)
            && !ask(&format!(
                "rm: remove write-protected regular file '{shown}'? "
            ))
        {
            return;
        }
        let result = if kind.is_symlink() {
            remove_link(path, link_is_dir(kind))
        } else {
            clear_readonly(path);
            fs::remove_file(path)
        };
        match result {
            Ok(()) => self.announce(&format!("removed '{shown}'")),
            Err(error) => self.error(&format!("cannot remove '{shown}': {}", os_text(&error))),
        }
    }

    /// GNU asks about write-protected files only when standard input is a terminal and
    /// `-f` was not given.
    fn is_write_protected_prompt(&self, meta: &fs::Metadata) -> bool {
        !self.opts.force
            && self.opts.interactive == Interactive::Never
            && meta.permissions().readonly()
            && stdin_is_tty()
    }

    fn remove_dir_entry(&mut self, path: &Path, shown: &str, meta: &fs::Metadata) {
        if !self.opts.recursive && !self.opts.dir {
            self.error(&format!("cannot remove '{shown}': Is a directory"));
            return;
        }
        if self.opts.one_file_system && device_of(meta) != self.root_dev {
            self.error(&format!(
                "skipping '{shown}', since it's on a different device"
            ));
            return;
        }
        if self.opts.recursive {
            if self.opts.interactive == Interactive::Always
                && !ask(&format!("rm: descend into directory '{shown}'? "))
            {
                return;
            }
            self.remove_children(path, shown);
        }
        if self.opts.interactive == Interactive::Always
            && !ask(&format!("rm: remove directory '{shown}'? "))
        {
            return;
        }
        match fs::remove_dir(path) {
            Ok(()) => self.announce(&format!("removed directory '{shown}'")),
            Err(error) => self.error(&format!("cannot remove '{shown}': {}", os_text(&error))),
        }
    }

    fn remove_children(&mut self, path: &Path, shown: &str) {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                self.error(&format!("cannot remove '{shown}': {}", os_text(&error)));
                return;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let child_shown = format!("{shown}/{}", entry.file_name().to_string_lossy());
                    self.remove_path(&entry.path(), &child_shown);
                }
                Err(error) => self.error(&format!("cannot remove '{shown}': {}", os_text(&error))),
            }
        }
    }

    fn announce(&self, text: &str) {
        if self.opts.verbose {
            say(text);
        }
    }
}

/// Removes `path` and everything below it without prompting. Used by `mv` after a copy
/// across devices.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    let kind = meta.file_type();
    if kind.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        fs::remove_dir(path)
    } else if kind.is_symlink() {
        remove_link(path, link_is_dir(kind))
    } else {
        clear_readonly(path);
        fs::remove_file(path)
    }
}

/// Parsed command line: the options and the operands.
struct Cli {
    opts: Opts,
    operands: Vec<String>,
}

fn collect(items: Vec<Item>) -> Result<Cli, i32> {
    let mut opts = Opts::default();
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => return Err(print_help("rm", HELP)),
                "version" => return Err(print_version("rm")),
                "force" => {
                    opts.force = true;
                    opts.interactive = Interactive::Never;
                }
                "always" => opts.interactive = Interactive::Always,
                "once" => opts.interactive = Interactive::Once,
                "interactive" => {
                    opts.interactive =
                        match value {
                            None => Interactive::Always,
                            Some(text) => argmatch("--interactive", &text, WHEN_GROUPS).map_err(
                                |message| report_usage("rm", &UsageError { message }, EXIT_FAIL),
                            )?,
                        };
                }
                "one-fs" => opts.one_file_system = true,
                "no-preserve-root" => opts.preserve_root = false,
                "preserve-root" => match value.as_deref() {
                    None | Some("all") => opts.preserve_root = true,
                    Some(text) => {
                        return Err(report_usage(
                            "rm",
                            &UsageError {
                                message: format!("unrecognized --preserve-root argument: '{text}'"),
                            },
                            EXIT_FAIL,
                        ))
                    }
                },
                "recursive" => opts.recursive = true,
                "dir" => opts.dir = true,
                "verbose" => opts.verbose = true,
                _ => {}
            },
        }
    }
    Ok(Cli { opts, operands })
}

/// Entry point for `rm`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("rm", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("rm", &error, EXIT_FAIL),
    };
    let cli = match collect(items) {
        Ok(cli) => cli,
        Err(code) => return code,
    };
    if cli.operands.is_empty() {
        if cli.opts.force {
            return EXIT_OK;
        }
        return report_usage(
            "rm",
            &UsageError {
                message: "missing operand".to_string(),
            },
            EXIT_FAIL,
        );
    }
    if cli.opts.interactive == Interactive::Once && !confirm_once(&cli) {
        return EXIT_OK;
    }
    let mut remover = Remover {
        opts: cli.opts,
        failed: false,
        root_dev: None,
    };
    for operand in &cli.operands {
        remover.remove_operand(operand);
    }
    if remover.failed {
        EXIT_FAIL
    } else {
        EXIT_OK
    }
}

/// `-I`: one question before removing more than three files or anything recursively.
fn confirm_once(cli: &Cli) -> bool {
    let count = cli.operands.len();
    if !cli.opts.recursive && count <= 3 {
        return true;
    }
    let noun = if count == 1 { "argument" } else { "arguments" };
    let recursively = if cli.opts.recursive {
        " recursively"
    } else {
        ""
    };
    ask(&format!("rm: remove {count} {noun}{recursively}? "))
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;

    fn remover(opts: Opts) -> Remover {
        Remover {
            opts,
            failed: false,
            root_dev: None,
        }
    }

    #[test]
    fn remove_tree_deletes_nested_directories() {
        let t = TempDir::new("rm-tree");
        fs::create_dir_all(t.at("d/s")).unwrap();
        t.write("d/s/f", "x");
        remove_tree(&t.at("d")).unwrap();
        assert!(!t.at("d").exists());
    }

    #[test]
    fn removes_a_regular_file() {
        let t = TempDir::new("rm-file");
        let file = t.write("a", "x");
        let mut r = remover(Opts::default());
        r.remove_operand(file.to_str().unwrap());
        assert!(!file.exists());
        assert!(!r.failed);
    }

    #[test]
    fn directory_without_recursive_is_refused() {
        let t = TempDir::new("rm-dir-norec");
        fs::create_dir(t.at("d")).unwrap();
        let mut r = remover(Opts::default());
        r.remove_operand(t.at("d").to_str().unwrap());
        assert!(r.failed);
        assert!(t.at("d").is_dir());
    }

    #[test]
    fn recursive_removes_a_whole_tree() {
        let t = TempDir::new("rm-dir-rec");
        t.write("d/f", "x");
        let opts = Opts {
            recursive: true,
            ..Opts::default()
        };
        let mut r = remover(opts);
        r.remove_operand(t.at("d").to_str().unwrap());
        assert!(!r.failed);
        assert!(!t.at("d").exists());
    }

    #[test]
    fn force_ignores_a_missing_operand() {
        let t = TempDir::new("rm-force");
        let opts = Opts {
            force: true,
            ..Opts::default()
        };
        let mut r = remover(opts);
        r.remove_operand(t.at("nope").to_str().unwrap());
        assert!(!r.failed);
        let mut strict = remover(Opts::default());
        strict.remove_operand(t.at("nope").to_str().unwrap());
        assert!(strict.failed);
    }

    #[test]
    fn option_parsing_selects_interactive_modes_last_wins() {
        let args: Vec<String> = ["-I", "--interactive=always", "-f"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let items = Parser::new("rm", SPECS).parse(&args).unwrap();
        let cli = collect(items).unwrap();
        assert_eq!(cli.opts.interactive, Interactive::Never);
        assert!(cli.opts.force);
        let args: Vec<String> = ["-f", "-i"].iter().map(|s| s.to_string()).collect();
        let cli = collect(Parser::new("rm", SPECS).parse(&args).unwrap()).unwrap();
        assert_eq!(cli.opts.interactive, Interactive::Always);
    }
}
