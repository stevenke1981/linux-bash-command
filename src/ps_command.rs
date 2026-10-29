use std::collections::BTreeSet;
use sysinfo::System;

#[derive(Default)]
struct Options {
    all: bool,
    pids: BTreeSet<u32>,
    json: bool,
}

const HELP: &str = "Usage: ps [OPTIONS]\n\nList running processes. Output fields: PID, PPID, NAME and STATUS.\nThe default output lists all processes on the current host.\n\nOptions:\n  -a, -e, -A, --all  List all processes (same as the default)\n  -p PID[,PID...]    Show only the selected process IDs\n      --json         Emit a JSON array\n  -h, --help         Show this help\n\nThis is a compact cross-platform process view, not a full POSIX ps replacement.\nExit status: 0 on success, 2 on invalid options or system errors.\n";

fn parse(args: &[String]) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => return Ok(None),
            "-a" | "-e" | "-A" | "--all" => options.all = true,
            "--json" => options.json = true,
            "-p" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or("-p requires one or more process IDs")?;
                for value in value.split(',') {
                    if value.is_empty() {
                        return Err("-p contains an empty process ID".into());
                    }
                    let pid = value
                        .parse::<u32>()
                        .map_err(|_| format!("invalid process ID: {value}"))?;
                    options.pids.insert(pid);
                }
            }
            option if option.starts_with("-p") && option.len() > 2 => {
                for value in option[2..].split(',') {
                    if value.is_empty() {
                        return Err("-p contains an empty process ID".into());
                    }
                    let pid = value
                        .parse::<u32>()
                        .map_err(|_| format!("invalid process ID: {value}"))?;
                    options.pids.insert(pid);
                }
            }
            option => return Err(format!("unknown option: {option}")),
        }
        index += 1;
    }
    Ok(Some(options))
}

pub fn run(args: &[String]) -> i32 {
    let options = match parse(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            print!("{HELP}");
            return 0;
        }
        Err(error) => {
            eprintln!("ps: {error}\nTry 'ps --help' for usage.");
            return 2;
        }
    };

    let mut system = System::new_all();
    system.refresh_all();
    let mut rows = system
        .processes()
        .iter()
        .filter(|(pid, _)| options.pids.is_empty() || options.pids.contains(&pid.as_u32()))
        .map(|(pid, process)| {
            let parent = process.parent().map(|pid| pid.as_u32());
            let name = process
                .exe()
                .map(|path| path.display().to_string())
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| process.name().to_string_lossy().into_owned());
            let status = format!("{:?}", process.status());
            (pid.as_u32(), parent, name, status)
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.0);

    if options.json {
        let records = rows
            .iter()
            .map(|(pid, parent, name, status)| {
                serde_json::json!({
                    "pid": pid,
                    "parent_pid": parent,
                    "name": name,
                    "status": status,
                })
            })
            .collect::<Vec<_>>();
        match serde_json::to_writer(std::io::stdout().lock(), &records) {
            Ok(()) => println!(),
            Err(error) => {
                eprintln!("ps: cannot write JSON output: {error}");
                return 2;
            }
        }
    } else {
        println!("{:>7} {:>7} {:<20} STATUS", "PID", "PPID", "NAME");
        for (pid, parent, name, status) in rows {
            let short_name = name.rsplit(['/', '\\']).next().unwrap_or(&name);
            println!(
                "{:>7} {:>7} {:<20} {}",
                pid,
                parent.map_or_else(|| "-".to_string(), |pid| pid.to_string()),
                short_name,
                status
            );
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_pid_filters_and_json() {
        let args = ["-p".into(), "12,34".into(), "--json".into()];
        let options = parse(&args).unwrap().unwrap();
        assert!(options.pids.contains(&12));
        assert!(options.pids.contains(&34));
        assert!(options.json);
    }

    #[test]
    fn rejects_invalid_pid() {
        let args = ["-p".into(), "abc".into()];
        assert!(parse(&args).is_err());
    }
}
