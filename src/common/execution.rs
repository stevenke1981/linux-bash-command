//! Optional execution labels on stderr; command data and exit codes are untouched.
use std::io::{self, IsTerminal, Write};
use std::time::Instant;

pub fn run(name: &str, command: impl FnOnce() -> i32) -> i32 {
    let agent = std::env::var("AGENTCMD_AGENT").unwrap_or_default();
    let mode = std::env::var("AGENTCMD_COLOR").unwrap_or_default();
    let enabled = match mode.as_str() {
        "always" => true,
        "never" => false,
        _ => !agent.is_empty() && io::stderr().is_terminal(),
    } && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").as_deref() != Ok("dumb")
        && windows_console_ready(mode == "always");
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
        "\x1b[33m[RUNNING]\x1b[0m  {name:<12} | agent: \x1b[{color}m{label}\x1b[0m"
    );
    let started = Instant::now();
    let status = command();
    let elapsed = started.elapsed().as_secs_f64();
    let (state, state_color) = if status == 0 {
        ("DONE", 32)
    } else {
        ("EXIT", 31)
    };
    let _ = writeln!(
        io::stderr(),
        "\x1b[{state_color}m[{state:<7}]\x1b[0m  {name:<12} | agent: \x1b[{color}m{label}\x1b[0m | exit: {status} | time: {elapsed:.3}s"
    );
    status
}

/// Legacy Windows consoles print raw escape codes unless VT processing is on.
/// Enabling it is attempted in every mode; `auto` stays silent when it fails.
#[cfg(windows)]
fn windows_console_ready(forced: bool) -> bool {
    crossterm::ansi_support::supports_ansi() || forced
}

#[cfg(not(windows))]
fn windows_console_ready(_forced: bool) -> bool {
    true
}
