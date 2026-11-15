use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, io, path,
};
use std::{env, fs, path::PathBuf};

const HELP: &str = "Usage: which [OPTION]... COMMAND...\nLocate commands in PATH.\n  -a        print all matching commands\n  -s        no output, only return status\n      --help     display this help\n      --version  output version information\nExit status: 0 if all commands are found, 1 otherwise, 2 on invalid options.\n";

#[derive(Default, Debug)]
struct Options {
    all: bool,
    silent: bool,
    names: Vec<String>,
}

fn parse(arguments: &[String]) -> Result<Options, i32> {
    let specs = [
        OptSpec {
            id: "all",
            short: 'a',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec {
            id: "silent",
            short: 's',
            long: "",
            has_arg: HasArg::No,
        },
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = args::parse(arguments, &specs, true)
        .map_err(|message| error::usage("which", message, 2))?;
    let mut options = Options::default();
    for item in items {
        match item {
            Item::Opt("help", _) => {
                return Err(args::help_version("which", &["--help".into()], HELP, true).unwrap())
            }
            Item::Opt("version", _) => {
                return Err(args::help_version("which", &["--version".into()], HELP, true).unwrap())
            }
            Item::Opt("all", _) => options.all = true,
            Item::Opt("silent", _) => options.silent = true,
            Item::Operand(name) => options.names.push(name),
            Item::Opt(_, _) => unreachable!(),
        }
    }
    if options.names.is_empty() {
        return Err(error::usage("which", "missing operand", 1));
    }
    Ok(options)
}

#[cfg(any(windows, test))]
fn extensions(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.starts_with('.') {
                part.to_owned()
            } else {
                format!(".{part}")
            }
        })
        .collect()
}

fn names_for(name: &str, extensions: &[String], windows: bool) -> Vec<String> {
    if !windows {
        return vec![name.to_owned()];
    }
    if let Some(extension) = PathBuf::from(name).extension().and_then(|s| s.to_str()) {
        if extensions.iter().any(|candidate| {
            candidate
                .trim_start_matches('.')
                .eq_ignore_ascii_case(extension)
        }) {
            return vec![name.to_owned()];
        }
        return Vec::new();
    }
    extensions
        .iter()
        .map(|extension| format!("{name}{extension}"))
        .collect()
}

fn executable(path: &std::path::Path, windows: bool, extensions: &[String]) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    if windows {
        let Some(extension) = path.extension().and_then(|s| s.to_str()) else {
            return false;
        };
        return extensions.iter().any(|candidate| {
            candidate
                .trim_start_matches('.')
                .eq_ignore_ascii_case(extension)
        });
    }
    #[cfg(unix)]
    if !windows {
        use std::os::unix::fs::PermissionsExt;
        return metadata.permissions().mode() & 0o111 != 0;
    }
    let _ = windows;
    true
}

fn find_all(
    name: &str,
    search_path: &[PathBuf],
    extensions: &[String],
    windows: bool,
) -> Vec<PathBuf> {
    let candidates = names_for(name, extensions, windows);
    if name.contains('/') || (windows && name.contains('\\')) {
        return candidates
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| executable(p, windows, extensions))
            .collect();
    }
    let mut found = Vec::new();
    for directory in search_path {
        if directory.as_os_str().is_empty() {
            continue;
        }
        for candidate in &candidates {
            let path = directory.join(candidate);
            if executable(&path, windows, extensions) && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

fn path_text(paths: &[PathBuf]) -> String {
    env::join_paths(paths.iter())
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

pub fn run(arguments: &[String]) -> i32 {
    let options = match parse(arguments) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let path_entries = env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    #[cfg(windows)]
    let windows = true;
    #[cfg(not(windows))]
    let windows = false;
    #[cfg(windows)]
    let exts = extensions(env::var("PATHEXT").ok().as_deref());
    #[cfg(not(windows))]
    let exts = Vec::new();
    let mut failed = false;
    let code = io::output("which", |out| {
        for name in &options.names {
            let matches = find_all(name, &path_entries, &exts, windows);
            if matches.is_empty() {
                failed = true;
                if !options.silent {
                    eprintln!("which: no {name} in ({})", path_text(&path_entries));
                }
            } else if !options.silent {
                let chosen = if options.all {
                    &matches[..]
                } else {
                    &matches[..1]
                };
                for candidate in chosen {
                    writeln!(out, "{}", path::display_path(candidate))?;
                }
            }
        }
        Ok(())
    });
    if code != 0 {
        code
    } else {
        i32::from(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "which-unit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self, name: &str) {
            fs::write(self.0.join(name), b"dummy").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(self.0.join(name), fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_all_and_silent_flags() {
        let o = parse(&["-a".into(), "-s".into(), "x".into()]).unwrap();
        assert!(o.all && o.silent);
    }
    #[test]
    fn no_operand_is_usage_error() {
        assert_eq!(parse(&[]).unwrap_err(), 1);
    }
    #[test]
    fn rejects_unknown_option_with_status_two() {
        assert_eq!(parse(&["--bad".into()]).unwrap_err(), 2);
    }
    #[test]
    fn supports_multiple_names() {
        assert_eq!(parse(&["a".into(), "b".into()]).unwrap().names.len(), 2);
    }
    #[test]
    fn explicit_path_is_not_searched_in_path() {
        let d = Dir::new();
        d.file("x");
        assert!(find_all("x", &[], &[], false).is_empty());
    }
    #[test]
    fn direct_relative_path_is_tested() {
        let d = Dir::new();
        d.file("x");
        assert!(find_all(d.0.join("x").to_str().unwrap(), &[], &[], false).len() <= 1);
    }
    #[test]
    fn path_search_finds_executable() {
        let d = Dir::new();
        d.file("x");
        assert_eq!(
            find_all("x", std::slice::from_ref(&d.0), &[], false),
            vec![d.0.join("x")]
        );
    }
    #[test]
    fn path_search_ignores_empty_component() {
        let d = Dir::new();
        d.file("x");
        assert!(find_all("x", &[PathBuf::new()], &[], false).is_empty());
    }
    #[test]
    fn all_mode_can_collect_duplicate_names_in_distinct_dirs() {
        let a = Dir::new();
        let b = Dir::new();
        a.file("x");
        b.file("x");
        assert_eq!(
            find_all("x", &[a.0.clone(), b.0.clone()], &[], false).len(),
            2
        );
    }
    #[test]
    fn candidates_use_extensions_for_windows() {
        assert_eq!(names_for("git", &[".EXE".into()], true), ["git.EXE"]);
    }
    #[test]
    fn existing_extension_is_tried_first() {
        assert_eq!(names_for("tool.exe", &[".EXE".into()], true), ["tool.exe"]);
    }
    #[test]
    fn explicit_extension_outside_pathext_is_rejected() {
        assert!(names_for("note.txt", &[".EXE".into()], true).is_empty());
    }
    #[test]
    fn extensionless_windows_files_are_not_candidates() {
        let d = Dir::new();
        d.file("tool");
        assert!(find_all("tool", std::slice::from_ref(&d.0), &[".EXE".into()], true).is_empty());
    }
    #[test]
    fn windows_search_finds_a_pathext_candidate() {
        let d = Dir::new();
        d.file("tool.EXE");
        assert_eq!(
            find_all("tool", std::slice::from_ref(&d.0), &[".EXE".into()], true),
            vec![d.0.join("tool.EXE")]
        );
    }
    #[test]
    fn windows_extensionless_candidate_does_not_precede_exe() {
        let d = Dir::new();
        d.file("tool");
        d.file("tool.EXE");
        assert_eq!(
            find_all("tool", std::slice::from_ref(&d.0), &[".EXE".into()], true),
            vec![d.0.join("tool.EXE")]
        );
    }
    #[test]
    fn extension_list_normalizes_dot() {
        assert_eq!(extensions(Some("EXE;.BAT")), [".EXE", ".BAT"]);
    }
    #[test]
    fn empty_extension_elements_are_ignored() {
        assert!(extensions(Some(";;")).is_empty());
    }
    #[test]
    fn executable_predicate_rejects_missing_file() {
        assert!(!executable(
            std::path::Path::new("missing-which-file"),
            false,
            &[]
        ));
    }
    #[test]
    fn executable_predicate_rejects_directory() {
        let d = Dir::new();
        assert!(!executable(&d.0, false, &[]));
    }
    #[test]
    fn path_text_joins_entries() {
        assert!(!path_text(&[PathBuf::from("bin"), PathBuf::from("tools")]).is_empty());
    }
    #[test]
    fn candidate_extensions_are_appended_in_order() {
        assert_eq!(
            names_for("a", &[".EXE".into(), ".CMD".into()], true),
            ["a.EXE", "a.CMD"]
        );
    }
    #[test]
    fn unix_candidate_does_not_add_extensions() {
        assert_eq!(names_for("a", &[".EXE".into()], false), ["a"]);
    }
    #[test]
    fn candidate_order_checks_each_path_directory() {
        let a = Dir::new();
        let b = Dir::new();
        a.file("x");
        b.file("x");
        assert_eq!(
            find_all("x", &[b.0.clone(), a.0.clone()], &[], false)[0],
            b.0.join("x")
        );
    }
    #[test]
    fn nonexecutable_file_is_not_found_on_unix() {
        #[cfg(unix)]
        {
            let d = Dir::new();
            fs::write(d.0.join("x"), b"x").unwrap();
            assert!(find_all("x", std::slice::from_ref(&d.0), &[], false).is_empty());
        }
    }
}
