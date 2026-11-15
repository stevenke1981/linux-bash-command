mod common;
use std::collections::BTreeSet;

#[test]
fn every_implemented_phase_help_option_has_a_case() {
    let names = "wc sort uniq cut tr tee echo printf true false test [ seq tac nl paste comm fold expand unexpand yes basename dirname xargs env printenv sleep egrep fgrep mkdir rm rmdir cp mv touch ln link unlink realpath readlink stat du mktemp date md5sum sha1sum sha224sum sha256sum sha384sum sha512sum b2sum cksum sum base64 base32 basenc truncate nproc hostname arch uptime ls find df";
    let pattern = regex::Regex::new(r"--[a-z][a-z0-9-]*").unwrap();
    let cli_cases: serde_json::Value =
        serde_json::from_str(include_str!("cli_option_cases.json")).unwrap();
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
        let data: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).unwrap_or_else(|_| b"{\"cases\":[]}".to_vec()),
        )
        .unwrap();
        let args: Vec<_> = data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["skip_reason"].as_str().is_none_or(str::is_empty))
            .flat_map(|c| {
                c["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap())
            })
            .chain(
                cli_cases
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|case| case["command"].as_str() == Some(name))
                    .flat_map(|case| {
                        case["args"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|arg| arg.as_str().unwrap())
                    }),
            )
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
fn cli_option_cases_verify_documented_gnu_differences() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("cli_option_cases.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let sandbox = common::Sandbox::new();
        for (name, contents) in case["files"].as_object().unwrap() {
            sandbox.write(name, contents.as_str().unwrap());
        }
        let name = case["command"].as_str().unwrap();
        let args: Vec<_> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap())
            .collect();
        let output = sandbox.run(name, &args, b"");
        assert_eq!(
            output.status.code(),
            Some(case["exit"].as_i64().unwrap() as i32),
            "{name}: {output:?}"
        );
        if let Some(expected) = case["stdout"].as_str() {
            assert_eq!(output.stdout, expected.as_bytes(), "{name}");
        } else {
            let pattern = regex::Regex::new(case["stdout_regex"].as_str().unwrap()).unwrap();
            assert!(
                pattern.is_match(std::str::from_utf8(&output.stdout).unwrap()),
                "{name}: {output:?}"
            );
        }
        if let Some(expected) = case["stderr_contains"].as_str() {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(expected),
                "{name}: {output:?}"
            );
        } else {
            assert!(output.stderr.is_empty(), "{name}: {output:?}");
        }
    }
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
