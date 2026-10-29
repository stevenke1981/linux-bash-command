use agentcmd::{diff, files, search};

const HELP: &str = "agentcmd — read-only tools for coding agents

Usage: agentcmd <COMMAND> [OPTIONS]

Commands:
  search  Search text in files recursively
  diff    Compare two text files
  list    List files in a directory
  read    Read numbered lines from a file
  help    Show this help or help for a command

Use 'agentcmd <COMMAND> --help' for command-specific usage.
Exit status: 0 for success, 1 for no search matches or differing files, 2 for errors.";

fn main() {
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|_| {
            eprintln!("agentcmd: arguments must be valid UTF-8");
            std::process::exit(2);
        });

    let status = match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") if args.len() <= 1 => {
            println!("{HELP}");
            0
        }
        Some("--version" | "-V") if args.len() == 1 => {
            println!("agentcmd {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("help") if args.len() == 2 => {
            let help = vec!["--help".to_string()];
            match args[1].as_str() {
                "search" => search::run(&help),
                "diff" => diff::run(&help),
                "list" => files::list(&help),
                "read" => files::read(&help),
                command => {
                    eprintln!("agentcmd: unknown command '{command}'");
                    2
                }
            }
        }
        Some("search") => search::run(&args[1..]),
        Some("diff") => diff::run(&args[1..]),
        Some("list") => files::list(&args[1..]),
        Some("read") => files::read(&args[1..]),
        Some(command) => {
            eprintln!("agentcmd: unknown command or invalid arguments '{command}'\nUse 'agentcmd --help' for usage.");
            2
        }
        None => unreachable!(),
    };
    std::process::exit(status);
}
