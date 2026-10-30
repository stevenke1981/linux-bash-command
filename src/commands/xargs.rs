use crate::common::{
    args::{self, Item, OptSpec},
    error, io, process,
};
use std::ffi::{OsStr, OsString};
use std::io::BufRead;
use std::process::{Child, Stdio};
use std::time::Duration;
const HELP:&str="Usage: xargs [OPTION]... [COMMAND [INITIAL-ARGS]...]\nBuild and execute command lines from standard input.\n  -0, --null                 Items end in NUL; quotes are literal\n  -a, --arg-file=FILE        Read items from FILE\n  -d, --delimiter=CHAR       Items end in CHAR\n  -E EOF, -e[EOF], --eof[=EOF]  Stop at EOF marker\n  -I STR, -i[STR], --replace[=STR]  Replace STR once per input line\n  -L N, -l[N], --max-lines[=N]  Use at most N input lines\n  -n, --max-args=N           Use at most N arguments\n  -P, --max-procs=N          Run N processes (0: CPU count times 4)\n  -r, --no-run-if-empty      Do not run with empty input\n  -s, --max-chars=N          Limit command line size\n  -t, --verbose              Print each command on stderr\n  -x, --exit                 Fail if command size would be exceeded\n      --process-slot-var=VAR Set VAR to a unique running process slot\n      --show-limits          Show command size limits\n      --help                 Display this help\n      --version              Display version\nExit status: 123 for child status 1..125, 124 for 255, 125 for signals, 126 cannot execute, 127 not found, 1 own errors.\n";
const SPECS: &[OptSpec] = &[
    OptSpec::flag("null", '0'),
    OptSpec::value("arg-file", 'a'),
    OptSpec::value("delimiter", 'd'),
    OptSpec {
        id: "eof-required",
        short: 'E',
        long: "",
        has_arg: args::HasArg::Required,
    },
    OptSpec::optional("eof", 'e'),
    OptSpec {
        id: "replace-required",
        short: 'I',
        long: "",
        has_arg: args::HasArg::Required,
    },
    OptSpec::optional("replace", 'i'),
    OptSpec {
        id: "lines-required",
        short: 'L',
        long: "",
        has_arg: args::HasArg::Required,
    },
    OptSpec::optional("max-lines", 'l'),
    OptSpec::value("max-args", 'n'),
    OptSpec::value("max-procs", 'P'),
    OptSpec::flag("no-run-if-empty", 'r'),
    OptSpec::value("max-chars", 's'),
    OptSpec::flag("verbose", 't'),
    OptSpec::flag("exit", 'x'),
    OptSpec::value("process-slot-var", '\0'),
    OptSpec::flag("show-limits", '\0'),
    OptSpec::flag("help", '\0'),
    OptSpec::flag("version", '\0'),
];

#[derive(Default)]
struct Options {
    delimiter: Option<u8>,
    file: Option<String>,
    eof: Option<Vec<u8>>,
    replace: Option<String>,
    lines: Option<usize>,
    number: Option<usize>,
    parallel: usize,
    size: usize,
    no_empty: bool,
    verbose: bool,
    exit: bool,
    slot: Option<String>,
    limits: bool,
    command: Vec<String>,
}
fn positive(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("invalid number '{value}'"))
}
fn command_limit() -> usize {
    #[cfg(unix)]
    {
        let environment = std::env::vars_os()
            .map(|(k, v)| k.len() + v.len() + 2)
            .sum::<usize>();
        // POSIX reserves argument and environment storage together; GNU leaves 2 KiB headroom.
        let maximum = usize::try_from(unsafe { libc::sysconf(libc::_SC_ARG_MAX) })
            .unwrap_or(131072 + environment + 2048);
        maximum.saturating_sub(environment + 2048).min(131072)
    }
    #[cfg(not(unix))]
    {
        32767
    }
}
fn delimiter(text: &str) -> Result<u8, String> {
    if text.len() == 1 {
        return Ok(text.as_bytes()[0]);
    }
    let bytes = if let Some(octal) = text
        .strip_prefix('\\')
        .filter(|s| s.bytes().all(|b| (b'0'..=b'7').contains(&b)))
    {
        vec![u8::from_str_radix(octal, 8).map_err(|_| "invalid delimiter")?]
    } else {
        crate::common::escape::decode(text.as_bytes(), crate::common::escape::Mode::Echo)?.0
    };
    if bytes.len() == 1 {
        Ok(bytes[0])
    } else {
        Err("delimiter must be a single character or an escape sequence".into())
    }
}
fn parse(arguments: &[String]) -> Result<Options, String> {
    let items = args::parse(arguments, SPECS, false)?;
    let mut o = Options {
        parallel: 1,
        size: command_limit(),
        ..Options::default()
    };
    for item in items {
        match item {
            Item::Operand(s) => o.command.push(s),
            Item::Opt("null", _) => o.delimiter = Some(0),
            Item::Opt("delimiter", Some(s)) => o.delimiter = Some(delimiter(&s)?),
            Item::Opt("arg-file", s) => o.file = s,
            Item::Opt("eof-required" | "eof", s) => {
                o.eof = s.filter(|s| !s.is_empty()).map(String::into_bytes)
            }
            Item::Opt("replace-required" | "replace", s) => {
                let s = s.unwrap_or_else(|| "{}".into());
                if s.is_empty() {
                    return Err("replacement string must not be empty".into());
                }
                o.replace = Some(s);
                o.lines = Some(1);
                o.number = None;
                o.exit = true;
            }
            Item::Opt("lines-required" | "max-lines", s) => {
                o.lines = Some(positive(s.as_deref().unwrap_or("1"))?);
                o.number = None;
                o.replace = None;
            }
            Item::Opt("max-args", Some(s)) => {
                let n = positive(&s)?;
                if o.replace.is_none() || n != 1 {
                    o.number = Some(n);
                    o.lines = None;
                    o.replace = None;
                }
            }
            Item::Opt("max-procs", Some(s)) => {
                o.parallel = s.parse().map_err(|_| format!("invalid number '{s}'"))?;
                if o.parallel == 0 {
                    o.parallel = std::thread::available_parallelism().map_or(4, |n| n.get() * 4);
                }
            }
            Item::Opt("max-chars", Some(s)) => o.size = positive(&s)?.min(o.size),
            Item::Opt("no-run-if-empty", _) => o.no_empty = true,
            Item::Opt("verbose", _) => o.verbose = true,
            Item::Opt("exit", _) => o.exit = true,
            Item::Opt("process-slot-var", Some(s)) => {
                if s.is_empty() || s.contains('=') {
                    return Err("invalid process slot variable".into());
                }
                o.slot = Some(s);
            }
            Item::Opt("show-limits", _) => o.limits = true,
            Item::Opt("help" | "version", _) => {}
            _ => unreachable!(),
        }
    }
    if o.command.is_empty() {
        o.command.push("echo".into());
    }
    Ok(o)
}

struct Tokens {
    reader: Box<dyn BufRead>,
    delimiter: Option<u8>,
    replace: bool,
    line: usize,
    line_has_item: bool,
    continuation: bool,
}
impl Tokens {
    fn byte(&mut self) -> std::io::Result<Option<u8>> {
        let b = self.reader.fill_buf()?.first().copied();
        if b.is_some() {
            self.reader.consume(1);
        }
        Ok(b)
    }
    fn next(&mut self) -> Result<Option<(Vec<u8>, usize)>, String> {
        if let Some(delimiter) = self.delimiter {
            let mut bytes = Vec::new();
            let n = self
                .reader
                .read_until(delimiter, &mut bytes)
                .map_err(|e| error::io_text(&e))?;
            if n == 0 {
                return Ok(None);
            }
            if bytes.last() == Some(&delimiter) {
                bytes.pop();
            }
            self.line += 1;
            return Ok(Some((bytes, self.line)));
        }
        let mut token = Vec::new();
        let mut quote = None;
        let mut active = false;
        let mut escaped = false;
        loop {
            let Some(byte) = self.byte().map_err(|e| error::io_text(&e))? else {
                if quote.is_some() {
                    return Err("unmatched quote; by default quotes are special to xargs unless you use the -0 option".into());
                }
                if escaped {
                    return Err("trailing backslash in input".into());
                }
                return Ok(active.then_some((token, self.line)));
            };
            if escaped {
                token.push(byte);
                active = true;
                self.continuation = false;
                escaped = false;
                continue;
            }
            if byte == b'\n' && quote.is_some() {
                return Err("unmatched quote; by default quotes are special to xargs unless you use the -0 option".into());
            }
            if byte == b'\\' && quote.is_none() {
                escaped = true;
                active = true;
                self.continuation = false;
                continue;
            }
            if byte == b'\'' || byte == b'"' {
                if quote == Some(byte) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(byte);
                } else {
                    token.push(byte);
                }
                active = true;
                self.continuation = false;
                continue;
            }
            if quote.is_none() && byte.is_ascii_whitespace() {
                if byte == b'\n' {
                    let line = self.line;
                    if active || (self.line_has_item && !self.continuation) {
                        self.line += 1;
                    }
                    self.line_has_item = false;
                    self.continuation = false;
                    if active {
                        return Ok(Some((token, line)));
                    }
                } else if self.replace && active {
                    token.push(byte);
                } else if active {
                    self.line_has_item = true;
                    self.continuation = true;
                    return Ok(Some((token, self.line)));
                }
                continue;
            }
            token.push(byte);
            active = true;
            self.continuation = false;
        }
    }
}
fn argument(bytes: Vec<u8>) -> Result<OsString, String> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes)
            .map(OsString::from)
            .map_err(|_| "input must be valid UTF-8 on this platform".into())
    }
}
fn size(arguments: &[OsString]) -> usize {
    arguments
        .iter()
        .map(|s| {
            if cfg!(windows) {
                s.to_string_lossy().encode_utf16().count()
                    + 3
                    + s.as_encoded_bytes()
                        .iter()
                        .filter(|b| **b == b'"' || **b == b'\\')
                        .count()
            } else {
                s.len() + 1
            }
        })
        .sum()
}
fn child_code(status: std::process::ExitStatus) -> i32 {
    match status.code() {
        Some(0) => 0,
        Some(255) => 124,
        Some(_) => 123,
        None => 125,
    }
}
struct Jobs {
    children: Vec<(Child, usize)>,
    code: i32,
}
impl Jobs {
    fn reap(&mut self, block: bool) -> Result<(), String> {
        let mut i = 0;
        let previous = self.children.len();
        loop {
            while i < self.children.len() {
                if let Some(status) = self.children[i]
                    .0
                    .try_wait()
                    .map_err(|e| error::io_text(&e))?
                {
                    let code = child_code(status);
                    if code != 0 {
                        self.code = self.code.max(code);
                    }
                    self.children.swap_remove(i);
                } else {
                    i += 1;
                }
            }
            if !block || self.children.is_empty() || self.children.len() < previous {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
            i = 0;
        }
    }
    fn launch(&mut self, arguments: &[OsString], o: &Options) -> Result<(), i32> {
        while self.children.len() >= o.parallel {
            if let Err(e) = self.reap(true) {
                eprintln!("xargs: {e}");
                return Err(1);
            }
            if self.code >= 124 {
                return Err(self.code);
            }
        }
        if let Err(e) = self.reap(false) {
            eprintln!("xargs: {e}");
            return Err(1);
        }
        if self.code >= 124 {
            return Err(self.code);
        }
        let slot = (0..o.parallel)
            .find(|slot| !self.children.iter().any(|(_, s)| s == slot))
            .unwrap();
        let mut environment: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        if let Some(name) = &o.slot {
            environment.retain(|(k, _)| !process::env_name_eq(k, OsStr::new(name)));
            environment.push((name.into(), slot.to_string().into()));
        }
        if o.verbose {
            eprintln!(
                "{}",
                arguments
                    .iter()
                    .map(
                        |s| String::from_utf8_lossy(&crate::common::escape::quote_name(
                            s.as_encoded_bytes()
                        ))
                        .into_owned()
                    )
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        let mut command = match process::command(&arguments[0], &environment) {
            Ok(c) => c,
            Err(e) => {
                if cfg!(windows)
                    && arguments[0] == "echo"
                    && e.kind() == std::io::ErrorKind::NotFound
                {
                    let text = arguments[1..]
                        .iter()
                        .map(|s| s.to_string_lossy().into_owned())
                        .collect::<Vec<_>>();
                    let result = crate::commands::echo::run(&text);
                    if result != 0 {
                        self.code = 123;
                    }
                    return Ok(());
                }
                eprintln!(
                    "xargs: {}: {}",
                    arguments[0].to_string_lossy(),
                    error::io_text(&e)
                );
                return Err(process::error_code(&e));
            }
        };
        command.args(&arguments[1..]);
        if o.file.is_none() {
            command.stdin(Stdio::null());
        }
        match command.spawn() {
            Ok(child) => {
                self.children.push((child, slot));
                Ok(())
            }
            Err(e) => {
                eprintln!(
                    "xargs: {}: {}",
                    arguments[0].to_string_lossy(),
                    error::io_text(&e)
                );
                Err(process::error_code(&e))
            }
        }
    }
}

pub fn run(arguments: &[String]) -> i32 {
    // Stop information parsing at COMMAND, whose arguments belong to the child.
    match args::parse(arguments, SPECS, false) {
        Ok(items) => {
            for item in items {
                if let Item::Opt(id @ ("help" | "version"), _) = item {
                    return args::help_version("xargs", &[format!("--{id}")], HELP, true).unwrap();
                }
            }
        }
        Err(e) => return error::usage("xargs", e, 1),
    }
    let o = match parse(arguments) {
        Ok(o) => o,
        Err(e) => return error::usage("xargs", e, 1),
    };
    if o.limits {
        eprintln!("Your environment variables take up {} bytes\nMaximum length of command we could actually use: {}\nSize of command buffer we are actually using: {}\nMaximum parallelism: {}",std::env::vars_os().map(|(k,v)|k.len()+v.len()+2).sum::<usize>(),if cfg!(windows){32767}else{131072},o.size,o.parallel);
    }
    let command: Vec<OsString> = o.command.iter().map(OsString::from).collect();
    if size(&command) > o.size {
        return error::usage(
            "xargs",
            "cannot fit single argument within argument list size limit",
            1,
        );
    }
    let reader = match io::input(o.file.as_deref().unwrap_or("-")) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "xargs: {}: {}",
                o.file.as_deref().unwrap_or("-"),
                error::io_text(&e)
            );
            return 1;
        }
    };
    let mut tokens = Tokens {
        reader,
        delimiter: o.delimiter,
        replace: o.replace.is_some(),
        line: 0,
        line_has_item: false,
        continuation: false,
    };
    let mut jobs = Jobs {
        children: Vec::new(),
        code: 0,
    };
    let mut pending = None;
    let mut ran = false;
    loop {
        let mut batch = Vec::new();
        let mut first_line = None;
        let mut done = false;
        loop {
            let item = if pending.is_some() {
                pending.take()
            } else {
                match tokens.next() {
                    Ok(item) => item,
                    Err(e) => {
                        eprintln!("xargs: {e}");
                        jobs.code = jobs.code.max(1);
                        done = true;
                        break;
                    }
                }
            };
            let Some((bytes, line)) = item else {
                done = true;
                break;
            };
            if o.eof.as_ref() == Some(&bytes) {
                done = true;
                break;
            }
            let text = match argument(bytes.clone()) {
                Ok(s) => s,
                Err(message) => {
                    eprintln!("xargs: {message}");
                    jobs.code = 1;
                    done = true;
                    break;
                }
            };
            if first_line.is_some_and(|first| o.lines.is_some_and(|n| line - first >= n))
                && !batch.is_empty()
            {
                pending = Some((bytes, line));
                break;
            }
            let mut proposed = command.clone();
            proposed.extend(batch.iter().cloned());
            proposed.push(text.clone());
            if o.replace.is_none() && size(&proposed) > o.size {
                if batch.is_empty() || o.exit {
                    eprintln!("xargs: argument line too long");
                    jobs.code = 1;
                    done = true;
                    break;
                }
                pending = Some((bytes, line));
                break;
            }
            first_line.get_or_insert(line);
            batch.push(text);
            if o.replace.is_some() || o.number.is_some_and(|n| batch.len() >= n) {
                break;
            }
        }
        if jobs.code == 1 {
            break;
        }
        if batch.is_empty() && (ran || o.no_empty) {
            break;
        }
        let args = if let Some(replace) = &o.replace {
            if batch.is_empty() {
                break;
            }
            o.command
                .iter()
                .map(|s| {
                    let mut bytes = Vec::new();
                    let mut rest = s.as_bytes();
                    let needle = replace.as_bytes();
                    while let Some(position) = rest.windows(needle.len()).position(|w| w == needle)
                    {
                        bytes.extend_from_slice(&rest[..position]);
                        bytes.extend_from_slice(batch[0].as_encoded_bytes());
                        rest = &rest[position + needle.len()..];
                    }
                    bytes.extend_from_slice(rest);
                    argument(bytes).expect("replacement preserves platform encoding")
                })
                .collect::<Vec<_>>()
        } else {
            let mut args = command.clone();
            args.extend(batch);
            args
        };
        if size(&args) > o.size {
            eprintln!("xargs: argument list too long");
            jobs.code = 1;
            break;
        }
        if let Err(code) = jobs.launch(&args, &o) {
            jobs.code = code;
            break;
        }
        ran = true;
        if done {
            break;
        }
    }
    while !jobs.children.is_empty() {
        if let Err(e) = jobs.reap(true) {
            eprintln!("xargs: {e}");
            return 1;
        }
    }
    jobs.code
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tokens(text: &[u8], d: Option<u8>, replace: bool) -> Result<Vec<Vec<u8>>, String> {
        let mut t = Tokens {
            reader: Box::new(std::io::Cursor::new(text.to_vec())),
            delimiter: d,
            replace,
            line: 0,
            line_has_item: false,
            continuation: false,
        };
        let mut out = Vec::new();
        while let Some((v, _)) = t.next()? {
            out.push(v);
        }
        Ok(out)
    }
    #[test]
    fn whitespace() {
        assert_eq!(
            tokens(b" a\tb\nc ", None, false).unwrap(),
            [b"a", b"b", b"c"]
        );
    }
    #[test]
    fn quotes() {
        assert_eq!(
            tokens(b"'a b' \"c d\" ''", None, false).unwrap(),
            [b"a b".to_vec(), b"c d".to_vec(), Vec::new()]
        );
    }
    #[test]
    fn escape() {
        assert_eq!(tokens(b"a\\ b", None, false).unwrap(), [b"a b"]);
    }
    #[test]
    fn unclosed() {
        assert!(tokens(b"'a", None, false).is_err());
    }
    #[test]
    fn nul() {
        assert_eq!(
            tokens(b"a b\0'c'\0", Some(0), false).unwrap(),
            [b"a b".to_vec(), b"'c'".to_vec()]
        );
    }
    #[test]
    fn replacement() {
        assert_eq!(
            tokens(b"  a b\nc d\n", None, true).unwrap(),
            [b"a b", b"c d"]
        );
    }
    #[test]
    fn delimiter_escape() {
        assert_eq!(delimiter(r"\n").unwrap(), 10);
        assert_eq!(delimiter(r"\101").unwrap(), b'A');
        assert!(delimiter("ab").is_err());
    }
    #[test]
    fn no_permutation() {
        let o = parse(&["echo".into(), "-n".into()]).unwrap();
        assert_eq!(o.command, ["echo", "-n"]);
        assert!(o.number.is_none());
    }
}
