use std::{collections::HashMap, fs, path::PathBuf};

#[test]
fn gnu_case_ids_are_unique_across_all_fixtures() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/gnu_cases");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .expect("read GNU fixtures")
        .map(|entry| entry.expect("read fixture directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no GNU fixtures in {}", dir.display());
    let mut seen = HashMap::new();
    for path in paths {
        let bytes = fs::read(&path).expect("read GNU fixture");
        let fixture: serde_json::Value = serde_json::from_slice(&bytes).expect("parse GNU fixture");
        let command = fixture["command"].as_str().expect("fixture command");
        let cases = fixture["cases"].as_array().expect("fixture cases");
        for case in cases {
            let name = case["name"].as_str().expect("case name");
            let key = (command.to_owned(), name.to_owned());
            if let Some(previous) = seen.insert(key, path.clone()) {
                panic!(
                    "duplicate GNU case {command}/{name} in {} and {}",
                    previous.display(),
                    path.display()
                );
            }
        }
    }
}
