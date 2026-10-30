use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::io::{self, Write};
use sysinfo::{
    ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, Users,
    MINIMUM_CPU_UPDATE_INTERVAL,
};

const HELP: &str = "Usage: ps [OPTIONS]
       ps [aux | ax | -ef | -e | -A | -W] [OPTIONS]

List running processes on the current host.

Formats (default: PID PPID NAME STATUS):
  aux, u          USER PID %CPU %MEM VSZ RSS TTY STAT START TIME COMMAND
  -f, -ef         UID PID PPID C STIME TTY TIME CMD
  -o FIELDS       Custom columns, comma-separated (also repeatable):
                  pid ppid user uid comm args cmd command %cpu pcpu %mem pmem
                  rss vsz stat state etime start lstart time tty
                  Use name= for a blank header or name=TEXT for a custom header.

Selection:
  -e, -A, -a, -W  All processes (the default; accepted for compatibility)
  -p PID[,PID...] Only the listed process IDs
  -u USER[,...]   Only processes owned by USER (name or numeric ID; best effort)
  -C NAME[,...]   Only processes whose command name is NAME

Output:
      --sort=KEY[,KEY]  Sort; prefix - for descending. Keys: pid ppid %cpu %mem rss vsz
                        comm args user
      --no-headers, h   Omit the header line
      --json            Emit a JSON array of the selected processes
  -h, --help            Show this help

Notes: TIME and TTY are shown as '?' because the OS statistics are not available here.
START and STIME are shown in UTC. %CPU is measured over about 200 ms when requested.
The process name and command come from the operating system.
Exit status: 0 on success, 1 when a filter selected no processes, 2 on invalid options.
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Pid,
    Ppid,
    User,
    Comm,
    Args,
    Cpu,
    CpuInt,
    Mem,
    Rss,
    Vsz,
    Stat,
    StatusName,
    Etime,
    Start,
    Time,
    Tty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Right,
}

#[derive(Clone)]
struct Column {
    field: Field,
    header: String,
}

fn column(field: Field, header: &str) -> Column {
    Column {
        field,
        header: header.to_string(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Pid,
    Ppid,
    Cpu,
    Mem,
    Rss,
    Vsz,
    Comm,
    Args,
    User,
}

#[derive(Default)]
struct Options {
    all: bool,
    full: bool,
    user_format: bool,
    no_headers: bool,
    json: bool,
    pids: BTreeSet<u32>,
    users: Vec<String>,
    commands: Vec<String>,
    fields: Vec<Column>,
    sort: Vec<(SortKey, bool)>,
}

enum Parsed {
    Help,
    Run(Options),
}

fn parse_fields(text: &str, fields: &mut Vec<Column>) -> Result<(), String> {
    for item in text.split(',').filter(|item| !item.is_empty()) {
        let (name, header) = match item.split_once('=') {
            Some((name, header)) => (name, Some(header.to_string())),
            None => (item, None),
        };
        let (field, default_header) = field_for(name)?;
        let header = header.unwrap_or_else(|| default_header.to_string());
        fields.push(Column { field, header });
    }
    Ok(())
}

fn field_for(name: &str) -> Result<(Field, &'static str), String> {
    let mapped = match name {
        "pid" => (Field::Pid, "PID"),
        "ppid" => (Field::Ppid, "PPID"),
        "user" | "uname" => (Field::User, "USER"),
        "uid" => (Field::User, "UID"),
        "comm" | "ucomm" => (Field::Comm, "COMMAND"),
        "args" | "cmd" | "command" => (Field::Args, "COMMAND"),
        "%cpu" | "pcpu" | "cpu" => (Field::Cpu, "%CPU"),
        "%mem" | "pmem" => (Field::Mem, "%MEM"),
        "rss" | "rsz" => (Field::Rss, "RSS"),
        "vsz" | "vsize" => (Field::Vsz, "VSZ"),
        "stat" | "state" | "s" => (Field::Stat, "STAT"),
        "etime" => (Field::Etime, "ELAPSED"),
        "start" | "lstart" | "stime" => (Field::Start, "START"),
        "time" | "cputime" => (Field::Time, "TIME"),
        "tty" | "tt" | "tname" => (Field::Tty, "TT"),
        _ => return Err(format!("unknown output field: {name}")),
    };
    Ok(mapped)
}

fn sort_key(name: &str) -> Result<SortKey, String> {
    Ok(match name {
        "pid" => SortKey::Pid,
        "ppid" => SortKey::Ppid,
        "%cpu" | "pcpu" | "cpu" => SortKey::Cpu,
        "%mem" | "pmem" | "mem" => SortKey::Mem,
        "rss" => SortKey::Rss,
        "vsz" => SortKey::Vsz,
        "comm" => SortKey::Comm,
        "args" | "cmd" | "command" => SortKey::Args,
        "user" | "uname" => SortKey::User,
        _ => return Err(format!("unknown sort key: {name}")),
    })
}

fn parse_sort(text: &str, options: &mut Options) -> Result<(), String> {
    for item in text.split(',').filter(|item| !item.is_empty()) {
        let (descending, name) = match item.strip_prefix('-') {
            Some(name) => (true, name),
            None => (false, item.strip_prefix('+').unwrap_or(item)),
        };
        options.sort.push((sort_key(name)?, descending));
    }
    Ok(())
}

fn parse_pids(text: &str, pids: &mut BTreeSet<u32>) -> Result<(), String> {
    for value in text.split(',') {
        if value.is_empty() {
            return Err("-p contains an empty process ID".into());
        }
        let pid = value
            .parse::<u32>()
            .map_err(|_| format!("invalid process ID: {value}"))?;
        pids.insert(pid);
    }
    Ok(())
}

fn split_names(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(',')
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

/// Applies one option that takes a value. `inline` is the text after `=` or after the
/// letter in a cluster; otherwise the next argument is used.
fn apply_value_option(
    name: &str,
    inline: Option<&str>,
    args: &[String],
    index: &mut usize,
    options: &mut Options,
) -> Result<(), String> {
    let value = match inline {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => {
            *index += 1;
            args.get(*index)
                .cloned()
                .ok_or_else(|| format!("option {name} requires a value"))?
        }
    };
    match name {
        "p" | "pid" => parse_pids(&value, &mut options.pids),
        "u" | "U" | "user" => {
            options.users.extend(split_names(&value));
            Ok(())
        }
        "C" | "command-name" => {
            options.commands.extend(split_names(&value));
            Ok(())
        }
        "o" | "O" | "format" => parse_fields(&value, &mut options.fields),
        "sort" => parse_sort(&value, options),
        _ => Err(format!("unknown option: {name}")),
    }
}

fn apply_flag(letter: char, options: &mut Options) -> Result<(), String> {
    match letter {
        'a' | 'A' | 'e' | 'W' => options.all = true,
        'x' | 'w' => {}
        'f' => options.full = true,
        'u' => options.user_format = true,
        'h' => options.no_headers = true,
        other => return Err(format!("unknown option: -{other}")),
    }
    Ok(())
}

fn parse_long(
    arg: &str,
    args: &[String],
    index: &mut usize,
    options: &mut Options,
) -> Result<(), String> {
    let (name, inline) = match arg.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (arg, None),
    };
    match name {
        "all" => {
            options.all = true;
            Ok(())
        }
        "json" => {
            options.json = true;
            Ok(())
        }
        "no-headers" | "no-heading" => {
            options.no_headers = true;
            Ok(())
        }
        "sort" | "pid" | "user" | "format" | "command-name" => {
            apply_value_option(name, inline, args, index, options)
        }
        _ => Err(format!("unknown option: --{name}")),
    }
}

/// Parses a dash cluster such as `-ef` or `-p12`. A value-taking letter consumes the rest
/// of the cluster, or the next argument when the rest is empty.
fn parse_dash_cluster(
    cluster: &str,
    args: &[String],
    index: &mut usize,
    options: &mut Options,
) -> Result<(), String> {
    for (position, letter) in cluster.char_indices() {
        if matches!(letter, 'p' | 'u' | 'U' | 'C' | 'o' | 'O') {
            let rest = &cluster[position + letter.len_utf8()..];
            return apply_value_option(&letter.to_string(), Some(rest), args, index, options);
        }
        apply_flag(letter, options)?;
    }
    Ok(())
}

/// Parses BSD-style letters such as `aux` or `ax`, which need no dash.
fn parse_bsd_letters(letters: &str, options: &mut Options) -> Result<(), String> {
    for letter in letters.chars() {
        apply_flag(letter, options)?;
    }
    Ok(())
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "-h" || arg == "--help" {
            return Ok(Parsed::Help);
        } else if let Some(long) = arg.strip_prefix("--") {
            parse_long(long, args, &mut index, &mut options)?;
        } else if let Some(cluster) = arg.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            if arg == "-l" {
                return Err("-l (long format) is not supported; use -o or -f".into());
            }
            parse_dash_cluster(cluster, args, &mut index, &mut options)?;
        } else if !arg.is_empty() && arg.chars().all(|letter| letter.is_ascii_alphabetic()) {
            parse_bsd_letters(arg, &mut options)?;
        } else {
            return Err(format!("unknown option: {arg}"));
        }
        index += 1;
    }
    Ok(Parsed::Run(options))
}

pub fn run(args: &[String]) -> i32 {
    let options = match parse(args) {
        Ok(Parsed::Run(options)) => options,
        Ok(Parsed::Help) => {
            print!("{HELP}");
            return 0;
        }
        Err(error) => {
            eprintln!("ps: {error}\nTry 'ps --help' for usage.");
            return 2;
        }
    };
    let mut rows = collect_rows(&options);
    rows.sort_by(|left, right| compare_rows(left, right, &options.sort));
    let printed = if options.json {
        print_json(&rows)
    } else {
        print_table(&columns_for(&options), &rows, options.no_headers)
    };
    if printed != 0 {
        return printed;
    }
    // A filter that matched nothing is reported with status 1, as procps ps does.
    if options.has_filter() && rows.is_empty() {
        1
    } else {
        0
    }
}

impl Options {
    fn has_filter(&self) -> bool {
        !self.pids.is_empty() || !self.users.is_empty() || !self.commands.is_empty()
    }

    fn needs_cpu(&self) -> bool {
        let cpu_column = columns_for(self)
            .iter()
            .any(|column| matches!(column.field, Field::Cpu | Field::CpuInt));
        let cpu_sort = self.sort.iter().any(|(key, _)| matches!(key, SortKey::Cpu));
        cpu_column || cpu_sort || self.json
    }
}

fn columns_for(options: &Options) -> Vec<Column> {
    if !options.fields.is_empty() {
        return options.fields.clone();
    }
    if options.user_format {
        return vec![
            column(Field::User, "USER"),
            column(Field::Pid, "PID"),
            column(Field::Cpu, "%CPU"),
            column(Field::Mem, "%MEM"),
            column(Field::Vsz, "VSZ"),
            column(Field::Rss, "RSS"),
            column(Field::Tty, "TTY"),
            column(Field::Stat, "STAT"),
            column(Field::Start, "START"),
            column(Field::Time, "TIME"),
            column(Field::Args, "COMMAND"),
        ];
    }
    if options.full {
        return vec![
            column(Field::User, "UID"),
            column(Field::Pid, "PID"),
            column(Field::Ppid, "PPID"),
            column(Field::CpuInt, "C"),
            column(Field::Start, "STIME"),
            column(Field::Tty, "TTY"),
            column(Field::Time, "TIME"),
            column(Field::Args, "CMD"),
        ];
    }
    vec![
        column(Field::Pid, "PID"),
        column(Field::Ppid, "PPID"),
        column(Field::Comm, "NAME"),
        column(Field::StatusName, "STATUS"),
    ]
}

/// One process, with every value already converted to display form.
struct Row {
    pid: u32,
    parent: Option<u32>,
    user: Option<String>,
    /// Full executable path when known, else the process name.
    path_name: String,
    /// Basename of the executable or name, used for COMMAND/NAME.
    comm: String,
    args: String,
    cpu: f32,
    mem: f32,
    rss_kib: u64,
    vsz_kib: u64,
    stat: char,
    status_name: String,
    run_secs: u64,
    start_secs: u64,
}

fn collect_rows(options: &Options) -> Vec<Row> {
    let mut system = System::new_all();
    system.refresh_all();
    if options.needs_cpu() {
        std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );
    }
    let users = Users::new_with_refreshed_list();
    let total_memory = system.total_memory();
    system
        .processes()
        .iter()
        .map(|(pid, process)| {
            let exe = process.exe().map(|path| path.display().to_string());
            let comm = process.name().to_string_lossy().into_owned();
            let comm_basename = exe
                .as_deref()
                .and_then(|path| path.rsplit(['/', '\\']).next())
                .filter(|name| !name.is_empty())
                .map_or_else(|| comm.clone(), str::to_string);
            let args = if process.cmd().is_empty() {
                comm.clone()
            } else {
                process
                    .cmd()
                    .iter()
                    .map(|part| part.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let memory = process.memory();
            let mem = if total_memory == 0 {
                0.0
            } else {
                (memory as f64 * 100.0 / total_memory as f64) as f32
            };
            Row {
                pid: pid.as_u32(),
                parent: process.parent().map(|parent| parent.as_u32()),
                user: process
                    .user_id()
                    .and_then(|uid| users.get_user_by_id(uid))
                    .map(|user| user.name().to_string()),
                path_name: exe.filter(|path| !path.is_empty()).unwrap_or(comm),
                comm: comm_basename,
                args,
                cpu: process.cpu_usage(),
                mem,
                rss_kib: memory / 1024,
                vsz_kib: process.virtual_memory() / 1024,
                stat: stat_letter(process.status()),
                status_name: format!("{:?}", process.status()),
                run_secs: process.run_time(),
                start_secs: process.start_time(),
            }
        })
        .filter(|row| selected(row, options))
        .collect()
}

fn selected(row: &Row, options: &Options) -> bool {
    let pid_ok = options.pids.is_empty() || options.pids.contains(&row.pid);
    let user_ok = options.users.is_empty()
        || row
            .user
            .as_ref()
            .is_some_and(|user| options.users.contains(user));
    let command_ok = options.commands.is_empty() || options.commands.contains(&row.comm);
    pid_ok && user_ok && command_ok
}

fn stat_letter(status: ProcessStatus) -> char {
    match status {
        ProcessStatus::Run => 'R',
        ProcessStatus::Sleep => 'S',
        ProcessStatus::Idle => 'I',
        ProcessStatus::Stop => 'T',
        ProcessStatus::Zombie => 'Z',
        ProcessStatus::Dead => 'D',
        ProcessStatus::Tracing => 't',
        _ => '?',
    }
}

fn compare_rows(left: &Row, right: &Row, keys: &[(SortKey, bool)]) -> Ordering {
    for (key, descending) in keys {
        let order = compare_key(left, right, *key);
        let order = if *descending { order.reverse() } else { order };
        if order != Ordering::Equal {
            return order;
        }
    }
    left.pid.cmp(&right.pid)
}

fn compare_key(left: &Row, right: &Row, key: SortKey) -> Ordering {
    match key {
        SortKey::Pid => left.pid.cmp(&right.pid),
        SortKey::Ppid => left.parent.cmp(&right.parent),
        SortKey::Cpu => left.cpu.total_cmp(&right.cpu),
        SortKey::Mem => left.mem.total_cmp(&right.mem),
        SortKey::Rss => left.rss_kib.cmp(&right.rss_kib),
        SortKey::Vsz => left.vsz_kib.cmp(&right.vsz_kib),
        SortKey::Comm => left.comm.cmp(&right.comm),
        SortKey::Args => left.args.cmp(&right.args),
        SortKey::User => left.user.cmp(&right.user),
    }
}

fn alignment(field: Field) -> Align {
    match field {
        Field::Pid
        | Field::Ppid
        | Field::CpuInt
        | Field::Cpu
        | Field::Mem
        | Field::Rss
        | Field::Vsz => Align::Right,
        _ => Align::Left,
    }
}

fn cell(field: Field, row: &Row) -> String {
    match field {
        Field::Pid => row.pid.to_string(),
        Field::Ppid => row
            .parent
            .map_or_else(|| "?".to_string(), |pid| pid.to_string()),
        Field::User => row.user.clone().unwrap_or_else(|| "?".to_string()),
        Field::Comm => row.comm.clone(),
        Field::Args => row.args.clone(),
        Field::Cpu => format!("{:.1}", row.cpu),
        Field::CpuInt => format!("{:.0}", row.cpu),
        Field::Mem => format!("{:.1}", row.mem),
        Field::Rss => row.rss_kib.to_string(),
        Field::Vsz => row.vsz_kib.to_string(),
        Field::Stat => row.stat.to_string(),
        Field::StatusName => row.status_name.clone(),
        Field::Etime => elapsed_text(row.run_secs),
        Field::Start => clock_text(row.start_secs),
        Field::Time | Field::Tty => "?".to_string(),
    }
}

/// `[[dd-]hh:]mm:ss`, as ps prints elapsed time.
fn elapsed_text(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;
    if days > 0 {
        format!("{days}-{hours:02}:{minutes:02}:{secs:02}")
    } else if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

/// `HH:MM` in UTC for a Unix timestamp, or `?` when unknown.
fn clock_text(unix_seconds: u64) -> String {
    if unix_seconds == 0 {
        return "?".to_string();
    }
    let day_seconds = unix_seconds % 86_400;
    format!(
        "{:02}:{:02}",
        day_seconds / 3_600,
        (day_seconds % 3_600) / 60
    )
}

fn print_table(columns: &[Column], rows: &[Row], no_headers: bool) -> i32 {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .map(|column| cell(column.field, row))
                .collect()
        })
        .collect();
    let widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let values = cells.iter().map(|line| line[index].chars().count());
            let header = if no_headers {
                0
            } else {
                column.header.chars().count()
            };
            values.fold(header, usize::max)
        })
        .collect();
    let headers: Vec<String> = columns.iter().map(|column| column.header.clone()).collect();
    let mut out = String::new();
    let show_header = !no_headers && headers.iter().any(|header| !header.is_empty());
    if show_header {
        push_line(&mut out, columns, &headers, &widths);
    }
    for line in &cells {
        push_line(&mut out, columns, line, &widths);
    }
    write_stdout(&out)
}

fn push_line(out: &mut String, columns: &[Column], values: &[String], widths: &[usize]) {
    let last = columns.len().saturating_sub(1);
    let mut pieces = Vec::with_capacity(columns.len());
    for (index, column) in columns.iter().enumerate() {
        let value = &values[index];
        let width = widths[index];
        let piece = if index == last && alignment(column.field) == Align::Left {
            value.clone()
        } else {
            match alignment(column.field) {
                Align::Right => format!("{value:>width$}"),
                Align::Left => format!("{value:<width$}"),
            }
        };
        pieces.push(piece);
    }
    out.push_str(pieces.join(" ").trim_end());
    out.push('\n');
}

fn print_json(rows: &[Row]) -> i32 {
    let records = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "pid": row.pid,
                "parent_pid": row.parent,
                "name": row.path_name,
                "status": row.status_name,
                "user": row.user,
                "cpu_percent": row.cpu,
                "memory_percent": row.mem,
                "rss_kib": row.rss_kib,
                "vsz_kib": row.vsz_kib,
                "command": row.args,
            })
        })
        .collect::<Vec<_>>();
    let mut text = match serde_json::to_string(&records) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("ps: cannot write JSON output: {error}");
            return 2;
        }
    };
    text.push('\n');
    write_stdout(&text)
}

/// Writes to stdout, treating a closed pipe as a normal end of output.
fn write_stdout(text: &str) -> i32 {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            eprintln!("ps: cannot write output: {error}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{elapsed_text, parse, Field, Parsed};

    fn owned(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    fn options(args: &[&str]) -> super::Options {
        match parse(&owned(args)) {
            Ok(Parsed::Run(options)) => options,
            _ => panic!("expected options for {args:?}"),
        }
    }

    #[test]
    fn parses_pid_filters_and_json() {
        let parsed = options(&["-p", "12,34", "--json"]);
        assert!(parsed.pids.contains(&12));
        assert!(parsed.pids.contains(&34));
        assert!(parsed.json);
    }

    #[test]
    fn rejects_invalid_pid() {
        assert!(parse(&owned(&["-p", "abc"])).is_err());
    }

    #[test]
    fn accepts_bsd_style_and_dash_clusters() {
        assert!(options(&["aux"]).user_format);
        assert!(options(&["ax"]).all);
        let ef = options(&["-ef"]);
        assert!(ef.full && ef.all);
        assert!(options(&["-A"]).all);
    }

    #[test]
    fn custom_fields_and_headers() {
        let parsed = options(&["-o", "pid,comm=,%cpu"]);
        let headers: Vec<(Field, &str)> = parsed
            .fields
            .iter()
            .map(|column| (column.field, column.header.as_str()))
            .collect();
        assert_eq!(
            headers,
            vec![(Field::Pid, "PID"), (Field::Comm, ""), (Field::Cpu, "%CPU")]
        );
    }

    #[test]
    fn rejects_unknown_fields_and_long_format() {
        assert!(parse(&owned(&["-o", "nosuch"])).is_err());
        assert!(parse(&owned(&["-l"])).is_err());
    }

    #[test]
    fn formats_elapsed_time() {
        assert_eq!(elapsed_text(65), "01:05");
        assert_eq!(elapsed_text(3_725), "01:02:05");
        assert_eq!(elapsed_text(90_061), "1-01:01:01");
    }
}
