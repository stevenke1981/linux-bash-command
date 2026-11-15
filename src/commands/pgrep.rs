use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use regex::bytes::{Regex, RegexBuilder};
use sysinfo::{System, Uid, Users};

const PGREP_HELP:&str="Usage: pgrep [OPTION]... PATTERN\nList process IDs matching an extended regular expression.\n  -f, --full             match the full command line\n  -i, --ignore-case      ignore case\n  -x, --exact            require an exact match\n  -n, --newest           select the newest match\n  -o, --oldest           select the oldest match\n  -v, --inverse          invert the selection\n  -c, --count            print the match count\n  -P, --parent=PPID,...  match parent process IDs\n  -u, --euid=USER,...    match effective user IDs\n  -U, --uid=USER,...     match real user IDs (Unix)\n  -l, --list-name        print PID and process name\n  -a, --list-full        print PID and command line\n  -d, --delimiter=STR    use STR between entries\n      --help             display this help\n      --version          output version information\nExit status: 0 matched, 1 no match, 2 syntax, 3 fatal error.\n";
const PKILL_HELP:&str="Usage: pkill [OPTION]... PATTERN\nSignal processes matching an extended regular expression.\n  -f, --full             match the full command line\n  -i, --ignore-case      ignore case\n  -x, --exact            require an exact match\n  -n, --newest           select the newest match\n  -o, --oldest           select the oldest match\n  -v, --inverse          invert the selection\n  -c, --count            print the match count\n  -P, --parent=PPID,...  match parent process IDs\n  -u, --euid=USER,...    match effective user IDs\n  -U, --uid=USER,...     match real user IDs (Unix)\n      --signal=SIGNAL    signal name or number; -SIGNAL also accepted\n  -e, --echo             show successfully signalled processes\n      --help             display this help\n      --version          output version information\nExit status: 0 matched, 1 no match, 2 syntax, 3 fatal error.\n";

#[derive(Default)]
struct Options {
    full: bool,
    ignore_case: bool,
    exact: bool,
    newest: bool,
    oldest: bool,
    inverse: bool,
    count: bool,
    list_name: bool,
    list_full: bool,
    echo: bool,
    delimiter: Option<String>,
    parents: Vec<u32>,
    euid: Vec<Uid>,
    uid: Vec<Uid>,
    signal: i32,
}
struct Query {
    options: Options,
    regex: Option<Regex>,
}
#[derive(Clone, Debug)]
struct Row {
    pid: u32,
    parent: Option<u32>,
    name: Vec<u8>,
    command: Vec<u8>,
    start: u64,
    euid: Option<Uid>,
    uid: Option<Uid>,
}

fn user_ids(text: &str, users: &Users) -> Result<Vec<Uid>, String> {
    text.split(',')
        .map(|name| {
            if name.is_empty() {
                return Err("empty user ID".into());
            }
            name.parse::<Uid>()
                .ok()
                .or_else(|| {
                    users
                        .list()
                        .iter()
                        .find(|u| u.name() == name)
                        .map(|u| u.id().clone())
                })
                .ok_or_else(|| format!("invalid user name: {name}"))
        })
        .collect()
}
fn parents(text: &str) -> Result<Vec<u32>, String> {
    text.split(',')
        .map(|s| {
            s.parse::<u32>()
                .map_err(|_| format!("invalid parent PID: {s}"))
        })
        .collect()
}
fn specs(pkill: bool) -> Vec<OptSpec> {
    let mut s = vec![
        OptSpec::flag("full", 'f'),
        OptSpec::flag("ignore-case", 'i'),
        OptSpec::flag("exact", 'x'),
        OptSpec::flag("newest", 'n'),
        OptSpec::flag("oldest", 'o'),
        OptSpec::flag("inverse", 'v'),
        OptSpec::flag("count", 'c'),
        OptSpec::value("parent", 'P'),
        OptSpec::value("euid", 'u'),
        OptSpec::value("uid", 'U'),
        OptSpec::value("terminal", 't'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    if pkill {
        s.extend([OptSpec::value("signal", '\0'), OptSpec::flag("echo", 'e')]);
    } else {
        s.extend([
            OptSpec::flag("list-name", 'l'),
            OptSpec::flag("list-full", 'a'),
            OptSpec::value("delimiter", 'd'),
        ]);
    }
    s
}
pub(super) fn normalize_signal(argv: &[String], pkill: bool) -> Vec<String> {
    let mut result = Vec::new();
    let mut consume = false;
    let mut stopped = false;
    for arg in argv {
        if consume {
            result.push(arg.clone());
            consume = false;
            continue;
        }
        if arg == "--" {
            stopped = true;
            result.push(arg.clone());
            continue;
        }
        if !stopped
            && pkill
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg.len() > 1
            && super::kill::signal_number(&arg[1..]).is_some()
        {
            result.push(format!("--signal={}", &arg[1..]));
            continue;
        }
        consume = matches!(
            arg.as_str(),
            "-P" | "-u"
                | "-U"
                | "-d"
                | "-t"
                | "--parent"
                | "--euid"
                | "--uid"
                | "--delimiter"
                | "--terminal"
                | "--signal"
        );
        result.push(arg.clone());
    }
    result
}
fn apply_item(
    o: &mut Options,
    operands: &mut Vec<String>,
    item: Item,
    users: &Users,
) -> Result<(), String> {
    match item {
        Item::Opt("full", _) => o.full = true,
        Item::Opt("ignore-case", _) => o.ignore_case = true,
        Item::Opt("exact", _) => o.exact = true,
        Item::Opt("newest", _) => o.newest = true,
        Item::Opt("oldest", _) => o.oldest = true,
        Item::Opt("inverse", _) => o.inverse = true,
        Item::Opt("count", _) => o.count = true,
        Item::Opt("list-name", _) => o.list_name = true,
        Item::Opt("list-full", _) => o.list_full = true,
        Item::Opt("echo", _) => o.echo = true,
        Item::Opt("delimiter", v) => o.delimiter = v,
        Item::Opt("parent", Some(v)) => o.parents.extend(parents(&v)?),
        Item::Opt("euid", Some(v)) => o.euid.extend(user_ids(&v, users)?),
        Item::Opt("uid", Some(v)) => {
            if cfg!(windows) {
                return Err("option '--uid' is not supported by agentcmd on Windows".into());
            }
            o.uid.extend(user_ids(&v, users)?);
        }
        Item::Opt("signal", Some(v)) => {
            o.signal =
                super::kill::signal_number(&v).ok_or_else(|| format!("invalid signal: {v}"))?
        }
        Item::Opt("terminal", _) => {
            return Err("option '--terminal' is not supported by agentcmd".into())
        }
        Item::Opt("help" | "version", _) => unreachable!(),
        Item::Operand(v) => operands.push(v),
        _ => {}
    }
    Ok(())
}
fn parse(argv: &[String], pkill: bool, users: &Users) -> Result<Query, String> {
    let argv = normalize_signal(argv, pkill);
    let items = args::parse(&argv, &specs(pkill), true)?;
    let mut o = Options {
        signal: 15,
        ..Options::default()
    };
    let mut operands = Vec::new();
    for item in items {
        apply_item(&mut o, &mut operands, item, users)?;
    }
    if o.newest && o.oldest || o.inverse && (o.newest || o.oldest) {
        return Err("options -n, -o and -v cannot be combined".into());
    }
    if operands.len() > 1 {
        return Err("only one pattern can be provided".into());
    }
    if operands.is_empty() && o.parents.is_empty() && o.euid.is_empty() && o.uid.is_empty() {
        return Err("no matching criteria specified".into());
    }
    let regex = operands
        .first()
        .map(|s| {
            let pat = if o.exact {
                format!("^(?:{s})$")
            } else {
                s.clone()
            };
            RegexBuilder::new(&pat)
                .unicode(false)
                .case_insensitive(o.ignore_case)
                .build()
                .map_err(|e| e.to_string())
        })
        .transpose()?;
    Ok(Query { options: o, regex })
}
fn os_bytes(value: &std::ffi::OsStr) -> std::borrow::Cow<'_, [u8]> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::borrow::Cow::Borrowed(value.as_bytes())
    }
    #[cfg(not(unix))]
    {
        std::borrow::Cow::Owned(value.to_string_lossy().into_owned().into_bytes())
    }
}
fn collect() -> Vec<Row> {
    let system = System::new_all();
    system
        .processes()
        .iter()
        .map(|(pid, p)| {
            let mut name = os_bytes(p.name()).into_owned();
            if cfg!(windows)
                && name.len() >= 4
                && name[name.len() - 4..].eq_ignore_ascii_case(b".exe")
            {
                name.truncate(name.len() - 4);
            }
            let command = if p.cmd().is_empty() {
                name.clone()
            } else {
                let mut command = Vec::new();
                for (i, arg) in p.cmd().iter().enumerate() {
                    if i != 0 {
                        command.push(b' ');
                    }
                    command.extend_from_slice(&os_bytes(arg));
                }
                command
            };
            Row {
                pid: pid.as_u32(),
                parent: p.parent().map(|v| v.as_u32()),
                name,
                command,
                start: p.start_time(),
                euid: p
                    .effective_user_id()
                    .or_else(|| if cfg!(windows) { p.user_id() } else { None })
                    .cloned(),
                uid: p.user_id().cloned(),
            }
        })
        .collect()
}
fn selected(row: &Row, query: &Query, self_pid: u32) -> bool {
    let o = &query.options;
    if row.pid == self_pid {
        return false;
    }
    let matches = (o.parents.is_empty() || row.parent.is_some_and(|p| o.parents.contains(&p)))
        && (o.euid.is_empty() || row.euid.as_ref().is_some_and(|u| o.euid.contains(u)))
        && (o.uid.is_empty() || row.uid.as_ref().is_some_and(|u| o.uid.contains(u)))
        && query
            .regex
            .as_ref()
            .is_none_or(|r| r.is_match(if o.full { &row.command } else { &row.name }));
    if o.inverse {
        !matches
    } else {
        matches
    }
}
fn matches(mut rows: Vec<Row>, query: &Query, self_pid: u32) -> Vec<Row> {
    rows.retain(|r| selected(r, query, self_pid));
    rows.sort_by_key(|r| r.pid);
    if query.options.newest {
        rows.into_iter()
            .max_by_key(|r| (r.start, r.pid))
            .into_iter()
            .collect()
    } else if query.options.oldest {
        rows.into_iter()
            .min_by_key(|r| (r.start, r.pid))
            .into_iter()
            .collect()
    } else {
        rows
    }
}
fn print_pgrep(rows: &[Row], options: &Options) -> i32 {
    io::output("pgrep", |out| {
        if options.count {
            writeln!(out, "{}", rows.len())?;
            return Ok(());
        }
        let delimiter = options.delimiter.as_deref().unwrap_or("\n");
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                write!(out, "{delimiter}")?;
            }
            write!(out, "{}", row.pid)?;
            if options.list_full {
                write!(out, " ")?;
                out.write_all(&row.command)?;
            } else if options.list_name {
                write!(out, " ")?;
                out.write_all(&row.name)?;
            }
        }
        if !rows.is_empty() {
            writeln!(out)?;
        }
        Ok(())
    })
}
fn write_signals(
    rows: &[Row],
    options: &Options,
    out: &mut dyn std::io::Write,
    send: &mut dyn FnMut(i32, i32) -> std::io::Result<()>,
) -> std::io::Result<usize> {
    let mut success = 0;
    for row in rows {
        let result = i32::try_from(row.pid)
            .ok()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))
            .and_then(|pid| send(pid, options.signal));
        match result {
            Ok(()) => {
                success += 1;
                if options.echo {
                    out.write_all(&row.name)?;
                    writeln!(out, " killed (pid {})", row.pid)?;
                }
            }
            Err(e) => {
                eprintln!("pkill: ({}) - {}", row.pid, error::io_text(&e));
            }
        }
    }
    if options.count {
        writeln!(out, "{}", rows.len())?;
    }
    Ok(success)
}
fn signal_rows(rows: &[Row], options: &Options) -> i32 {
    let mut success = 0;
    let status = io::output("pkill", |out| {
        success = write_signals(rows, options, out, &mut super::kill::send)?;
        Ok(())
    });
    if status != 0 || success == 0 {
        1
    } else {
        0
    }
}
fn run(command: &str, argv: &[String], pkill: bool) -> i32 {
    let normalized = normalize_signal(argv, pkill);
    let items = match args::parse(&normalized, &specs(pkill), true) {
        Ok(v) => v,
        Err(e) => return error::usage(command, e, 2),
    };
    if let Some(Item::Opt(id, _)) = items
        .iter()
        .find(|v| matches!(v, Item::Opt("help" | "version", _)))
    {
        return args::help_version(
            command,
            &[format!("--{id}")],
            if pkill { PKILL_HELP } else { PGREP_HELP },
            true,
        )
        .unwrap_or(0);
    }
    let users = Users::new_with_refreshed_list();
    let query = match parse(argv, pkill, &users) {
        Ok(q) => q,
        Err(e) => return error::usage(command, e, 2),
    };
    let rows = matches(collect(), &query, std::process::id());
    if pkill {
        signal_rows(&rows, &query.options)
    } else {
        let status = print_pgrep(&rows, &query.options);
        if status != 0 {
            status
        } else {
            i32::from(rows.is_empty())
        }
    }
}
pub fn run_pgrep(argv: &[String]) -> i32 {
    run("pgrep", argv, false)
}
pub fn run_pkill(argv: &[String]) -> i32 {
    run("pkill", argv, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(pid: u32, name: &str, start: u64) -> Row {
        Row {
            pid,
            parent: Some(10),
            name: name.as_bytes().to_vec(),
            command: format!("{name} --arg").into_bytes(),
            start,
            euid: None,
            uid: None,
        }
    }
    fn query(pattern: &str, extras: &[&str]) -> Query {
        let mut a = extras.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        a.push("--".into());
        a.push(pattern.into());
        parse(&a, false, &Users::new_with_refreshed_list()).unwrap()
    }
    #[cfg(unix)]
    #[test]
    fn unix_os_strings_preserve_non_utf8_process_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let raw = std::ffi::OsStr::from_bytes(b"p4\xffname");
        assert_eq!(os_bytes(raw).as_ref(), b"p4\xffname");
    }
    #[test]
    fn matching() {
        let r = row(11, "Alpha", 2);
        for p in ["Al", "A.*a", "^Alpha$"] {
            assert!(selected(&r, &query(p, &[]), 20));
        }
        assert!(!selected(&r, &query("beta", &[]), 20));
    }
    #[test]
    fn flags() {
        let r = row(11, "Alpha", 2);
        assert!(selected(&r, &query("alpha", &["-i", "-x"]), 20));
        assert!(!selected(&r, &query("Al", &["-x"]), 20));
        assert!(selected(&r, &query("--arg", &["-f"]), 20));
        assert!(!selected(&r, &query("Alpha", &["-v"]), 20));
        assert!(!selected(&r, &query("Alpha", &[]), 11));
        let mut raw = row(12, "x", 1);
        raw.name = vec![0xff, b'X'];
        assert!(selected(&raw, &query("^.X$", &[]), 99));
    }
    #[test]
    fn ordering() {
        let rows = vec![row(2, "a", 3), row(3, "a", 4), row(4, "a", 1)];
        assert_eq!(matches(rows.clone(), &query("a", &["-n"]), 99)[0].pid, 3);
        assert_eq!(matches(rows, &query("a", &["-o"]), 99)[0].pid, 4);
    }
    #[test]
    fn parent_filters() {
        assert_eq!(parents("2,3"), Ok(vec![2, 3]));
        assert!(parents("x").is_err());
        assert!(selected(&row(11, "a", 1), &query("a", &["-P", "10"]), 99));
        assert!(!selected(&row(11, "a", 1), &query("a", &["-P", "11"]), 99));
    }
    #[test]
    fn inverse_applies_to_all_criteria_but_never_self() {
        let r = row(11, "a", 1);
        assert!(selected(&r, &query("a", &["-v", "-P", "12"]), 99));
        assert!(!selected(&r, &query("a", &["-v", "-P", "10"]), 99));
        assert!(!selected(&r, &query("a", &["-v", "-P", "12"]), 11));
        let args = ["-v".into(), "-P".into(), "12".into()];
        let q = parse(&args, false, &Users::new_with_refreshed_list()).unwrap();
        assert!(selected(&r, &q, 99));
    }
    #[test]
    fn count_reports_matches_even_when_signal_fails() {
        let rows = vec![row(2, "a", 1), row(3, "b", 2)];
        let options = Options {
            count: true,
            ..Options::default()
        };
        let mut output = Vec::new();
        let mut send = |pid, _| {
            if pid == 2 {
                Ok(())
            } else {
                Err(std::io::ErrorKind::PermissionDenied.into())
            }
        };
        assert_eq!(
            write_signals(&rows, &options, &mut output, &mut send).unwrap(),
            1
        );
        assert_eq!(output, b"2\n");
        output.clear();
        let mut fail = |_, _| Err(std::io::ErrorKind::PermissionDenied.into());
        assert_eq!(
            write_signals(&rows, &options, &mut output, &mut fail).unwrap(),
            0
        );
        assert_eq!(output, b"2\n");
    }
}
