mod common;
use std::collections::BTreeSet;

#[test]
fn every_phase1_help_option_has_a_case() {
    let names = "wc sort uniq cut tr tee echo printf true false test [ seq tac nl paste comm fold expand unexpand yes basename dirname xargs env printenv sleep egrep fgrep";
    let pattern = regex::Regex::new(r"--[a-z][a-z0-9-]*").unwrap();
    let mut missing = Vec::new();
    for name in names.split_whitespace().filter(|n| *n != "test") {
        let args = if name == "[" {
            vec!["[", "--help"]
        } else {
            vec![name, "--help"]
        };
        let output = common::run_multicall(&args, b"");
        assert!(
            !output.stdout.is_empty(),
            "{name} --help did not print help"
        );
        let text = String::from_utf8(output.stdout).unwrap();
        let options: BTreeSet<_> = pattern.find_iter(&text).map(|m| m.as_str()).collect();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/gnu_cases/{name}.json"));
        let data: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let args: Vec<_> = data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|c| {
                c["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap())
            })
            .collect();
        for option in options {
            if !args
                .iter()
                .any(|a| *a == option || a.strip_prefix(option).is_some_and(|s| s.starts_with('=')))
            {
                missing.push(format!("{name}: {option}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "untested help options: {}",
        missing.join(", ")
    );
}

#[test]
fn registry_entries_have_standalone_binaries() {
    for utility in agentcmd::registry::UTILITIES {
        if utility.name != "[" {
            assert!(
                std::path::Path::new(common::binary(utility.name)).is_file(),
                "{}",
                utility.name
            );
        }
    }
}
