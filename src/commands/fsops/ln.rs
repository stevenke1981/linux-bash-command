//! `ln`: make hard or symbolic links. Symbolic links to relative targets are written as given,
//! or relative to the link's directory with `-r`.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, unsupported, EXIT_FAIL, EXIT_OK};
use crate::common::path::display_path;

use super::backup::{self, Control};
use super::helpers::{
    ask, join_name, make_symlink, os_text, print_help, same_file, say, symlink_error_text,
};
use super::targets;

const HELP: &str = r#"Usage: ln [OPTION]... [-T] TARGET LINK_NAME
  or:  ln [OPTION]... TARGET
  or:  ln [OPTION]... TARGET... DIRECTORY
  or:  ln [OPTION]... -t DIRECTORY TARGET...
In the 1st form, create a link to TARGET with the name LINK_NAME.
In the 2nd form, create a link to TARGET in the current directory.
In the 3rd and 4th forms, create links to each TARGET in DIRECTORY.
Create hard links by default, symbolic links with --symbolic.

  -b                      like --backup but does not accept an argument
      --backup[=CONTROL]  make a backup of each existing destination file
  -f, --force             remove existing destination files
  -i, --interactive       prompt whether to remove destinations
  -L, --logical           dereference TARGETs that are symbolic links
  -n, --no-dereference    treat LINK_NAME as a normal file if it is a symbolic
                            link to a directory
  -P, --physical          make hard links directly to symbolic links
  -r, --relative          with -s, create links relative to link location
  -s, --symbolic          make symbolic links instead of hard links
  -S, --suffix=SUFFIX     override the usual backup suffix
  -t, --target-directory=DIRECTORY  specify the DIRECTORY in which to create
                            the links
  -T, --no-target-directory  treat LINK_NAME as a normal file always
  -v, --verbose           print name of each linked file
      --help              display this help and exit
      --version           output version information and exit

The backup CONTROL is none (off), numbered (t), existing (nil) or simple (never).
Not supported: -d, --directory, -F.
"#;

const SPECS: &[OptSpec] = &[
    OptSpec::flag("backup", Some('b'), None),
    OptSpec::opt("backup", None, Some("backup")),
    OptSpec::flag("directory-short", Some('d'), None),
    OptSpec::flag("directory", None, Some("directory")),
    OptSpec::flag("directory-f", Some('F'), None),
    OptSpec::flag("force", Some('f'), Some("force")),
    OptSpec::flag("interactive", Some('i'), Some("interactive")),
    OptSpec::flag("logical", Some('L'), Some("logical")),
    OptSpec::flag("no-deref", Some('n'), Some("no-dereference")),
    OptSpec::flag("physical", Some('P'), Some("physical")),
    OptSpec::flag("relative", Some('r'), Some("relative")),
    OptSpec::flag("symbolic", Some('s'), Some("symbolic")),
    OptSpec::req("suffix", Some('S'), Some("suffix")),
    OptSpec::req("target", Some('t'), Some("target-directory")),
    OptSpec::flag("no-target", Some('T'), Some("no-target-directory")),
    OptSpec::flag("verbose", Some('v'), Some("verbose")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Options after parsing.
struct Settings {
    backup: Control,
    suffix: String,
    force: bool,
    interactive: bool,
    logical: bool,
    relative: bool,
    symbolic: bool,
    verbose: bool,
}

fn usage(message: String) -> i32 {
    report_usage("ln", &UsageError { message }, EXIT_FAIL)
}

/// Lexical relative path from directory `from` to `to`; both must be absolute.
fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from_parts: Vec<Component<'_>> = from.components().collect();
    let to_parts: Vec<Component<'_>> = to.components().collect();
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = PathBuf::new();
    for _ in common..from_parts.len() {
        out.push("..");
    }
    for part in &to_parts[common..] {
        out.push(part.as_os_str());
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// The text a relative symbolic link stores: `target` seen from the link's directory.
fn relative_target(target: &Path, link: &Path) -> PathBuf {
    let link_dir = link
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let target_abs = fs::canonicalize(target).or_else(|_| std::path::absolute(target));
    let dir_abs = fs::canonicalize(link_dir).or_else(|_| std::path::absolute(link_dir));
    match (target_abs, dir_abs) {
        (Ok(t), Ok(d)) => relative_path(&d, &t),
        _ => target.to_path_buf(),
    }
}

/// Makes one link. Returns false on failure (already reported).
fn make_link(target: &str, link: &Path, settings: &Settings) -> bool {
    let link_text = display_path(link);
    if fs::symlink_metadata(link).is_ok() && !clear_for_link(target, link, settings) {
        return false;
    }
    let result = if settings.symbolic {
        make_symbolic(target, link, settings)
    } else {
        make_hard(target, link, settings)
    };
    match result {
        Ok(shown) => {
            if settings.verbose {
                say(&format!("'{link_text}' {shown}"));
            }
            true
        }
        Err(message) => {
            eprintln!("ln: {message}");
            false
        }
    }
}

/// Removes or backs up an existing link name when `-f`, `-i` or `-b` asks for it.
fn clear_for_link(target: &str, link: &Path, settings: &Settings) -> bool {
    let link_text = display_path(link);
    if settings.force && same_file(Path::new(target), link) {
        eprintln!("ln: '{target}' and '{link_text}' are the same file");
        return false;
    }
    if settings.interactive && !ask(&format!("ln: replace '{link_text}'? ")) {
        return false;
    }
    if settings.backup != Control::None {
        let backup = backup::backup_name(link, settings.backup, &settings.suffix);
        if let Err(error) = fs::rename(link, backup) {
            eprintln!("ln: cannot backup '{link_text}': {}", os_text(&error));
            return false;
        }
        return true;
    }
    if settings.force {
        if let Err(error) = fs::remove_file(link) {
            eprintln!("ln: cannot remove '{link_text}': {}", os_text(&error));
            return false;
        }
        return true;
    }
    eprintln!(
        "ln: failed to create {} link '{link_text}': File exists",
        if settings.symbolic {
            "symbolic"
        } else {
            "hard"
        }
    );
    false
}

/// Creates a symbolic link; returns the verbose suffix (`-> 'content'`).
fn make_symbolic(target: &str, link: &Path, settings: &Settings) -> Result<String, String> {
    let content = if settings.relative {
        relative_target(Path::new(target), link)
    } else {
        PathBuf::from(target)
    };
    let resolved = link
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .join(&content);
    let is_dir = fs::metadata(&resolved).is_ok_and(|meta| meta.is_dir());
    make_symlink(&content, link, is_dir).map_err(|error| {
        format!(
            "failed to create symbolic link '{}': {}",
            display_path(link),
            symlink_error_text(&error)
        )
    })?;
    Ok(format!("-> '{}'", display_path(&content)))
}

/// Creates a hard link; returns the verbose suffix (`=> 'target'`).
fn make_hard(target: &str, link: &Path, settings: &Settings) -> Result<String, String> {
    let source = Path::new(target);
    if let Err(error) = fs::symlink_metadata(source) {
        return Err(format!("failed to access '{target}': {}", os_text(&error)));
    }
    let real = if settings.logical {
        fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf())
    } else {
        source.to_path_buf()
    };
    fs::hard_link(&real, link).map_err(|error| {
        format!(
            "failed to create hard link '{}': {}",
            display_path(link),
            os_text(&error)
        )
    })?;
    Ok(format!("=> '{target}'"))
}

/// Parses `ln` options into [`Settings`] plus the operands.
fn collect(items: Vec<Item>) -> Result<(Settings, Option<String>, bool, Vec<String>), i32> {
    let mut settings = Settings {
        backup: Control::None,
        suffix: backup::default_suffix(),
        force: false,
        interactive: false,
        logical: false,
        relative: false,
        symbolic: false,
        verbose: false,
    };
    let mut target = None;
    let mut no_target = false;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Operand(text) => operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => return Err(print_help("ln", HELP)),
                "version" => return Err(print_version("ln")),
                "directory-short" => return Err(unsupported("ln", "-d")),
                "directory" => return Err(unsupported("ln", "--directory")),
                "directory-f" => return Err(unsupported("ln", "-F")),
                "backup" => {
                    settings.backup = match value {
                        None => Control::Existing,
                        Some(text) => backup::parse_control(&text).map_err(usage)?,
                    };
                }
                "force" => settings.force = true,
                "interactive" => settings.interactive = true,
                "logical" => settings.logical = true,
                "no-deref" | "physical" => {}
                "relative" => settings.relative = true,
                "symbolic" => settings.symbolic = true,
                "suffix" => settings.suffix = value.unwrap_or_default(),
                "target" => target = value,
                "no-target" => no_target = true,
                "verbose" => settings.verbose = true,
                _ => {}
            },
        }
    }
    Ok((settings, target, no_target, operands))
}

/// Entry point for `ln`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("ln", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("ln", &error, EXIT_FAIL),
    };
    let (settings, target, no_target, operands) = match collect(items) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let plan = if operands.len() == 1 && target.is_none() && !no_target {
        targets::Plan {
            sources: operands.clone(),
            dest: PathBuf::from("."),
            dest_is_dir: true,
        }
    } else {
        match targets::plan("ln", &operands, target.as_deref(), no_target) {
            Ok(plan) => plan,
            Err(code) => return code,
        }
    };
    let mut ok = true;
    for source in &plan.sources {
        let link = if plan.dest_is_dir {
            join_name(&plan.dest, source)
        } else {
            plan.dest.clone()
        };
        ok &= make_link(source, &link, &settings);
    }
    if ok {
        EXIT_OK
    } else {
        EXIT_FAIL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_climbs_out_of_the_link_directory() {
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/c/d")),
            PathBuf::from("../c/d")
        );
        assert_eq!(
            relative_path(Path::new("/a/b/c"), Path::new("/a/b")),
            PathBuf::from("..")
        );
        assert_eq!(
            relative_path(Path::new("/x"), Path::new("/x")),
            PathBuf::from(".")
        );
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::commands::fsops::helpers::testdir::TempDir;

    fn settings(symbolic: bool, force: bool) -> Settings {
        Settings {
            backup: Control::None,
            suffix: "~".to_string(),
            force,
            interactive: false,
            logical: false,
            relative: false,
            symbolic,
            verbose: false,
        }
    }

    #[test]
    fn hard_link_is_created_and_shares_contents() {
        let t = TempDir::new("ln-hard");
        t.write("a", "x");
        let link = t.at("b");
        assert!(make_link(
            t.at("a").to_str().unwrap(),
            &link,
            &settings(false, false)
        ));
        assert_eq!(fs::read_to_string(&link).unwrap(), "x");
    }

    #[test]
    #[cfg(unix)]
    fn symbolic_link_stores_the_target_as_given() {
        let t = TempDir::new("ln-sym");
        t.write("a", "x");
        let link = t.at("l");
        assert!(make_link("a", &link, &settings(true, false)));
        assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("a"));
    }

    #[test]
    fn existing_name_fails_without_force_and_is_replaced_with_it() {
        let t = TempDir::new("ln-force");
        t.write("a", "x");
        t.write("b", "y");
        assert!(!make_link(
            t.at("a").to_str().unwrap(),
            &t.at("b"),
            &settings(false, false)
        ));
        assert!(make_link(
            t.at("a").to_str().unwrap(),
            &t.at("b"),
            &settings(false, true)
        ));
        assert_eq!(fs::read_to_string(t.at("b")).unwrap(), "x");
    }

    #[test]
    fn relative_target_points_back_through_the_link_directory() {
        let t = TempDir::new("ln-relative");
        t.write("d/t", "x");
        fs::create_dir(t.at("e")).unwrap();
        assert_eq!(
            relative_target(&t.at("d/t"), &t.at("e/l")),
            PathBuf::from("../d/t")
        );
    }

    #[test]
    fn missing_hard_link_target_is_reported() {
        let t = TempDir::new("ln-missing");
        let error = make_hard(
            &t.at("nope").to_string_lossy(),
            &t.at("x"),
            &settings(false, false),
        );
        assert!(error.unwrap_err().contains("failed to access"));
    }
}
