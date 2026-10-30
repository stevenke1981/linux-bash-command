//! Command-line parsing for `sort`: option table, value validation and the resulting
//! [`Config`]. Usage errors exit with status 2, as GNU does.

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::EXIT_TROUBLE;

use super::keys::{parse_key, KeyDef, Modes};

const CMD: &str = "sort";

pub const OPTS: &[OptSpec] = &[
    OptSpec::flag("b", Some('b'), Some("ignore-leading-blanks")),
    OptSpec::flag("d", Some('d'), Some("dictionary-order")),
    OptSpec::flag("f", Some('f'), Some("ignore-case")),
    OptSpec::flag("g", Some('g'), Some("general-numeric-sort")),
    OptSpec::flag("i", Some('i'), Some("ignore-nonprinting")),
    OptSpec::flag("M", Some('M'), Some("month-sort")),
    OptSpec::flag("h", Some('h'), Some("human-numeric-sort")),
    OptSpec::flag("n", Some('n'), Some("numeric-sort")),
    OptSpec::flag("R", Some('R'), Some("random-sort")),
    OptSpec::req("random-source", None, Some("random-source")),
    OptSpec::flag("r", Some('r'), Some("reverse")),
    OptSpec::req("sort", None, Some("sort")),
    OptSpec::flag("V", Some('V'), Some("version-sort")),
    OptSpec::req("batch-size", None, Some("batch-size")),
    OptSpec::opt("check", Some('c'), Some("check")),
    OptSpec::flag("C", Some('C'), None),
    OptSpec::req("k", Some('k'), Some("key")),
    OptSpec::flag("m", Some('m'), Some("merge")),
    OptSpec::req("o", Some('o'), Some("output")),
    OptSpec::flag("s", Some('s'), Some("stable")),
    OptSpec::req("S", Some('S'), Some("buffer-size")),
    OptSpec::req("t", Some('t'), Some("field-separator")),
    OptSpec::req("T", Some('T'), Some("temporary-directory")),
    OptSpec::req("parallel", None, Some("parallel")),
    OptSpec::flag("u", Some('u'), Some("unique")),
    OptSpec::flag("z", Some('z'), Some("zero-terminated")),
    OptSpec::req("files0-from", None, Some("files0-from")),
    OptSpec::flag("debug", None, Some("debug")),
    OptSpec::req("compress-program", None, Some("compress-program")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

/// Default in-memory buffer when `-S` is absent.
const DEFAULT_BUFFER: u64 = 256 * 1024 * 1024;
const DEFAULT_BATCH: usize = 16;

/// The `-c`/`-C` checking mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    None,
    /// `-c`: report the first disorder.
    Diagnose,
    /// `-C` and `--check=quiet|silent`: exit status only.
    Quiet,
}

/// What the command line asks for.
pub enum Action {
    Sort(Box<Config>),
    Help,
    Version,
    /// Usage error already reported; the exit status to return.
    Exit(i32),
}

/// Validated options.
#[derive(Debug)]
pub struct Config {
    pub global: Modes,
    pub keys: Vec<KeyDef>,
    pub tab: Option<u8>,
    pub check: Check,
    pub merge: bool,
    pub unique: bool,
    pub stable: bool,
    pub zero: bool,
    pub output: Option<String>,
    pub buffer: u64,
    pub temp_dirs: Vec<String>,
    pub random_source: Option<String>,
    pub batch_size: usize,
    pub files0_from: Option<String>,
    pub files: Vec<String>,
}

/// Parses and validates `args`, reporting usage errors to stderr.
pub fn parse(args: &[String]) -> Action {
    let items = match Parser::new(CMD, OPTS).parse(args) {
        Ok(items) => items,
        Err(err) => return Action::Exit(report_usage(CMD, &err, EXIT_TROUBLE)),
    };
    let mut builder = Builder::default();
    for item in items {
        match item {
            Item::Operand(text) => builder.files.push(text),
            Item::Opt { id, value } => {
                if let Some(action) = builder.apply(id, value) {
                    return action;
                }
            }
        }
    }
    builder.finish()
}

#[derive(Default)]
struct Builder {
    global: Modes,
    keys: Vec<KeyDef>,
    tab: Option<u8>,
    check: Option<Check>,
    merge: bool,
    unique: bool,
    stable: bool,
    zero: bool,
    output: Option<String>,
    buffer: Option<u64>,
    temp_dirs: Vec<String>,
    random_source: Option<String>,
    batch_size: Option<usize>,
    files0_from: Option<String>,
    files: Vec<String>,
    help: bool,
    version: bool,
}

fn fail(message: &str) -> Action {
    eprintln!("{CMD}: {message}");
    Action::Exit(EXIT_TROUBLE)
}

impl Builder {
    /// Applies one option. Returns an action when parsing must stop early.
    fn apply(&mut self, id: &str, value: Option<String>) -> Option<Action> {
        let text = value.unwrap_or_default();
        let letter = id.chars().next().unwrap_or('?');
        if id.len() == 1 && self.global.set(letter as u8, true, true) {
            return None;
        }
        match id {
            "k" => return self.add_key(&text),
            "t" => return self.set_tab(&text),
            "u" => self.unique = true,
            "s" => self.stable = true,
            "check" => return self.set_check(&text),
            "C" => self.check = Some(Check::Quiet),
            "m" => self.merge = true,
            "z" => self.zero = true,
            "o" => self.output = Some(text),
            "S" => return self.set_buffer(&text),
            "T" => self.temp_dirs.push(text),
            "sort" => return self.set_sort(&text),
            "random-source" => self.random_source = Some(text),
            "batch-size" => return self.set_batch(&text),
            "files0-from" => self.files0_from = Some(text),
            "parallel" => return self.check_parallel(&text),
            "debug" => return Some(self.unsupported("--debug")),
            "compress-program" => return Some(self.unsupported("--compress-program")),
            "help" => self.help = true,
            "version" => self.version = true,
            _ => {}
        }
        None
    }

    fn unsupported(&self, opt: &str) -> Action {
        eprintln!(
            "{CMD}: option '{opt}' is not supported by agentcmd\nTry '{CMD} --help' for more information."
        );
        Action::Exit(EXIT_TROUBLE)
    }

    fn add_key(&mut self, spec: &str) -> Option<Action> {
        match parse_key(spec) {
            Ok(key) => {
                self.keys.push(key);
                None
            }
            Err(message) => Some(fail(&message)),
        }
    }

    fn set_tab(&mut self, text: &str) -> Option<Action> {
        let bytes = text.as_bytes();
        if bytes.is_empty() {
            return Some(fail("empty tab"));
        }
        if bytes == b"\\0" {
            self.tab = Some(0);
            return None;
        }
        if bytes.len() != 1 {
            return Some(fail(&format!("multi-character tab '{text}'")));
        }
        self.tab = Some(bytes[0]);
        None
    }

    fn set_check(&mut self, text: &str) -> Option<Action> {
        self.check = Some(match text {
            "" | "diagnose-first" => Check::Diagnose,
            "quiet" | "silent" => Check::Quiet,
            other => {
                return Some(invalid_choice(
                    other,
                    "--check",
                    &["diagnose-first", "quiet", "silent"],
                ))
            }
        });
        None
    }

    fn set_sort(&mut self, text: &str) -> Option<Action> {
        const WORDS: &[(&str, char)] = &[
            ("general-numeric", 'g'),
            ("human-numeric", 'h'),
            ("month", 'M'),
            ("numeric", 'n'),
            ("random", 'R'),
            ("version", 'V'),
        ];
        match WORDS.iter().find(|(word, _)| *word == text) {
            Some((_, letter)) => {
                self.global.set(*letter as u8, true, true);
                None
            }
            None => {
                let words: Vec<&str> = WORDS.iter().map(|(w, _)| *w).collect();
                Some(invalid_choice(text, "--sort", &words))
            }
        }
    }

    fn set_buffer(&mut self, text: &str) -> Option<Action> {
        match parse_size(text) {
            Some(bytes) => {
                self.buffer = Some(bytes);
                None
            }
            None => Some(fail(&format!("invalid -S argument '{text}'"))),
        }
    }

    fn set_batch(&mut self, text: &str) -> Option<Action> {
        match text.parse::<usize>() {
            Ok(n) if n >= 2 => {
                self.batch_size = Some(n);
                None
            }
            Ok(n) => {
                eprintln!("{CMD}: invalid --batch-size argument '{n}'");
                Some(fail("minimum --batch-size argument is '2'"))
            }
            Err(_) => Some(fail(&format!("invalid --batch-size argument '{text}'"))),
        }
    }

    fn check_parallel(&mut self, text: &str) -> Option<Action> {
        match text.parse::<usize>() {
            Ok(n) if n >= 1 => None,
            _ => Some(fail(&format!("invalid number of threads: '{text}'"))),
        }
    }

    fn finish(self) -> Action {
        if self.help {
            return Action::Help;
        }
        if self.version {
            return Action::Version;
        }
        if let Some(bad) = self.incompatible_modes() {
            return fail(&format!("options '-{bad}' are incompatible"));
        }
        if self.files0_from.is_some() && !self.files.is_empty() {
            return fail(&format!(
                "extra operand '{}' not allowed with --files0-from",
                self.files[0]
            ));
        }
        let check = self.check.unwrap_or(Check::None);
        if check != Check::None && self.files.len() > 1 {
            return fail(&format!(
                "extra operand '{}' not allowed with -c",
                self.files[1]
            ));
        }
        Action::Sort(Box::new(Config {
            global: self.global,
            keys: self.keys,
            tab: self.tab,
            check,
            merge: self.merge,
            unique: self.unique,
            stable: self.stable,
            zero: self.zero,
            output: self.output,
            buffer: self.buffer.unwrap_or(DEFAULT_BUFFER),
            temp_dirs: self.temp_dirs,
            random_source: self.random_source,
            batch_size: self.batch_size.unwrap_or(DEFAULT_BATCH),
            files0_from: self.files0_from,
            files: self.files,
        }))
    }

    /// The letters of the first key whose ordering modes conflict, if any. A key without
    /// its own ordering modes uses the global ones, so the global set is checked too.
    fn incompatible_modes(&self) -> Option<String> {
        if self.global.is_incompatible() {
            return Some(self.global.conflict_letters());
        }
        self.keys
            .iter()
            .find(|key| key.modes.is_incompatible())
            .map(|key| key.modes.conflict_letters())
    }
}

fn invalid_choice(value: &str, opt: &str, valid: &[&str]) -> Action {
    let mut message = format!("invalid argument '{value}' for '{opt}'\nValid arguments are:");
    for word in valid {
        message.push_str(&format!("\n  - '{word}'"));
    }
    eprintln!("{CMD}: {message}\nTry '{CMD} --help' for more information.");
    Action::Exit(EXIT_TROUBLE)
}

/// Parses a `-S` size: digits with an optional unit (`b`, `K`/`k`, `M`, ... `Q`).
pub fn parse_size(text: &str) -> Option<u64> {
    let digits = text.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let number: u64 = text[..digits].parse().ok()?;
    let unit = &text[digits..];
    let shift = match unit {
        "" | "b" => 0,
        "k" | "K" => 10,
        "M" => 20,
        "G" => 30,
        "T" => 40,
        "P" => 50,
        "E" => 60,
        _ => return None,
    };
    Some(number.saturating_mul(1u64 << shift))
}

pub const HELP: &str = "\
Usage: sort [OPTION]... [FILE]...
  or:  sort [OPTION]... --files0-from=F
Write sorted concatenation of all FILE(s) to standard output.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
Ordering options:

  -b, --ignore-leading-blanks  ignore leading blanks
  -d, --dictionary-order      consider only blanks and alphanumeric characters
  -f, --ignore-case           fold lower case to upper case characters
  -g, --general-numeric-sort  compare according to general numerical value
  -i, --ignore-nonprinting    consider only printable characters
  -M, --month-sort            compare (unknown) < 'JAN' < ... < 'DEC'
  -h, --human-numeric-sort    compare human readable numbers (e.g., 2K 1G)
  -n, --numeric-sort          compare according to string numerical value
  -R, --random-sort           shuffle, but group identical keys
      --random-source=FILE    get random bytes from FILE
  -r, --reverse               reverse the result of comparisons
      --sort=WORD             sort according to WORD:
                                general-numeric -g, human-numeric -h, month -M,
                                numeric -n, random -R, version -V
  -V, --version-sort          natural sort of (version) numbers within text

Other options:

      --batch-size=NMERGE   merge at most NMERGE inputs at once;
                            for more use temp files
  -c, --check, --check=diagnose-first  check for sorted input; do not sort
  -C, --check=quiet, --check=silent  like -c, but do not report first bad line
      --files0-from=F       read input from the files specified by
                            NUL-terminated names in file F;
                            If F is - then read names from standard input
  -k, --key=KEYDEF          sort via a key; KEYDEF gives location and type
  -m, --merge               merge already sorted files; do not sort
  -o, --output=FILE         write result to FILE instead of standard output
  -s, --stable              stabilize sort by disabling last-resort comparison
  -S, --buffer-size=SIZE    use SIZE for main memory buffer
  -t, --field-separator=SEP  use SEP instead of non-blank to blank transition
  -T, --temporary-directory=DIR  use DIR for temporaries, not $TMPDIR or /tmp;
                              multiple options specify multiple directories
      --parallel=N          change the number of sorts run concurrently to N
                            (accepted; sorting is single-threaded)
  -u, --unique              with -c, check for strict ordering;
                              without -c, output only the first of an equal run
  -z, --zero-terminated     line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

KEYDEF is F[.C][OPTS][,F[.C][OPTS]] for start and stop position, where F is a
field number and C a character position in the field; both are origin 1, and
the stop position defaults to the line's end.  If neither -t nor -b is in
effect, characters in a field are counted from the beginning of the preceding
whitespace.  OPTS is one or more single-letter ordering options [bdfgiMhnRrV],
which override global ordering options for that key.  If no key is given, use
the entire line as the key.

SIZE may be followed by the following multiplicative suffixes:
b 1, K 1024 (default), and so on for M, G, T, P, E.

*** WARNING ***
Byte values are compared (LC_ALL=C is always in effect).
";

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn config(list: &[&str]) -> Config {
        match parse(&args(list)) {
            Action::Sort(config) => *config,
            _ => panic!("expected a sort configuration"),
        }
    }

    #[test]
    fn sizes_take_unit_suffixes() {
        assert_eq!(parse_size("10"), Some(10));
        assert_eq!(parse_size("2K"), Some(2048));
        assert_eq!(parse_size("1k"), Some(1024));
        assert_eq!(parse_size("3x"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn global_letters_and_keys_are_collected() {
        let c = config(&["-nr", "-k2,2", "-t", ",", "-u", "file"]);
        assert!(c.global.numeric && c.global.reverse);
        assert_eq!(c.keys.len(), 1);
        assert_eq!(c.tab, Some(b','));
        assert!(c.unique);
        assert_eq!(c.files, ["file"]);
    }

    #[test]
    fn check_values_are_validated() {
        assert_eq!(config(&["-c"]).check, Check::Diagnose);
        assert_eq!(config(&["-C"]).check, Check::Quiet);
        assert_eq!(config(&["--check=silent"]).check, Check::Quiet);
        assert!(matches!(parse(&args(&["--check=bogus"])), Action::Exit(2)));
    }

    #[test]
    fn incompatible_modes_are_usage_errors() {
        assert!(matches!(parse(&args(&["-nM"])), Action::Exit(2)));
        assert!(matches!(parse(&args(&["-dn"])), Action::Exit(2)));
        assert!(matches!(parse(&args(&["-dV"])), Action::Sort(_)));
    }

    #[test]
    fn tab_and_batch_are_validated() {
        assert!(matches!(parse(&args(&["-t", ""])), Action::Exit(2)));
        assert!(matches!(parse(&args(&["-t", "ab"])), Action::Exit(2)));
        assert_eq!(config(&["-t", "\\0"]).tab, Some(0));
        assert!(matches!(parse(&args(&["--batch-size=1"])), Action::Exit(2)));
    }

    #[test]
    fn unsupported_options_exit_two() {
        assert!(matches!(parse(&args(&["--debug"])), Action::Exit(2)));
        assert!(matches!(parse(&args(&["--bogus"])), Action::Exit(2)));
    }

    #[test]
    fn help_and_version_are_actions() {
        assert!(matches!(parse(&args(&["--help"])), Action::Help));
        assert!(matches!(parse(&args(&["--version"])), Action::Version));
    }
}
