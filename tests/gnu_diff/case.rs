//! Loading and validating `tests/gnu_cases/*.json` (06-testing.md §2, §2.1).

use serde_json::{Map, Value};
use std::fs;
use std::path::Path;
use std::time::Duration;

const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_COMPARE: &[&str] = &["stdout", "exit"];
const KNOWN_COMPARE: &[&str] = &["stdout", "stdout_sorted", "exit", "files", "stderr_empty"];
const CASE_KEYS: &[&str] = &[
    "name",
    "args",
    "stdin",
    "stdin_hex",
    "files",
    "env",
    "gnu",
    "compare",
    "timeout_ms",
    "skip_reason",
];
const FILE_KEYS: &[&str] = &["hex", "dir", "symlink", "text", "mtime", "mode"];

#[derive(Debug)]
pub struct Case {
    pub file: String,
    pub command: String,
    pub name: String,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub files: Vec<(String, FileSpec)>,
    pub env: Vec<(String, String)>,
    /// Program name followed by fixed leading arguments (e.g. `["gawk", "--posix"]`).
    pub gnu: Vec<String>,
    pub compare: Vec<String>,
    pub timeout: Duration,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    File(Vec<u8>),
    Dir,
    Symlink(String),
}

#[derive(Debug)]
pub struct FileSpec {
    pub content: Content,
    /// Modification time in seconds since the Unix epoch.
    pub mtime: Option<u64>,
    /// Unix permission bits (octal in JSON, e.g. `"644"`).
    pub mode: Option<u32>,
}

impl Case {
    pub fn label(&self) -> String {
        format!("{}/{}", self.command, self.name)
    }

    pub fn wants(&self, item: &str) -> bool {
        self.compare.iter().any(|c| c == item)
    }
}

/// Loads every `*.json` file in `dir`, sorted by file name.
pub fn load_all(dir: &Path) -> Vec<Case> {
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("read_dir entry").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no case files in {}", dir.display());
    paths
        .iter()
        .flat_map(|path| {
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            let text =
                fs::read_to_string(path).unwrap_or_else(|e| panic!("{file}: cannot read: {e}"));
            parse_file(&file, &text)
        })
        .collect()
}

fn parse_file(file: &str, text: &str) -> Vec<Case> {
    let root: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{file}: {e}"));
    let root = object(&root, file);
    reject_unknown(root, &["command", "cases"], file);
    let command = required_str(root, "command", file);
    let cases = root
        .get("cases")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{file}: \"cases\" must be an array"));
    let mut names = Vec::new();
    cases
        .iter()
        .map(|value| {
            let case = parse_case(file, &command, value);
            assert!(
                !names.contains(&case.name),
                "{file}: duplicate case name {:?}",
                case.name
            );
            names.push(case.name.clone());
            case
        })
        .collect()
}

fn parse_case(file: &str, command: &str, value: &Value) -> Case {
    let obj = object(value, &format!("{file}: case"));
    let name = required_str(obj, "name", file);
    let ctx = format!("{file}: {command}/{name}");
    reject_unknown(obj, CASE_KEYS, &ctx);

    let stdin = match (obj.get("stdin"), obj.get("stdin_hex")) {
        (Some(_), Some(_)) => panic!("{ctx}: \"stdin\" and \"stdin_hex\" are mutually exclusive"),
        (Some(text), None) => string(text, &ctx).as_bytes().to_vec(),
        (None, Some(hex)) => {
            hex_decode(string(hex, &ctx)).unwrap_or_else(|e| panic!("{ctx}: stdin_hex: {e}"))
        }
        (None, None) => Vec::new(),
    };

    let files = match obj.get("files") {
        None => Vec::new(),
        Some(value) => object(value, &ctx)
            .iter()
            .map(|(path, spec)| (path.clone(), parse_file_spec(&ctx, path, spec)))
            .collect(),
    };

    let env = match obj.get("env") {
        None => Vec::new(),
        Some(value) => object(value, &ctx)
            .iter()
            .map(|(k, v)| (k.clone(), string(v, &ctx).to_owned()))
            .collect(),
    };

    let gnu = opt_str_list(obj, "gnu", &ctx).unwrap_or_else(|| vec![command.to_owned()]);
    assert!(!gnu.is_empty(), "{ctx}: \"gnu\" must not be empty");
    assert!(
        !gnu[0].contains('/'),
        "{ctx}: \"gnu\" program must be a bare name resolved in /usr/bin or /bin"
    );

    let compare = opt_str_list(obj, "compare", &ctx)
        .unwrap_or_else(|| DEFAULT_COMPARE.iter().map(|s| (*s).to_owned()).collect());
    for item in &compare {
        assert!(
            KNOWN_COMPARE.contains(&item.as_str()),
            "{ctx}: unknown compare item {item:?}"
        );
    }

    let timeout_ms = match obj.get("timeout_ms") {
        None => DEFAULT_TIMEOUT_MS,
        Some(v) => v
            .as_u64()
            .unwrap_or_else(|| panic!("{ctx}: \"timeout_ms\" must be a positive integer")),
    };

    let skip_reason = match obj.get("skip_reason") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let text = string(v, &ctx);
            assert!(
                !text.trim().is_empty(),
                "{ctx}: empty skip_reason; use null"
            );
            Some(text.to_owned())
        }
    };

    Case {
        file: file.to_owned(),
        command: command.to_owned(),
        name,
        args: opt_str_list(obj, "args", &ctx).unwrap_or_default(),
        stdin,
        files,
        env,
        gnu,
        compare,
        timeout: Duration::from_millis(timeout_ms),
        skip_reason,
    }
}

fn parse_file_spec(ctx: &str, path: &str, value: &Value) -> FileSpec {
    let ctx = format!("{ctx}: files[{path:?}]");
    if let Some(text) = value.as_str() {
        return FileSpec {
            content: Content::File(text.as_bytes().to_vec()),
            mtime: None,
            mode: None,
        };
    }
    let obj = object(value, &ctx);
    reject_unknown(obj, FILE_KEYS, &ctx);
    let content = if let Some(hex) = obj.get("hex") {
        Content::File(hex_decode(string(hex, &ctx)).unwrap_or_else(|e| panic!("{ctx}: hex: {e}")))
    } else if let Some(text) = obj.get("text") {
        Content::File(string(text, &ctx).as_bytes().to_vec())
    } else if let Some(target) = obj.get("symlink") {
        Content::Symlink(string(target, &ctx).to_owned())
    } else if obj.get("dir") == Some(&Value::Bool(true)) {
        Content::Dir
    } else {
        panic!("{ctx}: expected one of text, hex, symlink or dir:true");
    };
    if obj.contains_key("dir") && obj.get("dir") != Some(&Value::Bool(true)) {
        panic!("{ctx}: \"dir\" must be true");
    }
    let mtime = obj.get("mtime").map(|v| {
        v.as_u64()
            .unwrap_or_else(|| panic!("{ctx}: \"mtime\" must be unsigned seconds"))
    });
    let mode = obj.get("mode").map(|v| {
        u32::from_str_radix(string(v, &ctx), 8)
            .unwrap_or_else(|_| panic!("{ctx}: \"mode\" must be octal digits"))
    });
    FileSpec {
        content,
        mtime,
        mode,
    }
}

/// Decodes a hex string (case-insensitive, no separators). Hand-written to avoid a dependency.
pub fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    let digits = text.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return Err(format!("odd number of hex digits in {text:?}"));
    }
    digits
        .chunks(2)
        .map(|pair| Ok(nibble(pair[0])? << 4 | nibble(pair[1])?))
        .collect()
}

fn nibble(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("invalid hex digit {:?}", c as char)),
    }
}

fn object<'a>(value: &'a Value, ctx: &str) -> &'a Map<String, Value> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("{ctx}: expected an object"))
}

fn string<'a>(value: &'a Value, ctx: &str) -> &'a str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("{ctx}: expected a string"))
}

fn required_str(obj: &Map<String, Value>, key: &str, ctx: &str) -> String {
    let value = obj
        .get(key)
        .unwrap_or_else(|| panic!("{ctx}: missing \"{key}\""));
    string(value, ctx).to_owned()
}

fn opt_str_list(obj: &Map<String, Value>, key: &str, ctx: &str) -> Option<Vec<String>> {
    obj.get(key).map(|value| {
        value
            .as_array()
            .unwrap_or_else(|| panic!("{ctx}: \"{key}\" must be an array of strings"))
            .iter()
            .map(|item| string(item, ctx).to_owned())
            .collect()
    })
}

fn reject_unknown(obj: &Map<String, Value>, allowed: &[&str], ctx: &str) {
    for key in obj.keys() {
        assert!(
            allowed.contains(&key.as_str()),
            "{ctx}: unknown field {key:?} (allowed: {allowed:?})"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::hex_decode;

    #[test]
    fn hex_decodes_both_cases_and_rejects_bad_input() {
        assert_eq!(hex_decode("00ffA0").unwrap(), vec![0x00, 0xff, 0xa0]);
        assert!(hex_decode("abc").is_err());
        assert!(hex_decode("zz").is_err());
    }
}
