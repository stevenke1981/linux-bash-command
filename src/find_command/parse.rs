//! Turns find's command-line arguments into a `Plan`: starting points, global options,
//! and the expression tree. Operator precedence follows GNU find:
//! `!` binds tightest, then `-a` (also implied between adjacent terms), then `-o`,
//! then `,` (lowest).

use super::expr::{
    dev_ino, time_ns, Access, Action, Clock, Cmp, Expr, PermCheck, Test, DAY_NS, MINUTE_NS,
};
use super::glob::Glob;
use super::owner;
use super::printf;
use super::walk::message;
use crate::common::datetime;
use crate::common::regex::bre_to_regex;
use regex::RegexBuilder;
use std::fs::{self, File, OpenOptions};
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
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min_depth: 0,
            max_depth: usize::MAX,
            depth_first: false,
            follow: Follow::Never,
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
    /// Files named by `-fprint`, `-fprint0`, `-fprintf` and `-fls`, opened (and truncated)
    /// while parsing; actions refer to them by index.
    pub sinks: Vec<File>,
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
        sinks: Vec::new(),
    };
    let expr = parser.parse_all()?;
    Ok(Plan {
        roots,
        options: parser.options,
        expr,
        batches: parser.batches,
        sinks: parser.sinks.into_iter().map(|(_, file)| file).collect(),
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
    /// `-regextype posix-basic` makes later `-regex` patterns BRE; other types use ERE syntax.
    regex_basic: bool,
    /// Output files by name, shared when the same name is given twice.
    sinks: Vec<(String, File)>,
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
            // Accepted for compatibility. Mount boundaries are not enforced, and the
            // readdir race and leaf optimisations need no emulation.
            "-xdev" | "-mount" | "-noleaf" | "-ignore_readdir_race" => Ok(Expr::Const(true)),
            "-regextype" => {
                let kind = self.value(name)?;
                self.regex_basic = match kind {
                    "posix-basic" => true,
                    "posix-extended" | "posix-egrep" | "posix-awk" | "emacs" => false,
                    _ => return Err(format!("unknown regular expression type '{kind}'")),
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
            "-exec" => self.exec(false),
            "-execdir" => self.exec(true),
            "-ok" => self.confirm(name, false),
            "-okdir" => self.confirm(name, true),
            "-fprint" | "-fprint0" | "-fls" => {
                let file = self.value(name)?;
                let sink = self.sink(name, file)?;
                let action = match name {
                    "-fprint" => Action::FilePrint { sink, nul: false },
                    "-fprint0" => Action::FilePrint { sink, nul: true },
                    _ => Action::FileLs { sink },
                };
                self.action(action)
            }
            "-fprintf" => {
                let file = self.value(name)?;
                let format = self.value(name)?;
                let pieces = printf::parse_format(format)?;
                let sink = self.sink(name, file)?;
                self.action(Action::FilePrintf { sink, pieces })
            }
            _ => self.test(name).map(Expr::Test),
        }
    }

    /// Finds or opens (truncating) the output file `file` for an output action.
    fn sink(&mut self, name: &str, file: &str) -> Result<usize, String> {
        if let Some(index) = self.sinks.iter().position(|(known, _)| known == file) {
            return Ok(index);
        }
        let opened = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(file)
            .map_err(|error| format!("{name}: '{file}': {}", message(&error)))?;
        self.sinks.push((file.to_string(), opened));
        Ok(self.sinks.len() - 1)
    }

    /// `-ok CMD {} ;` and `-okdir`: like `-exec`/`-execdir` but confirmed interactively.
    fn confirm(&mut self, name: &str, in_dir: bool) -> Result<Expr, String> {
        let args: &'a [String] = self.args;
        let start = self.pos;
        let end = args[start..]
            .iter()
            .position(|token| token == ";")
            .map(|offset| start + offset)
            .ok_or_else(|| format!("missing terminator for {name} (use ';')"))?;
        let argv: Vec<String> = args[start..end].to_vec();
        if argv.is_empty() {
            return Err(format!("{name} needs a command before ';'"));
        }
        self.pos = end + 1;
        self.has_action = true;
        Ok(Expr::Action(Action::Confirm { argv, in_dir }))
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
            "-regex" => self.regex(name, false),
            "-iregex" => self.regex(name, true),
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
            "-user" => self.owner_id(name, owner::user_id, "user").map(Test::Uid),
            "-group" => self.owner_id(name, owner::group_id, "group").map(Test::Gid),
            "-uid" => self.numeric_id(name).map(Test::Uid),
            "-gid" => self.numeric_id(name).map(Test::Gid),
            "-nouser" => Ok(Test::NoUser),
            "-nogroup" => Ok(Test::NoGroup),
            "-links" => self.links(name),
            "-inum" => {
                check_unix_only(name)?;
                let text = self.value(name)?;
                let inode = text
                    .parse::<u64>()
                    .map_err(|_| format!("invalid argument '{text}' to {name}"))?;
                Ok(Test::Inum(inode))
            }
            "-samefile" => {
                check_unix_only(name)?;
                let file = self.value(name)?;
                let meta =
                    fs::metadata(file).map_err(|error| format!("'{file}': {}", message(&error)))?;
                let (dev, inode) = dev_ino(&meta);
                Ok(Test::SameFile { dev, inode })
            }
            "-lname" => Ok(Test::LinkName(Glob::new(self.value(name)?, false))),
            "-ilname" => Ok(Test::LinkName(Glob::new(self.value(name)?, true))),
            "-xtype" => Ok(Test::XType(parse_types(self.value(name)?)?)),
            _ if newer_xy_letters(name).is_some() => self.newer_xy(name),
            _ => Err(format!("unknown predicate '{name}'")),
        }
    }

    fn regex(&mut self, name: &str, ignore_case: bool) -> Result<Test, String> {
        let pattern = self.value(name)?;
        if self.regex_basic {
            let translated = bre_to_regex(pattern)?;
            regex_test(&translated, ignore_case)
        } else {
            regex_test(pattern, ignore_case)
        }
    }

    /// `-user NAME|UID` and `-group NAME|GID`: a numeric value is used as is, otherwise the
    /// name is looked up.
    fn owner_id(
        &mut self,
        name: &str,
        lookup: fn(&str) -> Option<u32>,
        what: &str,
    ) -> Result<u32, String> {
        let text = self.value(name)?;
        if let Ok(id) = text.parse::<u32>() {
            return Ok(id);
        }
        lookup(text).ok_or_else(|| format!("'{text}' is not the name of a known {what}"))
    }

    fn numeric_id(&mut self, name: &str) -> Result<u32, String> {
        let text = self.value(name)?;
        text.parse::<u32>()
            .map_err(|_| format!("invalid argument '{text}' to {name}"))
    }

    /// `-links [+-]N`: the hard-link count.
    fn links(&mut self, name: &str) -> Result<Test, String> {
        let text = self.value(name)?;
        let (cmp, digits) = split_sign(text);
        let count = digits
            .parse::<u64>()
            .map_err(|_| format!("invalid argument '{text}' to {name}"))?;
        Ok(Test::Links { cmp, count })
    }

    /// `-newerXY REF`: X is the file's time (a, B, c or m); Y is the reference's (a, B, c, m),
    /// or `t` for a date string given as the argument.
    fn newer_xy(&mut self, name: &str) -> Result<Test, String> {
        let (x, y) = newer_xy_letters(name).unwrap_or(('?', '?'));
        let clock =
            clock_letter(x).ok_or_else(|| format!("invalid argument '{name}' to -newerXY"))?;
        check_clock(name, clock)?;
        let value = self.value(name)?;
        let reference_ns = if y == 't' {
            let zone = datetime::zone_from_env();
            datetime::parse(value, datetime::now(), &zone)
                .map(|timestamp| timestamp.as_nanosecond())
                .map_err(|error| format!("{name}: {error}"))?
        } else {
            let reference_clock =
                clock_letter(y).ok_or_else(|| format!("invalid argument '{name}' to -newerXY"))?;
            check_clock(name, reference_clock)?;
            let meta =
                fs::metadata(value).map_err(|error| format!("'{value}': {}", message(&error)))?;
            time_ns(&meta, reference_clock)
                .ok_or_else(|| format!("{name}: cannot read the time of '{value}'"))?
        };
        Ok(Test::Newer {
            clock,
            reference_ns,
        })
    }

    fn newer(&mut self, name: &str, clock: Clock) -> Result<Test, String> {
        check_clock(name, clock)?;
        let file = self.value(name)?;
        let meta = fs::metadata(file).map_err(|error| format!("'{file}': {}", message(&error)))?;
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
    fn exec(&mut self, in_dir: bool) -> Result<Expr, String> {
        let name = if in_dir { "-execdir" } else { "-exec" };
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
            argv.pop();
            return self.batch(name, argv, in_dir);
        }
        if argv.is_empty() {
            return Err(format!("{name} needs a command before ';'"));
        }
        Ok(Expr::Action(Action::Exec { argv, in_dir }))
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

/// Inode-based tests need Unix device and inode numbers.
fn check_unix_only(name: &str) -> Result<(), String> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err(format!("{name} is not supported on this platform"))
    }
}

/// The letters X and Y of `-newerXY`, when `name` has that shape and valid letters.
fn newer_xy_letters(name: &str) -> Option<(char, char)> {
    let mut letters = name.strip_prefix("-newer")?.chars();
    match (letters.next(), letters.next(), letters.next()) {
        (Some(x), Some(y), None) if "aBcm".contains(x) && "aBcmt".contains(y) => Some((x, y)),
        _ => None,
    }
}

/// The clock named by one `-newerXY` letter (`t` is handled by the caller).
fn clock_letter(letter: char) -> Option<Clock> {
    match letter {
        'a' => Some(Clock::Accessed),
        'B' => Some(Clock::Birth),
        'c' => Some(Clock::Changed),
        'm' => Some(Clock::Modified),
        _ => None,
    }
}

fn regex_test(pattern: &str, ignore_case: bool) -> Result<Test, String> {
    RegexBuilder::new(&format!("^(?:{pattern})$"))
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
    let (number, unit_bytes) = UNITS
        .iter()
        .find_map(|(suffix, bytes)| digits.strip_suffix(suffix).map(|number| (number, *bytes)))
        .unwrap_or((digits, 512));
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
