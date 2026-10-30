//! procps-ng subset: `pgrep` lists processes whose name (or full command line with `-f`)
//! matches an extended regular expression, and `pkill` signals them. Process data comes from
//! `sysinfo`. A process never matches itself.
//!
//! Exit status: 0 when something matched (pkill: and was signalled), 1 when nothing did,
//! 2 for usage and pattern errors.

use std::io::Write;

use regex::{Regex, RegexBuilder};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, Users};

use super::signal::{send_signal, signal_failure_reason, signal_name, signal_number};
use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const SIGTERM: i32 = 15;

const HELP_PGREP: &str = r#"Usage: pgrep [options] <pattern>

Options:
  -d, --delimiter=STRING  separate PIDs with STRING (default newline)
  -l, --list-name         list PID and process name
  -a, --list-full         list PID and full command line
  -v, --inverse           negate the match
  -c, --count             count matching processes
  -f, --full              match against the full command line
  -i, --ignore-case       match case insensitively
  -n, --newest            select the most recently started process
  -o, --oldest            select the least recently started process
  -x, --exact             match the whole name exactly
  -P, --parent=PPID,...   match only children of the given parents
  -u, --euid=ID,...       match by effective user (name or number)
  -U, --uid=ID,...        match by real user (name or number)
      --help              display this help and exit
      --version           output version information and exit

Exit status: 0 if one or more processes matched, 1 if none matched, 2 on usage
or pattern errors.
"#;

const HELP_PKILL: &str = r#"Usage: pkill [options] <pattern>

Options:
  -SIGNAL, --signal=SIGNAL  signal to send (name or number; default TERM)
  -e, --echo                display what is killed
  -v, --inverse             negate the match
  -f, --full                match against the full command line
  -i, --ignore-case         match case insensitively
  -n, --newest              select the most recently started process
  -o, --oldest              select the least recently started process
  -x, --exact               match the whole name exactly
  -P, --parent=PPID,...     match only children of the given parents
  -u, --euid=ID,...         match by effective user (name or number)
  -U, --uid=ID,...          match by real user (name or number)
      --help                display this help and exit
      --version             output version information and exit

Exit status: 0 if one or more processes were signalled, 1 otherwise.
"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pgrep,
    Pkill,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Pgrep => "pgrep",
            Tool::Pkill => "pkill",
        }
    }
}

/// The option table for one tool. `-U` is Unix only.
fn specs(tool: Tool) -> Vec<OptSpec> {
    let mut list = vec![
        OptSpec::flag("full", Some('f'), Some("full")),
        OptSpec::flag("ignore-case", Some('i'), Some("ignore-case")),
        OptSpec::flag("exact", Some('x'), Some("exact")),
        OptSpec::flag("newest", Some('n'), Some("newest")),
        OptSpec::flag("oldest", Some('o'), Some("oldest")),
        OptSpec::flag("inverse", Some('v'), Some("inverse")),
        OptSpec::req("parent", Some('P'), Some("parent")),
        OptSpec::req("euid", Some('u'), Some("euid")),
        OptSpec::flag("help", None, Some("help")),
        OptSpec::flag("version", None, Some("version")),
    ];
    if cfg!(unix) {
        list.push(OptSpec::req("uid", Some('U'), Some("uid")));
    }
    match tool {
        Tool::Pgrep => {
            list.push(OptSpec::flag("count", Some('c'), Some("count")));
            list.push(OptSpec::flag("list-name", Some('l'), Some("list-name")));
            list.push(OptSpec::flag("list-full", Some('a'), Some("list-full")));
            list.push(OptSpec::req("delimiter", Some('d'), Some("delimiter")));
        }
        Tool::Pkill => {
            list.push(OptSpec::req("signal", None, Some("signal")));
            list.push(OptSpec::flag("echo", Some('e'), Some("echo")));
        }
    }
    list
}

/// What the command line asks for, after parsing.
struct Request {
    pattern: Option<String>,
    full: bool,
    ignore_case: bool,
    exact: bool,
    inverse: bool,
    newest: bool,
    oldest: bool,
    parents: Vec<String>,
    euids: Vec<String>,
    uids: Vec<String>,
    count: bool,
    list_name: bool,
    list_full: bool,
    delimiter: String,
    signal: i32,
    echo: bool,
}

/// A snapshot of one process, with the fields matching and output need.
struct Row {
    pid: u32,
    ppid: Option<u32>,
    name: String,
    cmdline: String,
    euid: Option<String>,
    ruid: Option<String>,
    start: u64,
}

impl Row {
    /// The name as matched: on Windows the `.exe` suffix is dropped, as procps does there.
    fn match_name(&self) -> &str {
        let len = self.name.len();
        if cfg!(windows) && len > 4 {
            if let (Some(stem), Some(tail)) = (self.name.get(..len - 4), self.name.get(len - 4..)) {
                if tail.eq_ignore_ascii_case(".exe") {
                    return stem;
                }
            }
        }
        &self.name
    }

    /// The text `-f` matches: the command line, or the name when there is none.
    fn full_text(&self) -> &str {
        if self.cmdline.is_empty() {
            &self.name
        } else {
            &self.cmdline
        }
    }
}

/// Rewrites `-SIG` short forms (`-9`, `-KILL`) as `--signal=SIG` for `pkill`. Operands after
/// `--` are left alone.
fn expand_signal_shorthand(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut literal = false;
    for arg in args {
        if literal {
            out.push(arg.clone());
        } else if arg == "--" {
            literal = true;
            out.push(arg.clone());
        } else if let Some(rest) = arg.strip_prefix('-').filter(|rest| {
            !rest.is_empty() && !rest.starts_with('-') && signal_number(rest).is_some()
        }) {
            out.push(format!("--signal={rest}"));
        } else {
            out.push(arg.clone());
        }
    }
    out
}

fn usage(tool: Tool, message: &str) -> i32 {
    let error = UsageError {
        message: message.to_string(),
    };
    report_usage(tool.name(), &error, EXIT_TROUBLE)
}

fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Resolves user names to numeric ids. Numbers pass through unchanged.
fn resolve_users(list: &[String]) -> Result<Vec<String>, String> {
    let directory = Users::new_with_refreshed_list();
    let mut ids = Vec::with_capacity(list.len());
    for item in list {
        if item.bytes().all(|b| b.is_ascii_digit()) {
            ids.push(item.clone());
            continue;
        }
        let user = directory.list().iter().find(|user| user.name() == item);
        match user {
            Some(user) => ids.push((**user.id()).to_string()),
            None => return Err(format!("invalid user name: {item}")),
        }
    }
    Ok(ids)
}

fn build_regex(request: &Request) -> Result<Option<Regex>, String> {
    let Some(pattern) = &request.pattern else {
        return Ok(None);
    };
    let source = if request.exact {
        format!("^(?:{pattern})$")
    } else {
        pattern.clone()
    };
    RegexBuilder::new(&source)
        .case_insensitive(request.ignore_case)
        .build()
        .map(Some)
        .map_err(|error| {
            let text = error.to_string();
            text.lines().last().unwrap_or("invalid pattern").to_string()
        })
}

/// Reads every process except this one. Linux threads are skipped: they share their
/// process's command line, and procps lists processes only.
fn snapshot() -> Vec<Row> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    let own = std::process::id();
    let mut rows: Vec<Row> = system
        .processes()
        .iter()
        .filter(|(pid, process)| pid.as_u32() != own && process.thread_kind().is_none())
        .map(|(pid, process)| {
            let cmd: Vec<String> = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect();
            Row {
                pid: pid.as_u32(),
                ppid: process.parent().map(|parent| parent.as_u32()),
                name: process.name().to_string_lossy().into_owned(),
                cmdline: cmd.join(" "),
                euid: process.effective_user_id().map(|uid| (**uid).to_string()),
                ruid: process.user_id().map(|uid| (**uid).to_string()),
                start: process.start_time(),
            }
        })
        .collect();
    rows.sort_by_key(|row| row.pid);
    rows
}

/// True when `row` passes the criteria, before `-v` is applied.
fn criteria_hold(request: &Request, regex: Option<&Regex>, row: &Row) -> bool {
    if let Some(regex) = regex {
        let text = if request.full {
            row.full_text()
        } else {
            row.match_name()
        };
        if !regex.is_match(text) {
            return false;
        }
    }
    if !request.parents.is_empty()
        && !row
            .ppid
            .is_some_and(|ppid| request.parents.contains(&ppid.to_string()))
    {
        return false;
    }
    if !request.euids.is_empty()
        && !row
            .euid
            .as_ref()
            .is_some_and(|id| request.euids.contains(id))
    {
        return false;
    }
    if !request.uids.is_empty()
        && !row
            .ruid
            .as_ref()
            .is_some_and(|id| request.uids.contains(id))
    {
        return false;
    }
    true
}

/// The rows that satisfy the request, in PID order, reduced to one for `-n` and `-o`.
fn select<'a>(request: &Request, regex: Option<&Regex>, rows: &'a [Row]) -> Vec<&'a Row> {
    let mut hits: Vec<&Row> = rows
        .iter()
        .filter(|row| criteria_hold(request, regex, row) != request.inverse)
        .collect();
    if request.newest {
        hits = hits
            .into_iter()
            .max_by_key(|row| (row.start, row.pid))
            .into_iter()
            .collect();
    } else if request.oldest {
        hits = hits
            .into_iter()
            .min_by_key(|row| (row.start, row.pid))
            .into_iter()
            .collect();
    }
    hits
}

fn print_help(tool: Tool) -> i32 {
    let text = match tool {
        Tool::Pgrep => HELP_PGREP,
        Tool::Pkill => HELP_PKILL,
    };
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(text.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("{}: write error: {error}", tool.name());
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output(tool.name(), &mut out)
}

/// Parses the arguments into a request. `Err` carries the exit status; help and version
/// requests come back as `Ok(None)` after printing.
fn parse(tool: Tool, args: &[String]) -> Result<Result<Request, i32>, i32> {
    let args = if tool == Tool::Pkill {
        expand_signal_shorthand(args)
    } else {
        args.to_vec()
    };
    let table = specs(tool);
    let items = match Parser::new(tool.name(), &table).parse(&args) {
        Ok(items) => items,
        Err(error) => return Err(report_usage(tool.name(), &error, EXIT_TROUBLE)),
    };
    let mut request = Request {
        pattern: None,
        full: false,
        ignore_case: false,
        exact: false,
        inverse: false,
        newest: false,
        oldest: false,
        parents: Vec::new(),
        euids: Vec::new(),
        uids: Vec::new(),
        count: false,
        list_name: false,
        list_full: false,
        delimiter: "\n".to_string(),
        signal: SIGTERM,
        echo: false,
    };
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => return Ok(Err(print_help(tool))),
            Item::Opt { id: "version", .. } => return Ok(Err(print_version(tool.name()))),
            Item::Opt { id, value } => apply_option(tool, &mut request, id, value)?,
            Item::Operand(text) => operands.push(text),
        }
    }
    request.parents = parse_numbers(request.parents, tool)?;
    if operands.len() > 1 {
        return Err(usage(tool, "only one pattern can be provided"));
    }
    request.pattern = operands.pop();
    Ok(Ok(request))
}

fn parse_numbers(list: Vec<String>, tool: Tool) -> Result<Vec<String>, i32> {
    let mut out = Vec::new();
    for item in list {
        for part in split_list(&item) {
            let Ok(number) = part.parse::<u32>() else {
                return Err(usage(tool, &format!("not a number: {part}")));
            };
            out.push(number.to_string());
        }
    }
    Ok(out)
}

/// Stores one parsed option. Returns the exit status on an invalid value.
fn apply_option(
    tool: Tool,
    request: &mut Request,
    id: &str,
    value: Option<String>,
) -> Result<(), i32> {
    let text = value.unwrap_or_default();
    match id {
        "full" => request.full = true,
        "ignore-case" => request.ignore_case = true,
        "exact" => request.exact = true,
        "newest" => request.newest = true,
        "oldest" => request.oldest = true,
        "inverse" => request.inverse = true,
        "count" => request.count = true,
        "list-name" => request.list_name = true,
        "list-full" => request.list_full = true,
        "echo" => request.echo = true,
        "parent" => request.parents.push(text),
        "euid" => request.euids.extend(split_list(&text)),
        "uid" => request.uids.extend(split_list(&text)),
        "delimiter" => request.delimiter = text,
        "signal" => {
            request.signal = signal_number(&text).ok_or_else(|| {
                eprintln!("{}: unknown signal: {text}", tool.name());
                EXIT_TROUBLE
            })?;
        }
        _ => {}
    }
    Ok(())
}

/// Resolves the user lists in a request. Unknown names are a usage error.
fn resolve_request(tool: Tool, request: &mut Request) -> Result<(), i32> {
    request.euids = resolve_users(&request.euids).map_err(|message| usage(tool, &message))?;
    request.uids = resolve_users(&request.uids).map_err(|message| usage(tool, &message))?;
    Ok(())
}

/// Reports what `pgrep` or `pkill` prints for the selected rows.
fn print_matches(request: &Request, hits: &[&Row]) -> i32 {
    let mut text = String::new();
    if request.count {
        text.push_str(&format!("{}\n", hits.len()));
    } else if request.list_name || request.list_full {
        for row in hits {
            let shown = if request.list_full {
                row.full_text()
            } else {
                row.name.as_str()
            };
            text.push_str(&format!("{} {shown}\n", row.pid));
        }
    } else {
        let pids: Vec<String> = hits.iter().map(|row| row.pid.to_string()).collect();
        if !pids.is_empty() {
            text.push_str(&pids.join(&request.delimiter));
            text.push('\n');
        }
    }
    let written = write_stdout("pgrep", &text);
    if written != EXIT_OK {
        return written;
    }
    if hits.is_empty() {
        EXIT_FAIL
    } else {
        EXIT_OK
    }
}

/// Sends the request's signal to each selected row. Returns 0 when every signal was sent
/// and at least one process was selected, 1 otherwise.
fn signal_matches(request: &Request, hits: &[&Row]) -> i32 {
    let mut failed = false;
    let mut sent = 0;
    let mut echo = String::new();
    let name = signal_name(request.signal).unwrap_or("TERM");
    for row in hits {
        match send_signal(row.pid as i32, request.signal) {
            Ok(()) => {
                sent += 1;
                if request.echo {
                    echo.push_str(&format!("{} ({}) {name}\n", row.name, row.pid));
                }
            }
            Err(error) => {
                eprintln!(
                    "pkill: killing pid {} failed: {}",
                    row.pid,
                    signal_failure_reason(&error)
                );
                failed = true;
            }
        }
    }
    let written = if echo.is_empty() {
        EXIT_OK
    } else {
        write_stdout("pkill", &echo)
    };
    if written != EXIT_OK || sent == 0 || failed {
        EXIT_FAIL
    } else {
        EXIT_OK
    }
}

/// Writes text to stdout through the buffered writer, with BrokenPipe handling.
fn write_stdout(tool: &str, text: &str) -> i32 {
    let mut out = stdout_writer();
    if let Err(error) = out.write_all(text.as_bytes()) {
        if !is_broken_pipe(&error) {
            eprintln!("{tool}: write error: {error}");
            return EXIT_FAIL;
        }
        return EXIT_OK;
    }
    finish_output(tool, &mut out)
}

fn run_tool(tool: Tool, args: &[String]) -> i32 {
    let mut request = match parse(tool, args) {
        Ok(Ok(request)) => request,
        Ok(Err(code)) | Err(code) => return code,
    };
    if request.pattern.is_none()
        && request.parents.is_empty()
        && request.euids.is_empty()
        && request.uids.is_empty()
    {
        return usage(tool, "no matching criteria specified");
    }
    if let Err(code) = resolve_request(tool, &mut request) {
        return code;
    }
    let regex = match build_regex(&request) {
        Ok(regex) => regex,
        Err(message) => {
            eprintln!("{}: regex error: {message}", tool.name());
            return EXIT_TROUBLE;
        }
    };
    let rows = snapshot();
    let hits = select(&request, regex.as_ref(), &rows);
    match tool {
        Tool::Pgrep => print_matches(&request, &hits),
        Tool::Pkill if hits.is_empty() => EXIT_FAIL,
        Tool::Pkill => signal_matches(&request, &hits),
    }
}

pub fn pgrep(args: &[String]) -> i32 {
    run_tool(Tool::Pgrep, args)
}

pub fn pkill(args: &[String]) -> i32 {
    run_tool(Tool::Pkill, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: u32, name: &str, cmdline: &str, ppid: Option<u32>) -> Row {
        Row {
            pid,
            ppid,
            name: name.to_string(),
            cmdline: cmdline.to_string(),
            euid: Some("1000".to_string()),
            ruid: Some("1000".to_string()),
            start: u64::from(pid),
        }
    }

    fn request(pattern: Option<&str>) -> Request {
        Request {
            pattern: pattern.map(str::to_owned),
            full: false,
            ignore_case: false,
            exact: false,
            inverse: false,
            newest: false,
            oldest: false,
            parents: Vec::new(),
            euids: Vec::new(),
            uids: Vec::new(),
            count: false,
            list_name: false,
            list_full: false,
            delimiter: "\n".to_string(),
            signal: SIGTERM,
            echo: false,
        }
    }

    #[test]
    fn exact_match_anchors_the_whole_name() {
        let mut req = request(Some("sleep"));
        req.exact = true;
        let regex = build_regex(&req).unwrap();
        let rows = [
            row(10, "sleep", "sleep 5", None),
            row(11, "sleepy", "sleepy", None),
        ];
        let hits = select(&req, regex.as_ref(), &rows);
        assert_eq!(hits.iter().map(|r| r.pid).collect::<Vec<_>>(), vec![10]);
    }

    #[test]
    fn full_match_uses_the_command_line() {
        let mut req = request(Some("data\\.txt"));
        req.full = true;
        let regex = build_regex(&req).unwrap();
        let rows = [
            row(20, "cat", "cat data.txt", None),
            row(21, "cat", "cat other", None),
        ];
        let hits = select(&req, regex.as_ref(), &rows);
        assert_eq!(hits.iter().map(|r| r.pid).collect::<Vec<_>>(), vec![20]);
    }

    #[test]
    fn newest_and_oldest_pick_one_process() {
        let rows = [
            row(30, "w", "w", None),
            row(31, "w", "w", None),
            row(32, "w", "w", None),
        ];
        let mut req = request(Some("w"));
        req.newest = true;
        let regex = build_regex(&req).unwrap();
        assert_eq!(select(&req, regex.as_ref(), &rows)[0].pid, 32);
        req.newest = false;
        req.oldest = true;
        assert_eq!(select(&req, regex.as_ref(), &rows)[0].pid, 30);
    }

    #[test]
    fn parent_filter_and_inverse() {
        let rows = [row(40, "a", "a", Some(1)), row(41, "b", "b", Some(2))];
        let mut req = request(None);
        req.parents = vec!["2".to_string()];
        assert_eq!(select(&req, None, &rows)[0].pid, 41);
        req.parents.clear();
        req.pattern = Some("a".to_string());
        req.inverse = true;
        let regex = build_regex(&req).unwrap();
        assert_eq!(select(&req, regex.as_ref(), &rows)[0].pid, 41);
    }

    #[test]
    fn pkill_shorthand_becomes_signal_option() {
        let args: Vec<String> = ["-9", "-KILL", "-f", "--", "-x"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            expand_signal_shorthand(&args),
            vec!["--signal=9", "--signal=KILL", "-f", "--", "-x"]
        );
    }

    #[test]
    fn bad_pattern_is_reported() {
        assert!(build_regex(&request(Some("("))).is_err());
    }

    #[test]
    fn snapshot_excludes_this_process() {
        let own = std::process::id();
        assert!(snapshot().iter().all(|row| row.pid != own));
    }
}
