pub mod cd_command;
pub mod commands;
pub mod common;
pub mod curl_command;
pub mod df_command;
pub mod diff;
pub mod files;
pub mod find_command;
pub mod fs_commands;
pub mod grep_command;
pub mod less_command;
pub mod ps_command;
pub mod registry;
pub mod search;
pub mod search_color;
pub mod text_commands;

pub use common::execution::run as run_with_status;
pub use common::path::display_path;

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

#[cfg(test)]
mod tests {
    use super::display_path;
    use std::path::Path;

    #[test]
    fn display_path_uses_forward_slashes_on_windows() {
        let shown = display_path(Path::new("src").join("bin").as_path());
        assert_eq!(shown, "src/bin");
    }
}
