//! Optional execution labels on stderr; command data and exit codes are untouched.
use std::io::{self, IsTerminal, Write};

pub fn run(name: &str, command: impl FnOnce() -> i32) -> i32 {
    let agent = std::env::var("AGENTCMD_AGENT").unwrap_or_default();
    let mode = std::env::var("AGENTCMD_COLOR").unwrap_or_default();
    let enabled = match mode.as_str() {
        "always" => true,
        "never" => false,
        _ => !agent.is_empty() && io::stderr().is_terminal(),
    } && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").as_deref() != Ok("dumb");
    if !enabled {
        return command();
    }

    // Bound the label and render control characters visibly on one terminal line.
    let label: String = if agent.is_empty() { "agentcmd" } else { &agent }
        .chars()
        .take(80)
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect();
    let palette = [94, 95, 96, 92, 93, 91];
    let hash = agent.bytes().fold(0usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as usize)
    });
    let color = match std::env::var("AGENTCMD_AGENT_COLOR").as_deref() {
        Ok("blue") => 94,
        Ok("magenta") => 95,
        Ok("cyan") => 96,
        Ok("green") => 92,
        Ok("yellow") => 93,
        Ok("red") => 91,
        _ => palette[hash % palette.len()],
    };
    // Never hold stderr's lock across the command, which may write diagnostics.
    // Status display is best effort and must not change the command's exit code.
    let _ = writeln!(
        io::stderr(),
        "\x1b[{color}m[{label}]\x1b[0m \x1b[33mRUNNING\x1b[0m {name}"
    );
    let status = command();
    let (state, state_color) = if status == 0 {
        ("DONE", 32)
    } else {
        ("EXIT", 31)
    };
    let _ = writeln!(
        io::stderr(),
        "\x1b[{color}m[{label}]\x1b[0m \x1b[{state_color}m{state}\x1b[0m {name} (exit {status})"
    );
    status
}
