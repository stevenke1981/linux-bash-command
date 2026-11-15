//! Turns find's command-line arguments into a `Plan`: starting points, global options,
//! and the expression tree. Operator precedence follows GNU find:
//! `!` binds tightest, then `-a` (also implied between adjacent terms), then `-o`,
//! then `,` (lowest).

use super::expr::{
    time_ns, Access, Action, Clock, Cmp, Expr, NumberField, PermCheck, Test, DAY_NS, MINUTE_NS,
};
use super::glob::Glob;
use super::output::{Destination, OutputFile};
use super::printf;
use super::walk::message;
use regex::RegexBuilder;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follow {
    /// `-P` (default): never follow symbolic links.
    Never,
    /// `-H`: follow symbolic links only for the starting points.
    RootsOnly,
    /// `-L`: follow all symbolic links, guarding against loops.
    Always,
}

#[derive(Clone, Copy)]
pub struct Options {
    pub min_depth: usize,
    pub max_depth: usize,
    /// `-depth`: visit directory contents before the directory itself.
    pub depth_first: bool,
    pub follow: Follow,
    pub one_fs: bool,
    pub ignore_race: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min_depth: 0,
            max_depth: usize::MAX,
            depth_first: false,
            follow: Follow::Never,
            one_fs: false,
            ignore_race: false,
        }
    }
}

/// A command queued by `-exec ... {} +` until it is run with many paths.
pub struct BatchSpec {
    pub argv: Vec<String>,
    pub in_dir: bool,
}

pub struct Plan {
    pub roots: Vec<PathBuf>,
    pub options: Options,
    pub expr: Expr,
    pub batches: Vec<BatchSpec>,
    pub files: Vec<OutputFile>,
}

pub fn parse(args: &[String]) -> Result<Plan, String> {
    let (options, args) = leading_options(args)?;
    let root_count = args
        .iter()
        .take_while(|arg| !starts_expression(arg))
        .count();
    let mut roots: Vec<PathBuf> = args[..root_count].iter().map(PathBuf::from).collect();
    if roots.is_empty() {
        roots.push(PathBuf::from("."));
    }
    let mut parser = Parser {
        args: &args[root_count..],
        pos: 0,
        options,
        batches: Vec::new(),
        has_action: false,
        regex_basic: false,
        files: Vec::new(),
        destinations: HashMap::new(),
    };
    let expr = parser.parse_all()?;
    Ok(Plan {
        roots,
        options: parser.options,
        expr,
        batches: parser.batches,
        files: parser.files,
    })
}

/// Parses the GNU options that precede the starting points:
/// `find [-H] [-L] [-P] [-D debugopts] [-Olevel] [path...] [expression]`.
fn leading_options(args: &[String]) -> Result<(Options, &[String]), String> {
    let mut options = Options::default();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "-H" => options.follow = Follow::RootsOnly,
            "-L" => options.follow = Follow::Always,
            "-P" => options.follow = Follow::Never,
            "-D" => {
                index += 1;
                if args.get(index).is_none() {
                    return Err("-D requires debug options".to_string());
                }
            }
            level if level.starts_with("-O") && level[2..].bytes().all(|b| b.is_ascii_digit()) => {}
            _ => break,
        }
        index += 1;
    }
    Ok((options, &args[index..]))
}

fn starts_expression(arg: &str) -> bool {
    (arg.starts_with('-') && arg.len() > 1) || matches!(arg, "(" | "!" | ",")
}

struct Parser<'a> {
    args: &'a [String],
    pos: usize,
    options: Options,
    batches: Vec<BatchSpec>,
    has_action: bool,
    regex_basic: bool,
    files: Vec<OutputFile>,
    destinations: HashMap<Destination, usize>,
}

impl<'a> Parser<'a> {
    fn parse_all(&mut self) -> Result<Expr, String> {
        let expr = if self.peek().is_some() {
            self.comma_expr()?
        } else {
            Expr::Const(true)
        };
        if let Some(extra) = self.peek() {
            return Err(if extra == ")" {
                "unmatched ')'".to_string()
            } else {
                format!("unexpected argument '{extra}' (paths must precede the expression)")
            });
        }
        // Without an explicit action, GNU find prints every entry the expression accepts.
        if self.has_action {
            Ok(expr)
        } else {
            Ok(Expr::And(
                Box::new(expr),
                Box::new(Expr::Action(Action::Print)),
            ))
        }
    }

    fn peek(&self) -> Option<&'a str> {
        let args: &'a [String] = self.args;
        args.get(self.pos).map(String::as_str)
    }

    fn next_token(&mut self) -> Option<&'a str> {
        let token = self.peek();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, names: &[&str]) -> bool {
        match self.peek() {
            Some(token) if names.contains(&token) => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn value(&mut self, option: &str) -> Result<&'a str, String> {
        self.next_token()
            .ok_or_else(|| format!("missing argument to '{option}'"))
    }

    fn comma_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.or_expr()?;
        while self.eat(&[","]) {
            let right = self.or_expr()?;
            left = Expr::Comma(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn or_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.and_expr()?;
        while self.eat(&["-o", "-or"]) {
            let right = self.and_expr()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            if !self.eat(&["-a", "-and"]) && !self.starts_operand() {
                break;
            }
            let right = self.unary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// True when the next token can begin a term, so two terms are ANDed implicitly.
    fn starts_operand(&self) -> bool {
        matches!(self.peek(), Some(token) if !matches!(token, "-o" | "-or" | ")" | ","))
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat(&["!", "-not"]) {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let Some(token) = self.next_token() else {
            return Err("expression expected before end of arguments".to_string());
        };
        match token {
            "(" => {
                let inner = self.comma_expr()?;
                if !self.eat(&[")"]) {
                    return Err("missing ')' in expression".to_string());
                }
                Ok(inner)
            }
            ")" | "-o" | "-or" | "-a" | "-and" | "," => Err(format!("unexpected '{token}'")),
            _ if token.starts_with('-') => self.predicate(token),
            _ => Err(format!("unexpected argument '{token}'")),
        }
    }

    fn predicate(&mut self, name: &str) -> Result<Expr, String> {
        match name {
            "-maxdepth" | "-mindepth" => {
                let text = self.value(name)?;
                let depth = text
                    .parse::<usize>()
                    .map_err(|_| format!("invalid argument '{text}' to {name}"))?;
                if name == "-maxdepth" {
                    self.options.max_depth = depth;
                } else {
                    self.options.min_depth = depth;
                }
                Ok(Expr::Const(true))
            }
            "-depth" => self.global(|options| options.depth_first = true),
            "-L" | "-follow" => self.global(|options| options.follow = Follow::Always),
            "-H" => self.global(|options| options.follow = Follow::RootsOnly),
            "-P" => self.global(|options| options.follow = Follow::Never),
            "-xdev" | "-mount" => self.global(|options| options.one_fs = true),
            "-noleaf" => Ok(Expr::Const(true)),
            "-ignore_readdir_race" => self.global(|options| options.ignore_race = true),
            "-regextype" => {
                self.regex_basic = match self.value(name)? {
                    "posix-basic" => true,
                    "posix-extended" | "emacs" => false,
                    value => return Err(format!("Unknown regular expression type '{value}'")),
                };
                Ok(Expr::Const(true))
            }
            "-true" => Ok(Expr::Const(true)),
            "-false" => Ok(Expr::Const(false)),
            "-prune" => Ok(Expr::Action(Action::Prune)),
            "-print" => self.action(Action::Print),
            "-print0" => self.action(Action::Print0),
            "-ls" => self.action(Action::Ls),
            "-quit" => self.action(Action::Quit),
            "-delete" => {
                self.options.depth_first = true;
                self.action(Action::Delete)
            }
            "-printf" => {
                let format = self.value(name)?;
                let pieces = printf::parse_format(format)?;
                self.action(Action::Printf(pieces))
            }
            "-exec" | "-ok" => self.exec(false, name == "-ok"),
            "-execdir" | "-okdir" => self.exec(true, name == "-okdir"),
            "-fprint" | "-fprint0" | "-fls" | "-fprintf" => {
                let path = self.value(name)?;
                let destination = Destination::resolve(path)
                    .map_err(|error| format!("'{path}': {}", message(&error)))?;
                let index = if let Some(index) = self.destinations.get(&destination) {
                    *index
                } else {
                    let file = destination
                        .create()
                        .map_err(|error| format!("'{path}': {}", message(&error)))?;
                    let index = self.files.len();
                    self.files.push(file);
                    self.destinations.insert(destination, index);
                    index
                };
                let action = match name {
                    "-fprint" => Action::Print,
                    "-fprint0" => Action::Print0,
                    "-fls" => Action::Ls,
                    _ => {
                        let format = self.value(name)?;
                        Action::Printf(printf::parse_format(format)?)
                    }
                };
                self.action(Action::File {
                    index,
                    action: Box::new(action),
                })
            }
            _ => self.test(name).map(Expr::Test),
        }
    }

    fn global(&mut self, change: impl FnOnce(&mut Options)) -> Result<Expr, String> {
        change(&mut self.options);
        Ok(Expr::Const(true))
    }

    fn action(&mut self, action: Action) -> Result<Expr, String> {
        self.has_action = true;
        Ok(Expr::Action(action))
    }

    fn test(&mut self, name: &str) -> Result<Test, String> {
        match name {
            "-name" => Ok(Test::Name(Glob::new(self.value(name)?, false))),
            "-iname" => Ok(Test::Name(Glob::new(self.value(name)?, true))),
            "-path" | "-wholename" => Ok(Test::Path(Glob::new(self.value(name)?, false))),
            "-ipath" | "-iwholename" => Ok(Test::Path(Glob::new(self.value(name)?, true))),
            "-regex" | "-iregex" => {
                let basic = self.regex_basic;
                regex_test(self.value(name)?, name == "-iregex", basic)
            }
            "-type" => Ok(Test::Type(parse_types(self.value(name)?)?)),
            "-empty" => Ok(Test::Empty),
            "-size" => parse_size(self.value(name)?),
            "-newer" => self.newer(name, Clock::Modified),
            "-anewer" => self.newer(name, Clock::Accessed),
            "-cnewer" => self.newer(name, Clock::Changed),
            "-mtime" => self.age(name, Clock::Modified, DAY_NS),
            "-atime" => self.age(name, Clock::Accessed, DAY_NS),
            "-ctime" => self.age(name, Clock::Changed, DAY_NS),
            "-mmin" => self.age(name, Clock::Modified, MINUTE_NS),
            "-amin" => self.age(name, Clock::Accessed, MINUTE_NS),
            "-cmin" => self.age(name, Clock::Changed, MINUTE_NS),
            "-perm" => Ok(Test::Perm(parse_perm(self.value(name)?)?)),
            "-readable" => Ok(Test::Access(Access::Read)),
            "-writable" => Ok(Test::Access(Access::Write)),
            "-executable" => Ok(Test::Access(Access::Execute)),
            "-user" | "-group" => {
                let value = self.value(name)?;
                let group = name == "-group";
                let id = value
                    .parse::<u32>()
                    .ok()
                    .or_else(|| {
                        if group {
                            crate::common::meta::group_id(value)
                        } else {
                            crate::common::meta::user_id(value)
                        }
                    })
                    .ok_or_else(|| {
                        format!(
                            "'{value}' is not a known {}",
                            if group { "group" } else { "user" }
                        )
                    })?;
                Ok(Test::Owner { group, id })
            }
            "-nouser" | "-nogroup" => Ok(Test::UnknownOwner {
                group: name == "-nogroup",
            }),
            "-uid" | "-gid" | "-links" | "-inum" => {
                let text = self.value(name)?;
                let (cmp, digits) = split_sign(text);
                let value = digits
                    .parse::<u64>()
                    .map_err(|_| format!("invalid argument '{text}' to {name}"))?;
                Ok(Test::Number {
                    field: match name {
                        "-uid" => NumberField::Uid,
                        "-gid" => NumberField::Gid,
                        "-links" => NumberField::Links,
                        _ => NumberField::Inode,
                    },
                    cmp,
                    value,
                })
            }
            "-samefile" => {
                let follow = self.options.follow != Follow::Never;
                let file = self.value(name)?;
                let meta = if follow {
                    fs::metadata(file)
                } else {
                    fs::symlink_metadata(file)
                }
                .map_err(|error| format!("'{file}': {}", message(&error)))?;
                Ok(Test::SameFile(
                    crate::common::meta::file_key(std::path::Path::new(file), &meta).ok(),
                ))
            }
            "-lname" | "-ilname" => Ok(Test::LinkName(Glob::new(
                self.value(name)?,
                name == "-ilname",
            ))),
            "-xtype" => Ok(Test::XType(parse_types(self.value(name)?)?)),
            name if name.starts_with("-newer") && name.len() == 8 => self.newer_xy(name),
            _ => Err(format!("unknown predicate '{name}'")),
        }
    }

    fn newer_xy(&mut self, name: &str) -> Result<Test, String> {
        let clock = |letter| match letter {
            b'a' => Ok(Clock::Accessed),
            b'c' => Ok(Clock::Changed),
            b'm' => Ok(Clock::Modified),
            b'B' => Ok(Clock::Birth),
            _ => Err(format!("invalid predicate '{name}'")),
        };
        let selected = clock(name.as_bytes()[6])?;
        let value = self.value(name)?;
        let reference_ns = if name.as_bytes()[7] == b't' {
            let time = crate::common::datetime::parse(value)
                .map_err(|error| format!("cannot interpret '{value}' as a date: {error}"))?;
            time.timestamp().as_nanosecond()
        } else {
            let reference = clock(name.as_bytes()[7])?;
            let metadata = if self.options.follow == Follow::Never {
                fs::symlink_metadata(value)
            } else {
                fs::metadata(value)
            }
            .map_err(|error| format!("'{value}': {}", message(&error)))?;
            super::expr::time_ns_at(std::path::Path::new(value), &metadata, reference)
                .ok_or_else(|| format!("cannot obtain reference time of '{value}'"))?
        };
        Ok(Test::Newer {
            clock: selected,
            reference_ns,
        })
    }

    fn newer(&mut self, name: &str, clock: Clock) -> Result<Test, String> {
        check_clock(name, clock)?;
        let file = self.value(name)?;
        let meta = if self.options.follow == Follow::Never {
            fs::symlink_metadata(file)
        } else {
            fs::metadata(file)
        }
        .map_err(|error| format!("'{file}': {}", message(&error)))?;
        let reference_ns = time_ns(&meta, clock)
            .ok_or_else(|| format!("{name}: cannot read the time of '{file}'"))?;
        Ok(Test::Newer {
            clock,
            reference_ns,
        })
    }

    fn age(&mut self, name: &str, clock: Clock, unit_ns: i128) -> Result<Test, String> {
        check_clock(name, clock)?;
        let text = self.value(name)?;
        let (cmp, digits) = split_sign(text);
        let count = digits
            .parse::<i64>()
            .map_err(|_| format!("invalid argument '{text}' to {name}"))?;
        Ok(Test::Age {
            clock,
            cmp,
            count,
            unit_ns,
        })
    }

    /// Parses `-exec`/`-execdir`. Arguments run up to a `;` or, when the previous
    /// argument is `{}`, up to a `+` (batched mode).
    fn exec(&mut self, in_dir: bool, prompt: bool) -> Result<Expr, String> {
        let name = match (in_dir, prompt) {
            (true, true) => "-okdir",
            (true, false) => "-execdir",
            (false, true) => "-ok",
            _ => "-exec",
        };
        let args: &'a [String] = self.args;
        let start = self.pos;
        let mut end = start;
        let batched = loop {
            let Some(token) = args.get(end) else {
                return Err(format!(
                    "missing terminator for {name} (use ';' or '{{}} +')"
                ));
            };
            if token == ";" {
                break false;
            }
            if token == "+" && end > start && args[end - 1] == "{}" {
                break true;
            }
            end += 1;
        };
        let mut argv: Vec<String> = args[start..end].to_vec();
        self.pos = end + 1;
        self.has_action = true;
        if batched {
            if prompt {
                return Err(format!("'{name}' does not support '{{}} +'"));
            }
            argv.pop();
            return self.batch(name, argv, in_dir);
        }
        if argv.is_empty() {
            return Err(format!("{name} needs a command before ';'"));
        }
        Ok(Expr::Action(Action::Exec {
            argv,
            in_dir,
            prompt,
        }))
    }

    fn batch(&mut self, name: &str, argv: Vec<String>, in_dir: bool) -> Result<Expr, String> {
        if argv.is_empty() {
            return Err(format!("{name} needs a command before '{{}} +'"));
        }
        if argv.iter().any(|arg| arg.contains("{}")) {
            return Err(format!(
                "{name} '{{}} +' supports {{}} only as the final argument"
            ));
        }
        let index = self.batches.len();
        self.batches.push(BatchSpec { argv, in_dir });
        Ok(Expr::Action(Action::Batch(index)))
    }
}

fn check_clock(name: &str, clock: Clock) -> Result<(), String> {
    if clock == Clock::Changed && !cfg!(unix) {
        return Err(format!("{name} is not supported on this platform"));
    }
    Ok(())
}

fn regex_test(pattern: &str, ignore_case: bool, basic: bool) -> Result<Test, String> {
    let translated = if basic {
        crate::common::regex::bre_to_regex(pattern)?
    } else {
        pattern.into()
    };
    RegexBuilder::new(&format!("^(?:{translated})$"))
        .case_insensitive(ignore_case)
        .build()
        .map(Test::Regex)
        .map_err(|error| format!("invalid regular expression '{pattern}': {error}"))
}

fn parse_types(text: &str) -> Result<Vec<char>, String> {
    let allowed: &[char] = if cfg!(unix) {
        &['f', 'd', 'l', 'b', 'c', 'p', 's']
    } else {
        &['f', 'd', 'l']
    };
    let mut kinds = Vec::new();
    for part in text.split(',') {
        let mut chars = part.chars();
        match (chars.next(), chars.next()) {
            (Some(kind), None) if allowed.contains(&kind) => kinds.push(kind),
            _ => return Err(format!("Unknown argument to -type: {text}")),
        }
    }
    Ok(kinds)
}

/// Splits a leading `+` (greater than), `-` (less than), or neither (equal).
fn split_sign(text: &str) -> (Cmp, &str) {
    if let Some(rest) = text.strip_prefix('+') {
        (Cmp::Greater, rest)
    } else if let Some(rest) = text.strip_prefix('-') {
        (Cmp::Less, rest)
    } else {
        (Cmp::Equal, text)
    }
}

/// Parses `-size [+-]N[cwbkMG]`. Sizes are rounded up to whole units, as GNU does.
fn parse_size(text: &str) -> Result<Test, String> {
    const UNITS: [(&str, u64); 6] = [
        ("c", 1),
        ("w", 2),
        ("b", 512),
        ("k", 1024),
        ("M", 1024 * 1024),
        ("G", 1024 * 1024 * 1024),
    ];
    let (cmp, digits) = split_sign(text);
    let (number, unit_bytes) = crate::common::size::split_suffix(digits, &UNITS, 512);
    let units = number
        .parse::<i64>()
        .map_err(|_| format!("invalid -size value '{text}'"))?;
    Ok(Test::Size {
        cmp,
        units,
        unit_bytes,
    })
}

/// Parses an octal mode: `MODE` (exact), `-MODE` (all bits set), or `/MODE` (any bit set).
fn parse_perm(text: &str) -> Result<PermCheck, String> {
    if !cfg!(unix) {
        return Err("-perm is only supported on Unix-like systems".to_string());
    }
    let (kind, digits) = if let Some(rest) = text.strip_prefix('-') {
        (0, rest)
    } else if let Some(rest) = text.strip_prefix('/') {
        (1, rest)
    } else {
        (2, text)
    };
    let valid = !digits.is_empty()
        && digits.len() <= 4
        && digits.chars().all(|character| character.is_digit(8));
    let bits = match u32::from_str_radix(digits, 8) {
        Ok(bits) if valid && bits <= 0o7777 => bits,
        _ => return Err(format!("invalid -perm mode '{text}' (octal only)")),
    };
    Ok(match kind {
        0 => PermCheck::AllBits(bits),
        1 => PermCheck::AnyBit(bits),
        _ => PermCheck::Exact(bits),
    })
}
