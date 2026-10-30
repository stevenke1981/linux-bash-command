//! Unix process handling for `timeout`. Unless `--foreground` is given, the command leads its
//! own process group, so a signal sent to the group also reaches what the command started.

use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};

use crate::commands::signal::send_signal;

/// A running command under supervision.
pub struct Monitored {
    child: Child,
    group: bool,
}

pub fn spawn(program: &Path, args: &[String], foreground: bool) -> io::Result<Monitored> {
    let mut command = Command::new(program);
    command.args(args);
    if !foreground {
        command.process_group(0);
    }
    let child = command.spawn()?;
    Ok(Monitored {
        child,
        group: !foreground,
    })
}

impl Monitored {
    pub fn child(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Sends `signal` to the command's process group, or to the command alone when it
    /// was not started in a group. Signal 0 sends nothing.
    pub fn send(&mut self, signal: i32) {
        if signal == 0 {
            return;
        }
        let pid = self.child.id() as i32;
        if self.group && send_signal(-pid, signal).is_ok() {
            return;
        }
        let _ = send_signal(pid, signal);
    }
}
