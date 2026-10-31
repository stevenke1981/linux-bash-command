//! Entry hook shared by every executable and the multi-call dispatcher.
//!
//! Commands color and format their own output (see `common::color`); this hook
//! adds nothing to stdout or stderr and never changes the exit code.
pub fn run(_name: &str, command: impl FnOnce() -> i32) -> i32 {
    command()
}
