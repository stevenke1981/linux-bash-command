use agentcmd::registry::{self, UTILITIES};
use std::path::Path;

fn dispatch(name: &str, args: &[String]) -> i32 {
    match registry::find(name) {
        Some(utility) => agentcmd::run_with_status(utility.name, || (utility.run)(args)),
        None => {
            eprintln!("agentcmd: function/utility not found: {name}");
            1
        }
    }
}

fn main() {
    let argv0 = std::env::args_os().next().unwrap_or_default();
    let basename = Path::new(&argv0)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let basename = if basename.to_ascii_lowercase().ends_with(".exe") {
        &basename[..basename.len() - 4]
    } else {
        &basename
    };
    let args = agentcmd::arguments("agentcmd");
    let status = if basename == "agentcmd" || basename.ends_with("utils") {
        match args.first().map(String::as_str) {
            None => {
                eprintln!("agentcmd: missing argument");
                1
            }
            Some("--list") if args.len() == 1 => {
                for utility in UTILITIES {
                    println!("{}", utility.name);
                }
                0
            }
            Some("--list") => {
                eprintln!("agentcmd: invalid argument");
                1
            }
            Some("--version" | "-V") => {
                println!("agentcmd {} (multi-call binary)", env!("CARGO_PKG_VERSION"));
                0
            }
            Some("--help" | "-h" | "help") if args.len() == 1 => {
                println!("Usage: agentcmd <utility> [arguments]\n\nUtilities:");
                for utility in UTILITIES {
                    println!("  {:<8} {}", utility.name, utility.summary);
                }
                0
            }
            Some("--help" | "-h" | "help") if args.len() == 2 => {
                dispatch(&args[1], &["--help".to_owned()])
            }
            Some(arg) if arg.starts_with('-') => {
                eprintln!("agentcmd: unrecognized option '{arg}'");
                1
            }
            Some(name) => dispatch(name, &args[1..]),
        }
    } else {
        let matched = UTILITIES
            .iter()
            .filter(|u| basename.ends_with(u.name))
            .max_by_key(|u| u.name.len());
        match matched {
            Some(utility) => agentcmd::run_with_status(utility.name, || (utility.run)(&args)),
            None => {
                eprintln!("agentcmd: function/utility not found: {basename}");
                1
            }
        }
    };
    std::process::exit(status);
}
