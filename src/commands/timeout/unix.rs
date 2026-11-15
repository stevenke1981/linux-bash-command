use std::{
    io,
    os::unix::process::CommandExt,
    process::{Child, Command},
};

pub(super) struct Group {
    foreground: bool,
}
pub(super) fn spawn(
    command: &mut Command,
    foreground: bool,
) -> Result<(Child, Group), super::SpawnError> {
    if !foreground {
        command.process_group(0);
    }
    command
        .spawn()
        .map(|child| (child, Group { foreground }))
        .map_err(super::SpawnError::Command)
}
impl Group {
    pub(super) fn signal(&self, child: &mut Child, signal: i32) -> io::Result<()> {
        let pid =
            i32::try_from(child.id()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        match super::super::kill::send(if self.foreground { pid } else { -pid }, signal) {
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            other => other,
        }
    }
}
