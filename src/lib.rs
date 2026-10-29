pub mod curl_command;
pub mod df_command;
pub mod diff;
pub mod files;
pub mod fs_commands;
pub mod grep_command;
pub mod less_command;
pub mod ps_command;
pub mod search;
pub mod text_commands;

pub fn arguments(command: &str) -> Vec<String> {
    std::env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|_| {
            eprintln!("{command}: arguments must be valid UTF-8");
            std::process::exit(2);
        })
}
