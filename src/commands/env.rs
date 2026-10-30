use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error, io, process,
};
use std::ffi::{OsStr, OsString};

const HELP:&str="Usage: env [OPTION]... [-] [NAME=VALUE]... [COMMAND [ARG]...]\nRun COMMAND in a modified environment, or print the environment.\n  -i, --ignore-environment  Start with an empty environment\n  -u, --unset=NAME          Remove variable NAME\n  -0, --null                End printed entries with NUL\n  -C, --chdir=DIR           Change working directory\n  -S, --split-string=S      Split S into arguments\n  -v, --debug               Explain processing on stderr\n      --help                Display this help\n      --version             Display version\nExit status: 125 on an env error, 126 if command cannot execute, 127 if absent; otherwise command status.\n";
const SPECS: &[OptSpec] = &[
    OptSpec::flag("ignore-environment", 'i'),
    OptSpec::value("unset", 'u'),
    OptSpec::flag("null", '0'),
    OptSpec::value("chdir", 'C'),
    OptSpec::value("split-string", 'S'),
    OptSpec::flag("debug", 'v'),
    OptSpec::flag("help", '\0'),
    OptSpec::flag("version", '\0'),
];

pub fn split(text: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut quote = None;
    let mut active = false;
    let mut token = String::new();
    let mut tokens = Vec::new();
    while i < chars.len() {
        let ch = chars[i];
        i += 1;
        if ch == '#' && quote.is_none() && !active {
            break;
        }
        if ch.is_ascii_whitespace() && quote.is_none() {
            if active {
                tokens.push(std::mem::take(&mut token));
                active = false;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            } else {
                token.push(ch);
            }
            active = true;
            continue;
        }
        if ch == '$' && quote != Some('\'') {
            if chars.get(i) != Some(&'{') {
                return Err("only ${VARNAME} expansion is supported".into());
            }
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '}' {
                i += 1;
            }
            if i == chars.len() {
                return Err("missing '}' in variable expansion".into());
            }
            let name: String = chars[start..i].iter().collect();
            i += 1;
            token.push_str(&std::env::var(name).unwrap_or_default());
            active = true;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            let escaped = *chars.get(i).ok_or("invalid trailing backslash in -S")?;
            i += 1;
            if escaped == 'c' {
                if quote.is_some() {
                    return Err("'\\c' must not appear in double-quoted -S string".into());
                }
                break;
            }
            if escaped == '_' && quote.is_none() {
                if active {
                    tokens.push(std::mem::take(&mut token));
                    active = false;
                }
                continue;
            }
            token.push(match escaped {
                '_' => ' ',
                'a' => '\x07',
                'b' => '\x08',
                'f' => '\x0c',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'v' => '\x0b',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                '#' => '#',
                '$' => '$',
                _ => return Err(format!("invalid sequence '\\{escaped}' in -S")),
            });
            active = true;
            continue;
        }
        token.push(ch);
        active = true;
    }
    if quote.is_some() {
        return Err("no terminating quote in -S string".into());
    }
    if active {
        tokens.push(token);
    }
    Ok(tokens)
}

fn parsed(arguments: &[String]) -> Result<Vec<Item>, String> {
    let mut flat = arguments.to_vec();
    let mut index = 0;
    // Expand in the original token stream so -- and command boundaries survive.
    // The shared parser below owns validation of all remaining options.
    'options: while let Some(arg) = flat.get(index) {
        if arg == "--" || arg == "-" || !arg.starts_with('-') {
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, attached) = long
                .split_once('=')
                .map_or((long, None), |(name, value)| (name, Some(value)));
            let mut matches = SPECS.iter().filter(|s| s.long.starts_with(name));
            let spec = SPECS.iter().find(|s| s.long == name).or_else(|| {
                let first = matches.next()?;
                matches.next().is_none().then_some(first)
            });
            let Some(spec) = spec else { break };
            if matches!(spec.id, "help" | "version")
                || spec.has_arg == HasArg::No && attached.is_some()
            {
                break;
            }
            let consumed = 1 + usize::from(spec.has_arg == HasArg::Required && attached.is_none());
            if spec.id == "split-string" {
                let Some(value) = attached.or_else(|| flat.get(index + 1).map(String::as_str))
                else {
                    break;
                };
                let expanded = split(value)?;
                flat.splice(index..index + consumed, expanded);
                continue;
            }
            index += consumed;
        } else {
            for (offset, ch) in arg[1..].char_indices() {
                let Some(spec) = SPECS.iter().find(|s| s.short == ch) else {
                    break 'options;
                };
                if spec.has_arg == HasArg::No {
                    continue;
                }
                let rest = &arg[1 + offset + ch.len_utf8()..];
                let consumed = 1 + usize::from(rest.is_empty());
                if spec.id == "split-string" {
                    let Some(value) = (!rest.is_empty())
                        .then_some(rest)
                        .or_else(|| flat.get(index + 1).map(String::as_str))
                    else {
                        break 'options;
                    };
                    let mut expanded = split(value)?;
                    if offset != 0 {
                        expanded.insert(0, arg[..1 + offset].to_owned());
                    }
                    flat.splice(index..index + consumed, expanded);
                    continue 'options;
                }
                index += consumed - 1;
                break;
            }
            index += 1;
        }
    }
    args::parse(&flat, SPECS, false)
}

pub fn run(arguments: &[String]) -> i32 {
    let items = match parsed(arguments) {
        Ok(v) => v,
        Err(e) => return error::usage("env", e, 125),
    };
    let mut environment: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let mut operands = Vec::new();
    let mut directory = None;
    let mut zero = false;
    let mut debug = false;
    for item in items {
        match item {
            Item::Opt("ignore-environment", _) => environment.clear(),
            Item::Opt("unset", Some(name)) => {
                if name.contains('=') {
                    return error::usage(
                        "env",
                        format!("cannot unset '{name}': Invalid argument"),
                        125,
                    );
                }
                environment.retain(|(k, _)| !process::env_name_eq(k, OsStr::new(&name)));
            }
            Item::Opt("chdir", value) => directory = value,
            Item::Opt("null", _) => zero = true,
            Item::Opt("debug", _) => debug = true,
            Item::Opt("help", _) => return io::output("env", |out| out.write_all(HELP.as_bytes())),
            Item::Opt("version", _) => {
                return args::help_version("env", &["--version".into()], HELP, true).unwrap()
            }
            Item::Operand(s) => operands.push(s),
            _ => unreachable!(),
        }
    }
    if operands.first().is_some_and(|s| s == "-") {
        environment.clear();
        operands.remove(0);
    }
    let mut command_start = 0;
    for operand in &operands {
        let Some((name, value)) = operand.split_once('=') else {
            break;
        };
        if let Some((_, entry)) = environment
            .iter_mut()
            .find(|(k, _)| process::env_name_eq(k, OsStr::new(name)))
        {
            *entry = value.into();
        } else {
            environment.push((name.into(), value.into()));
        }
        command_start += 1;
    }
    if command_start == operands.len() {
        if directory.is_some() {
            return error::usage("env", "must specify command with --chdir (-C)", 125);
        }
        return io::output("env", |out| {
            for (k, v) in &environment {
                out.write_all(k.as_encoded_bytes())?;
                out.write_all(b"=")?;
                out.write_all(v.as_encoded_bytes())?;
                out.write_all(if zero { b"\0" } else { b"\n" })?;
            }
            Ok(())
        });
    }
    if zero {
        return error::usage("env", "cannot specify --null (-0) with command", 125);
    }
    let name = &operands[command_start];
    if debug {
        eprintln!("executing: {name}");
        for (index, arg) in operands[command_start..].iter().enumerate() {
            eprintln!("   arg[{index}]= {arg:?}");
        }
    }
    if let Some(ref path) = directory {
        let checked = std::fs::metadata(path).and_then(|m| {
            if m.is_dir() {
                Ok(())
            } else {
                Err(std::io::Error::from(std::io::ErrorKind::NotADirectory))
            }
        });
        #[cfg(unix)]
        let checked = checked.and_then(|()| {
            let path = std::ffi::CString::new(path.as_bytes())
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
            if unsafe { libc::access(path.as_ptr(), libc::X_OK) } == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
        if let Err(e) = checked {
            eprintln!(
                "env: cannot change directory to '{path}': {}",
                error::io_text(&e)
            );
            return 125;
        }
    }
    let mut child = match process::command_in(
        OsStr::new(name),
        &environment,
        directory.as_deref().map(std::path::Path::new),
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("env: '{name}': {}", error::io_text(&e));
            return process::error_code(&e);
        }
    };
    child.args(&operands[command_start + 1..]);
    match child.status() {
        Ok(s) => process::status_code(s),
        Err(e) => {
            eprintln!("env: '{name}': {}", error::io_text(&e));
            process::error_code(&e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn words() {
        assert_eq!(split("one two").unwrap(), ["one", "two"]);
    }
    #[test]
    fn quotes() {
        assert_eq!(
            split("'one two' \"three four\" ''").unwrap(),
            ["one two", "three four", ""]
        );
    }
    #[test]
    fn escapes() {
        assert_eq!(split(r#"a\_b "c\_d""#).unwrap(), ["a", "b", "c d"]);
    }
    #[test]
    fn comment() {
        assert_eq!(split("one # two").unwrap(), ["one"]);
    }
    #[test]
    fn unmatched() {
        assert!(split("'one").is_err());
    }
    #[test]
    fn expansion() {
        assert_eq!(split("${AGENTCMD_NONEXISTENT_XXX}").unwrap(), [""]);
    }
    #[test]
    fn stops_at_command() {
        let result = parsed(&["echo".into(), "--null".into()]).unwrap();
        assert!(matches!(&result[1],Item::Operand(s)if s=="--null"));
    }
    #[test]
    fn split_preserves_terminator() {
        let result = parsed(&["-S".into(), "".into(), "--".into(), "-i".into()]).unwrap();
        assert_eq!(result, [Item::Operand("-i".into())]);
    }
    #[test]
    fn split_preserves_option_value() {
        let result = parsed(&["-S".into(), "-u".into(), "NAME".into()]).unwrap();
        assert_eq!(result, [Item::Opt("unset", Some("NAME".into()))]);
    }
    #[test]
    fn split_preserves_short_bundle() {
        let result = parsed(&["-vS-i".into(), "A=1".into()]).unwrap();
        assert_eq!(
            result,
            [
                Item::Opt("debug", None),
                Item::Opt("ignore-environment", None),
                Item::Operand("A=1".into())
            ]
        );
    }
    #[test]
    fn split_inserts_command_boundary() {
        let result = parsed(&["--split-s=tool".into(), "--unknown".into()]).unwrap();
        assert_eq!(
            result,
            [
                Item::Operand("tool".into()),
                Item::Operand("--unknown".into())
            ]
        );
    }
}
