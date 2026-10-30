use super::*;

/// A fresh directory under the system temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentcmd-realpath-unit-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn normalize_removes_dot_and_dotdot() {
    assert_eq!(
        normalize_lexical(Path::new("/a/./b/../c")),
        PathBuf::from("/a/c")
    );
    assert_eq!(normalize_lexical(Path::new("/../x")), PathBuf::from("/x"));
    assert_eq!(normalize_lexical(Path::new("/a/b/")), PathBuf::from("/a/b"));
}

#[test]
fn relative_path_between_siblings_and_nested() {
    assert_eq!(
        relative_path(Path::new("/a/b/c"), Path::new("/a/b")),
        PathBuf::from("c")
    );
    assert_eq!(
        relative_path(Path::new("/a/x"), Path::new("/a/b/c")),
        PathBuf::from("../../x")
    );
    assert_eq!(
        relative_path(Path::new("/a/b"), Path::new("/a/b")),
        PathBuf::from(".")
    );
}

#[test]
fn relative_path_keeps_target_when_roots_differ() {
    // A target that does not share the base's root (for example another Windows drive)
    // cannot be expressed relative to it, so it is returned unchanged.
    assert_eq!(
        relative_path(Path::new("rel/x"), Path::new("/base")),
        PathBuf::from("rel/x")
    );
}

#[test]
fn split_existing_finds_longest_prefix() {
    let dir = TempDir::new("split");
    fs::create_dir_all(dir.0.join("a")).unwrap();
    let (existing, rest) = split_existing(&dir.0.join("a").join("b").join("c"));
    assert_eq!(existing, dir.0.join("a"));
    assert_eq!(rest, vec![PathBuf::from("b"), PathBuf::from("c")]);
}

#[test]
fn all_but_last_allows_missing_final_component() {
    let dir = TempDir::new("last");
    let target = dir.0.join("new-file");
    let resolved = resolve_all_but_last(&target).unwrap();
    assert_eq!(resolved.file_name(), target.file_name());
}

#[test]
fn all_but_last_rejects_missing_middle_component() {
    let dir = TempDir::new("middle");
    let target = dir.0.join("missing").join("file");
    assert!(resolve_all_but_last(&target).is_err());
}

#[test]
fn missing_mode_accepts_any_path() {
    let dir = TempDir::new("missing");
    let target = dir.0.join("x").join("y");
    let resolved = resolve_missing(&target).unwrap();
    assert!(resolved.ends_with(Path::new("x").join("y")));
}

#[test]
fn existing_mode_requires_the_whole_path() {
    let dir = TempDir::new("exists");
    assert!(resolve_physical(&dir.0.join("nope"), Existence::All).is_err());
    fs::write(dir.0.join("here"), b"x").unwrap();
    assert!(resolve_physical(&dir.0.join("here"), Existence::All).is_ok());
}

#[test]
fn relative_output_rules() {
    let config = RealpathConfig {
        existence: Existence::AllButLast,
        links: Links::Physical,
        quiet: false,
        zero: false,
        relative_to: None,
        relative_base: Some(PathBuf::from("/base")),
        operands: Vec::new(),
    };
    assert_eq!(
        relative_output(Path::new("/base/x/y"), &config),
        PathBuf::from("x/y")
    );
    assert_eq!(
        relative_output(Path::new("/other/y"), &config),
        PathBuf::from("/other/y")
    );
}

#[test]
fn readlink_framing_uses_no_delimiter_with_n() {
    let config = readlink_config(&[Item::Opt {
        id: "no-newline",
        value: None,
    }])
    .unwrap();
    assert!(config.no_newline);
    assert!(!config.zero);
}
