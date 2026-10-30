use super::*;

/// A fresh directory under the system temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "agentcmd-mktemp-unit-{tag}-{}-{}",
            std::process::id(),
            random_chars(6)
        ));
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
fn split_template_takes_last_x_run() {
    assert_eq!(
        split_template("tmp.XXXXXXXXXX", None).unwrap(),
        ("tmp.".to_string(), 10, String::new())
    );
    assert_eq!(
        split_template("aXXX.txt", None).unwrap(),
        ("a".to_string(), 3, ".txt".to_string())
    );
    assert_eq!(
        split_template("XXbXXXX", None).unwrap(),
        ("XXb".to_string(), 4, String::new())
    );
}

#[test]
fn split_template_rejects_short_runs_and_slashes() {
    assert!(split_template("XX", None).is_err());
    assert!(split_template("abc", None).is_err());
    assert!(split_template("XXXXX/../b", None).is_err());
    assert!(split_template("abcXXX", Some("a/b")).is_err());
}

#[test]
fn suffix_option_requires_trailing_x() {
    assert_eq!(
        split_template("fooXXXX", Some(".txt")).unwrap(),
        ("foo".to_string(), 4, ".txt".to_string())
    );
    assert!(split_template("fooXXXX.log", Some(".txt")).is_err());
}

#[test]
fn random_chars_use_the_alphabet() {
    let text = random_chars(64);
    assert_eq!(text.len(), 64);
    assert!(text.bytes().all(|b| ALPHABET.contains(&b)));
}

#[test]
fn creates_unique_file_with_template() {
    let dir = TempDir::new("file");
    let options = Options {
        ..Options::default()
    };
    let path = make(&options, &dir.0, "job-XXXXXX").unwrap();
    assert!(path.is_file());
    assert!(path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("job-"));
    assert_eq!(path.file_name().unwrap().len(), "job-".len() + 6);
}

#[test]
fn creates_directory_when_asked() {
    let dir = TempDir::new("dir");
    let options = Options {
        directory: true,
        ..Options::default()
    };
    let path = make(&options, &dir.0, "d.XXXX").unwrap();
    assert!(path.is_dir());
}

#[test]
fn dry_run_creates_nothing() {
    let dir = TempDir::new("dry");
    let options = Options {
        dry_run: true,
        ..Options::default()
    };
    let path = make(&options, &dir.0, "x.XXXX").unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
}

#[test]
fn suffix_is_appended_after_random_part() {
    let dir = TempDir::new("suffix");
    let options = Options {
        suffix: Some(".txt".to_string()),
        ..Options::default()
    };
    let path = make(&options, &dir.0, "name_XXXX").unwrap();
    assert!(path.to_string_lossy().ends_with(".txt"));
    assert!(path.is_file());
}

#[test]
fn missing_directory_is_reported() {
    let options = Options::default();
    let error = make(
        &options,
        Path::new("/definitely/not/here/agentcmd"),
        "a.XXX",
    )
    .unwrap_err();
    assert!(error.starts_with("failed to create file via template 'a.XXX'"));
}

#[test]
fn collect_rejects_extra_templates() {
    let items = vec![
        Item::Operand("a.XXX".to_string()),
        Item::Operand("b.XXX".to_string()),
    ];
    assert_eq!(collect(&items).err(), Some(EXIT_FAIL));
}

#[test]
fn resolve_defaults_to_system_temp_with_tmp_prefix() {
    let options = Options::default();
    let (dir, template) = resolve_template(&options).unwrap();
    assert_eq!(dir, std::env::temp_dir());
    assert_eq!(template, "tmp.XXXXXXXXXX");
}

#[test]
fn resolve_relative_template_stays_in_working_directory() {
    let options = Options {
        template: Some("sub/a.XXX".to_string()),
        ..Options::default()
    };
    let (dir, template) = resolve_template(&options).unwrap();
    assert!(dir.as_os_str().is_empty());
    assert_eq!(template, "sub/a.XXX");
}

#[test]
fn single_component_rejects_slashes() {
    let options = Options {
        single_component: true,
        template: Some("sub/a.XXX".to_string()),
        ..Options::default()
    };
    assert!(resolve_template(&options).is_err());
}
