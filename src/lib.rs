pub mod curl_command;
pub mod df_command;
pub mod diff;
pub mod files;
pub mod find_command;
pub mod fs_commands;
pub mod grep_command;
pub mod less_command;
pub mod ps_command;
pub mod search;
pub mod text_commands;

/// Formats a path for output. On Windows, separators become `/` so results can be
/// pasted back into Bash (Git Bash, MSYS2) without backslash escaping.
pub fn display_path(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

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
