//! GNU `getopt_long`-compatible argument parsing.
//!
//! Two entry points serve different needs:
//! - [`Parser`] parses a full option table (`OptSpec`) into [`Item`]s. New commands use it.
//! - [`expand_long_abbrev`] and [`expand_legacy`] normalise arguments in front of a
//!   hand-written parser, so existing commands gain unique-prefix long options and
//!   `POSIXLY_CORRECT` without a rewrite.

use std::fmt;

/// Whether an option takes a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HasArg {
    No,
    /// The value is the rest of the short cluster, or the next argument, or `=VALUE`.
    Required,
    /// The value is only accepted as `=VALUE` (long) or as the rest of the cluster (short).
    Optional,
}

/// One option in a command's table.
#[derive(Clone, Copy, Debug)]
pub struct OptSpec {
    pub id: &'static str,
    pub short: Option<char>,
    pub long: Option<&'static str>,
    pub has_arg: HasArg,
}

impl OptSpec {
    pub const fn flag(id: &'static str, short: Option<char>, long: Option<&'static str>) -> Self {
        Self {
            id,
            short,
            long,
            has_arg: HasArg::No,
        }
    }

    pub const fn req(id: &'static str, short: Option<char>, long: Option<&'static str>) -> Self {
        Self {
            id,
            short,
            long,
            has_arg: HasArg::Required,
        }
    }

    pub const fn opt(id: &'static str, short: Option<char>, long: Option<&'static str>) -> Self {
        Self {
            id,
            short,
            long,
            has_arg: HasArg::Optional,
        }
    }
}

/// A parsed command line element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Opt {
        id: &'static str,
        value: Option<String>,
    },
    Operand(String),
}

/// A usage error. `message` omits the `cmd: ` prefix; see [`report_usage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    pub message: String,
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Parser over a fixed option table.
pub struct Parser<'a> {
    pub cmd: &'a str,
    pub specs: &'a [OptSpec],
    /// Allow options after operands (GNU permutation). Disabled by `POSIXLY_CORRECT`.
    pub permute: bool,
    /// Treat `-5`-style arguments as operands instead of invalid options.
    pub numeric_short: bool,
}

impl<'a> Parser<'a> {
    pub fn new(cmd: &'a str, specs: &'a [OptSpec]) -> Self {
        Self {
            cmd,
            specs,
            permute: true,
            numeric_short: false,
        }
    }

    /// Stops option parsing at the first operand, like `POSIXLY_CORRECT`.
    pub fn no_permute(mut self) -> Self {
        self.permute = false;
        self
    }

    /// Parses `args` (without the command name). Permutation is disabled when the
    /// `POSIXLY_CORRECT` environment variable is set.
    pub fn parse(&self, args: &[String]) -> Result<Vec<Item>, UsageError> {
        let permute = self.permute && std::env::var_os("POSIXLY_CORRECT").is_none();
        self.parse_with(args, permute)
    }

    fn parse_with(&self, args: &[String], permute: bool) -> Result<Vec<Item>, UsageError> {
        let mut items = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            index += 1;
            if arg == "--" {
                push_operands(&mut items, &args[index..]);
                break;
            }
            if is_operand(arg, self.numeric_short) {
                items.push(Item::Operand(arg.to_owned()));
                if !permute {
                    push_operands(&mut items, &args[index..]);
                    break;
                }
            } else if let Some(body) = arg.strip_prefix("--") {
                self.parse_long(body, args, &mut index, &mut items)?;
            } else {
                self.parse_cluster(&arg[1..], args, &mut index, &mut items)?;
            }
        }
        Ok(items)
    }

    fn parse_long(
        &self,
        body: &str,
        args: &[String],
        index: &mut usize,
        items: &mut Vec<Item>,
    ) -> Result<(), UsageError> {
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (body, None),
        };
        let spec = self.find_long(name, body)?;
        let value = match spec.has_arg {
            HasArg::No => {
                if inline.is_some() {
                    return Err(UsageError {
                        message: format!(
                            "option '--{}' doesn't allow an argument",
                            spec.long.unwrap_or_default()
                        ),
                    });
                }
                None
            }
            HasArg::Required => match inline {
                Some(value) => Some(value),
                None => Some(take_next(args, index).ok_or_else(|| UsageError {
                    message: format!(
                        "option '--{}' requires an argument",
                        spec.long.unwrap_or_default()
                    ),
                })?),
            },
            HasArg::Optional => inline,
        };
        items.push(Item::Opt { id: spec.id, value });
        Ok(())
    }

    /// Finds a long option by exact name, then by unique prefix. `shown` is the option text
    /// (without a value) used in messages.
    fn find_long(&self, name: &str, shown: &str) -> Result<&'a OptSpec, UsageError> {
        if let Some(spec) = self.specs.iter().find(|s| s.long == Some(name)) {
            return Ok(spec);
        }
        let candidates: Vec<&OptSpec> = self
            .specs
            .iter()
            .filter(|s| !name.is_empty() && s.long.is_some_and(|long| long.starts_with(name)))
            .collect();
        match candidates.as_slice() {
            [] => Err(UsageError {
                message: format!("unrecognized option '--{shown}'"),
            }),
            [only, rest @ ..] if rest.iter().all(|s| s.id == only.id) => Ok(*only),
            many => {
                let names: Vec<&str> = many.iter().filter_map(|s| s.long).collect();
                Err(UsageError {
                    message: ambiguous_message(name, &names),
                })
            }
        }
    }

    fn parse_cluster(
        &self,
        flags: &str,
        args: &[String],
        index: &mut usize,
        items: &mut Vec<Item>,
    ) -> Result<(), UsageError> {
        for (position, letter) in flags.char_indices() {
            let spec = self
                .specs
                .iter()
                .find(|s| s.short == Some(letter))
                .ok_or_else(|| UsageError {
                    message: format!("invalid option -- '{letter}'"),
                })?;
            if spec.has_arg == HasArg::No {
                items.push(Item::Opt {
                    id: spec.id,
                    value: None,
                });
                continue;
            }
            let rest = &flags[position + letter.len_utf8()..];
            let value = if !rest.is_empty() {
                Some(rest.to_owned())
            } else if spec.has_arg == HasArg::Required {
                Some(take_next(args, index).ok_or_else(|| UsageError {
                    message: format!("option requires an argument -- '{letter}'"),
                })?)
            } else {
                None
            };
            items.push(Item::Opt { id: spec.id, value });
            break;
        }
        Ok(())
    }
}

fn is_operand(arg: &str, numeric_short: bool) -> bool {
    if arg == "-" || !arg.starts_with('-') {
        return true;
    }
    numeric_short && arg.len() > 1 && arg[1..].bytes().all(|b| b.is_ascii_digit())
}

fn push_operands(items: &mut Vec<Item>, rest: &[String]) {
    items.extend(rest.iter().map(|arg| Item::Operand(arg.clone())));
}

fn take_next(args: &[String], index: &mut usize) -> Option<String> {
    let value = args.get(*index)?.clone();
    *index += 1;
    Some(value)
}

fn ambiguous_message(name: &str, candidates: &[&str]) -> String {
    let listed: Vec<String> = candidates.iter().map(|c| format!("'--{c}'")).collect();
    format!(
        "option '--{name}' is ambiguous; possibilities: {}",
        listed.join(" ")
    )
}

/// Prints `cmd: MESSAGE` and the `Try` line to stderr, then returns `code`.
pub fn report_usage(cmd: &str, err: &UsageError, code: i32) -> i32 {
    eprintln!(
        "{cmd}: {}\nTry '{cmd} --help' for more information.",
        err.message
    );
    code
}

/// Expands a unique long-option prefix such as `--line-n` to `--line-number`, keeping any
/// `=VALUE`. An exact name wins over a prefix. Unknown names are returned unchanged so the
/// caller's parser reports them. Ambiguity returns the GNU message (without `cmd: `).
pub fn expand_long_abbrev(arg: &str, longs: &[&str]) -> Result<String, String> {
    let Some(body) = arg.strip_prefix("--") else {
        return Ok(arg.to_owned());
    };
    let (name, rest) = match body.find('=') {
        Some(index) => (&body[..index], &body[index..]),
        None => (body, ""),
    };
    if name.is_empty() || longs.contains(&name) {
        return Ok(arg.to_owned());
    }
    let candidates: Vec<&str> = longs
        .iter()
        .copied()
        .filter(|long| long.starts_with(name))
        .collect();
    match candidates.as_slice() {
        [] => Ok(arg.to_owned()),
        [only] => Ok(format!("--{only}{rest}")),
        many => Err(ambiguous_message(name, many)),
    }
}

/// Normalises a legacy command line before its hand-written parser runs.
///
/// - Long options are expanded with [`expand_long_abbrev`].
/// - `value_longs` lists long options (without `--`) that take a separate next argument, and
///   `value_shorts` lists short letters that do; their values are passed through unchanged
///   so they are never mistaken for operands.
/// - Under `POSIXLY_CORRECT`, a `--` is inserted before the first operand so that later
///   option-looking arguments are operands, as POSIX requires.
pub fn expand_legacy(
    args: &[String],
    longs: &[&str],
    value_longs: &[&str],
    value_shorts: &str,
) -> Result<Vec<String>, String> {
    let posix = std::env::var_os("POSIXLY_CORRECT").is_some();
    expand_legacy_with(args, longs, value_longs, value_shorts, posix)
}

fn expand_legacy_with(
    args: &[String],
    longs: &[&str],
    value_longs: &[&str],
    value_shorts: &str,
    posix: bool,
) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if arg == "--" {
            out.push(arg.to_owned());
            out.extend(args[index..].iter().cloned());
            break;
        }
        if !is_operand(arg, false) {
            if arg.starts_with("--") {
                let expanded = expand_long_abbrev(arg, longs)?;
                let name = expanded[2..].split('=').next().unwrap_or_default();
                let takes_next = !expanded.contains('=') && value_longs.contains(&name);
                out.push(expanded);
                if takes_next {
                    if let Some(value) = args.get(index) {
                        out.push(value.clone());
                        index += 1;
                    }
                }
            } else {
                out.push(arg.to_owned());
                take_short_value(arg, value_shorts, args, &mut index, &mut out);
            }
            continue;
        }
        if posix {
            out.push("--".to_owned());
            out.extend(args[index - 1..].iter().cloned());
            break;
        }
        out.push(arg.to_owned());
    }
    Ok(out)
}

/// Expands long-option abbreviations in each argument up to a `--` and leaves everything
/// else alone. Unlike [`expand_legacy`] it never reorders for `POSIXLY_CORRECT`, which suits
/// commands whose operands can look like options (`ps aux`).
pub fn expand_abbrevs(args: &[String], longs: &[&str]) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(args.len());
    for (index, arg) in args.iter().enumerate() {
        if arg == "--" {
            out.extend(args[index..].iter().cloned());
            break;
        }
        out.push(expand_long_abbrev(arg, longs)?);
    }
    Ok(out)
}

/// [`expand_abbrevs`] for a command's entry point, reporting an ambiguous option with
/// `usage_code` as the status.
pub fn prepare_abbrevs(
    cmd: &str,
    args: &[String],
    longs: &[&str],
    usage_code: i32,
) -> Result<Vec<String>, i32> {
    expand_abbrevs(args, longs)
        .map_err(|message| report_usage(cmd, &UsageError { message }, usage_code))
}

/// [`expand_legacy`] for a command's entry point. An ambiguous long option is reported
/// as a usage error and `usage_code` is returned as the command's status.
pub fn prepare_legacy(
    cmd: &str,
    args: &[String],
    longs: &[&str],
    value_longs: &[&str],
    value_shorts: &str,
    usage_code: i32,
) -> Result<Vec<String>, i32> {
    expand_legacy(args, longs, value_longs, value_shorts)
        .map_err(|message| report_usage(cmd, &UsageError { message }, usage_code))
}

/// Copies the separate value of a short option whose letter takes one, if it is last in its
/// cluster (`-n 5`). `-n5` and `-vn5` keep their value inside the same argument.
fn take_short_value(
    arg: &str,
    value_shorts: &str,
    args: &[String],
    index: &mut usize,
    out: &mut Vec<String>,
) {
    for (position, letter) in arg[1..].char_indices() {
        if !value_shorts.contains(letter) {
            continue;
        }
        if 1 + position + letter.len_utf8() == arg.len() {
            if let Some(value) = args.get(*index) {
                out.push(value.clone());
                *index += 1;
            }
        }
        break;
    }
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
