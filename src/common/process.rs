use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub fn env_name_eq(left: &OsStr, right: &OsStr) -> bool {
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

pub fn resolve(name: &OsStr, environment: &[(OsString, OsString)]) -> io::Result<PathBuf> {
    resolve_at(name, environment, None)
}

fn resolve_at(
    name: &OsStr,
    environment: &[(OsString, OsString)],
    directory: Option<&Path>,
) -> io::Result<PathBuf> {
    let text = name.to_string_lossy();
    if Path::new(name).is_absolute()
        || text.contains('/')
        || (cfg!(windows) && (text.contains('\\') || text.contains(':')))
    {
        return Ok(PathBuf::from(name));
    }
    let path = environment
        .iter()
        .find(|(k, _)| env_name_eq(k, OsStr::new("PATH")))
        .map(|(_, v)| v.clone());
    if cfg!(windows) && path.is_none() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    let path = path.unwrap_or_else(|| OsString::from("/bin:/usr/bin"));
    #[cfg(windows)]
    let extensions: Vec<String> = environment
        .iter()
        .find(|(k, _)| env_name_eq(k, OsStr::new("PATHEXT")))
        .map(|(_, v)| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    let mut denied = false;
    for entry in std::env::split_paths(&path) {
        let entry = if entry.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            entry
        };
        let candidate = entry.join(name);
        let checked = directory
            .filter(|_| candidate.is_relative())
            .map_or_else(|| candidate.clone(), |base| base.join(&candidate));
        if let Ok(metadata) = checked.metadata() {
            if !metadata.is_file() {
                denied = true;
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    denied = true;
                    continue;
                }
            }
            return Ok(candidate);
        }
        #[cfg(windows)]
        if Path::new(name).extension().is_none() {
            for extension in &extensions {
                let candidate = entry.join(format!("{text}{extension}"));
                let checked = directory
                    .filter(|_| candidate.is_relative())
                    .map_or_else(|| candidate.clone(), |base| base.join(&candidate));
                if checked.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }
    Err(io::Error::from(if denied {
        io::ErrorKind::PermissionDenied
    } else {
        io::ErrorKind::NotFound
    }))
}

pub fn command(name: &OsStr, environment: &[(OsString, OsString)]) -> io::Result<Command> {
    command_in(name, environment, None)
}

pub fn command_in(
    name: &OsStr,
    environment: &[(OsString, OsString)],
    directory: Option<&Path>,
) -> io::Result<Command> {
    let executable = resolve_at(name, environment, directory)?;
    let executable = if let Some(directory) = directory.filter(|_| executable.is_relative()) {
        std::env::current_dir()?.join(directory).join(executable)
    } else {
        executable
    };
    #[cfg(windows)]
    let executable = {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let wide = executable
            .as_os_str()
            .encode_wide()
            .map(|unit| {
                if unit == u16::from(b'/') {
                    u16::from(b'\\')
                } else {
                    unit
                }
            })
            .collect::<Vec<_>>();
        OsString::from_wide(&wide)
    };
    let mut command = Command::new(executable);
    command
        .env_clear()
        .envs(environment.iter().map(|(k, v)| (k, v)));
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    Ok(command)
}

pub fn error_code(error: &io::Error) -> i32 {
    if error.kind() == io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

pub fn status_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
    }
    #[cfg(not(unix))]
    {
        status.code().unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_path() {
        assert_eq!(
            resolve(OsStr::new("./tool"), &[]).unwrap(),
            PathBuf::from("./tool")
        );
    }
    #[test]
    fn missing_path() {
        assert_eq!(
            resolve(
                OsStr::new("agentcmd-nonexistent-program"),
                &[("PATH".into(), "".into())]
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
    }
    #[test]
    fn missing_status() {
        assert_eq!(error_code(&io::Error::from(io::ErrorKind::NotFound)), 127);
    }
    #[test]
    fn permission_status() {
        assert_eq!(
            error_code(&io::Error::from(io::ErrorKind::PermissionDenied)),
            126
        );
    }
    #[test]
    fn environment_names() {
        assert_eq!(
            env_name_eq(OsStr::new("Path"), OsStr::new("PATH")),
            cfg!(windows)
        );
    }
    #[cfg(windows)]
    #[test]
    fn native_program_separators() {
        let command = command(OsStr::new("C:/Windows/System32/cmd.exe"), &[]).unwrap();
        assert_eq!(
            command.get_program(),
            OsStr::new(r"C:\Windows\System32\cmd.exe")
        );
    }
}
