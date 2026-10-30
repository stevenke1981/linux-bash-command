#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HasArg {
    No,
    Required,
    Optional,
}

/// Expands a unique GNU long-option prefix, retaining an attached value.
pub fn expand_long(arg: &str, options: &[&str]) -> Result<String, String> {
    let long = arg.strip_prefix("--").unwrap_or(arg);
    let (name, value) = long
        .split_once('=')
        .map_or((long, None), |(n, v)| (n, Some(v)));
    let matches: Vec<_> = options
        .iter()
        .copied()
        .filter(|s| s.starts_with(name))
        .collect();
    let full = if matches.contains(&name) {
        name
    } else if matches.len() == 1 {
        matches[0]
    } else if matches.is_empty() {
        return Err(format!("unrecognized option '--{name}'"));
    } else {
        return Err(format!(
            "option '--{name}' is ambiguous; possibilities: {}",
            matches
                .iter()
                .map(|s| format!("'--{s}'"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    };
    Ok(match value {
        Some(value) => format!("--{full}={value}"),
        None => format!("--{full}"),
    })
}

#[derive(Clone, Copy)]
pub struct OptSpec {
    pub id: &'static str,
    pub short: char,
    pub long: &'static str,
    pub has_arg: HasArg,
}

impl OptSpec {
    pub const fn flag(long: &'static str, short: char) -> Self {
        Self {
            id: long,
            short,
            long,
            has_arg: HasArg::No,
        }
    }
    pub const fn value(long: &'static str, short: char) -> Self {
        Self {
            id: long,
            short,
            long,
            has_arg: HasArg::Required,
        }
    }
    pub const fn optional(long: &'static str, short: char) -> Self {
        Self {
            id: long,
            short,
            long,
            has_arg: HasArg::Optional,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Item {
    Opt(&'static str, Option<String>),
    Operand(String),
}

impl Item {
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Opt(_, value) => value.as_deref(),
            Self::Operand(_) => None,
        }
    }
}

pub fn parse(args: &[String], specs: &[OptSpec], permute: bool) -> Result<Vec<Item>, String> {
    parse_with_posix(
        args,
        specs,
        permute && std::env::var_os("POSIXLY_CORRECT").is_none(),
    )
}

/// Parses utility options, consuming required values before recognizing info flags.
pub fn parse_command(
    command: &str,
    args: &[String],
    help: &str,
    specs: &[OptSpec],
    permute: bool,
) -> Result<Vec<Item>, i32> {
    let mut specs = specs.to_vec();
    specs.extend([OptSpec::flag("help", '\0'), OptSpec::flag("version", '\0')]);
    let items = parse(args, &specs, permute).map_err(|e| super::error::usage(command, e, 1))?;
    if let Some(Item::Opt(id, _)) = items
        .iter()
        .find(|item| matches!(item, Item::Opt("help" | "version", _)))
    {
        return Err(help_version(command, &[format!("--{id}")], help, true).unwrap());
    }
    Ok(items)
}

fn parse_with_posix(
    args: &[String],
    specs: &[OptSpec],
    permute: bool,
) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    let mut index = 0;
    let mut stopped = false;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if stopped || arg == "-" || !arg.starts_with('-') {
            items.push(Item::Operand(arg.clone()));
            stopped |= !permute;
            continue;
        }
        if arg == "--" {
            stopped = true;
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, attached) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            let matches: Vec<_> = specs
                .iter()
                .filter(|s| !s.long.is_empty() && s.long.starts_with(name))
                .collect();
            let spec = if let Some(exact) = matches.iter().find(|s| s.long == name) {
                **exact
            } else if matches.len() == 1 {
                *matches[0]
            } else if matches.is_empty() {
                return Err(format!("unrecognized option '--{name}'"));
            } else {
                return Err(format!(
                    "option '--{name}' is ambiguous; possibilities: {}",
                    matches
                        .iter()
                        .map(|s| format!("'--{}'", s.long))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            };
            let value = match (spec.has_arg, attached) {
                (HasArg::No, Some(_)) => {
                    return Err(format!("option '--{name}' doesn't allow an argument"))
                }
                (_, Some(value)) => Some(value.to_owned()),
                (HasArg::Required, None) => {
                    let value = args
                        .get(index)
                        .ok_or_else(|| format!("option '--{name}' requires an argument"))?;
                    index += 1;
                    Some(value.clone())
                }
                _ => None,
            };
            items.push(Item::Opt(spec.id, value));
            if matches!(spec.id, "help" | "version") {
                return Ok(items);
            }
        } else {
            let mut chars = arg[1..].char_indices().peekable();
            while let Some((offset, ch)) = chars.next() {
                let spec = specs
                    .iter()
                    .find(|s| s.short != '\0' && s.short == ch)
                    .ok_or_else(|| format!("invalid option -- '{ch}'"))?;
                let value = if spec.has_arg == HasArg::No {
                    None
                } else {
                    let rest = &arg[1 + offset + ch.len_utf8()..];
                    let value = if !rest.is_empty() {
                        Some(rest.to_owned())
                    } else if spec.has_arg == HasArg::Required {
                        let value = args
                            .get(index)
                            .ok_or_else(|| format!("option requires an argument -- '{ch}'"))?;
                        index += 1;
                        Some(value.clone())
                    } else {
                        None
                    };
                    while chars.next().is_some() {}
                    value
                };
                items.push(Item::Opt(spec.id, value));
                if matches!(spec.id, "help" | "version") {
                    return Ok(items);
                }
            }
        }
    }
    Ok(items)
}

pub fn help_version(command: &str, args: &[String], help: &str, singleton: bool) -> Option<i32> {
    if singleton && args.len() != 1 {
        return None;
    }
    for arg in args {
        match arg.as_str() {
            "--help" => {
                return Some(super::io::output(command, |out| {
                    std::io::Write::write_all(out, help.as_bytes())
                }));
            }
            "--version" => {
                return Some(super::io::output(command, |out| {
                    writeln!(out, "{command} (agentcmd) {}", env!("CARGO_PKG_VERSION"))
                }));
            }
            "--" => break,
            _ if singleton => break,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    const SPECS: &[OptSpec] = &[
        OptSpec::flag("verbose", 'v'),
        OptSpec::value("number", 'n'),
        OptSpec::optional("color", 'c'),
        OptSpec::flag("version", '\0'),
    ];
    fn p(args: &[&str], permute: bool) -> Result<Vec<Item>, String> {
        parse_with_posix(
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            SPECS,
            permute,
        )
    }
    #[test]
    fn bundled() {
        assert_eq!(
            p(&["-vn5"], true).unwrap(),
            [
                Item::Opt("verbose", None),
                Item::Opt("number", Some("5".into()))
            ]
        );
    }
    #[test]
    fn long_value() {
        assert_eq!(p(&["--number=5"], true), p(&["--number", "5"], true));
    }
    #[test]
    fn abbreviation() {
        assert_eq!(p(&["--num=5"], true), p(&["-n5"], true));
    }
    #[test]
    fn ambiguous() {
        assert!(p(&["--ver"], true).unwrap_err().contains("ambiguous"));
    }
    #[test]
    fn dash() {
        assert_eq!(
            p(&["--", "-v", "-"], true).unwrap(),
            [Item::Operand("-v".into()), Item::Operand("-".into())]
        );
    }
    #[test]
    fn optional() {
        assert_eq!(
            p(&["--color", "always"], true).unwrap()[0],
            Item::Opt("color", None)
        );
    }
    #[test]
    fn permutes() {
        assert_eq!(
            p(&["file", "-v"], true).unwrap()[1],
            Item::Opt("verbose", None)
        );
    }
    #[test]
    fn stops() {
        assert_eq!(
            p(&["file", "-v"], false).unwrap()[1],
            Item::Operand("-v".into())
        );
    }
    #[test]
    fn required() {
        assert!(p(&["-n"], true)
            .unwrap_err()
            .contains("requires an argument"));
    }
    #[test]
    fn no_value() {
        assert!(p(&["--verbose=x"], true)
            .unwrap_err()
            .contains("doesn't allow"));
    }
    #[test]
    fn expands_long_prefixes_and_rejects_ambiguity() {
        assert_eq!(
            expand_long("--num=3", &["number", "verbose"]).unwrap(),
            "--number=3"
        );
        assert!(expand_long("--ver", &["verbose", "version"])
            .unwrap_err()
            .contains("ambiguous"));
    }
}
