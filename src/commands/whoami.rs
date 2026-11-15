use crate::common::{args, error, io};
use std::io::Write;

const HELP: &str = "Usage: whoami [OPTION]...\nPrint the effective user name.\n      --help     display this help and exit\n      --version  output version information and exit\n";

pub fn run(arguments: &[String]) -> i32 {
    let items = match args::parse_command("whoami", arguments, HELP, &[], true) {
        Ok(items) => items,
        Err(code) => return code,
    };
    if !items.is_empty() {
        return error::usage("whoami", "extra operand", 1);
    }
    report_lookup(username(), &mut std::io::stderr().lock(), |name| {
        io::output("whoami", |out| writeln!(out, "{name}"))
    })
}

fn report_lookup(
    result: Result<String, String>,
    errors: &mut impl Write,
    output: impl FnOnce(&str) -> i32,
) -> i32 {
    match result {
        Ok(name) => output(&name),
        Err(message) => {
            let _ = writeln!(errors, "whoami: {message}");
            1
        }
    }
}

#[cfg(unix)]
fn username() -> Result<String, String> {
    // getpwuid_r avoids the shared static storage returned by getpwuid.
    unsafe {
        let uid = libc::geteuid();
        let mut size = 1024usize;
        loop {
            let mut buffer = vec![0u8; size];
            let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
            let mut result = std::ptr::null_mut();
            let code = libc::getpwuid_r(
                uid,
                entry.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                size,
                &mut result,
            );
            if code == libc::ERANGE && size < 1024 * 1024 {
                size *= 2;
                continue;
            }
            if code != 0 {
                let failure = std::io::Error::from_raw_os_error(code);
                return Err(format!(
                    "cannot find name for user ID {uid}: {}",
                    error::io_text(&failure)
                ));
            }
            if result.is_null() || (*result).pw_name.is_null() {
                return Err(format!("cannot find name for user ID {uid}"));
            }
            return Ok(std::ffi::CStr::from_ptr((*result).pw_name)
                .to_string_lossy()
                .into_owned());
        }
    }
}

#[cfg(windows)]
fn username() -> Result<String, String> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetUserNameW(buffer: *mut u16, size: *mut u32) -> i32;
    }
    unsafe {
        let mut buffer = vec![0u16; 512];
        let mut size = buffer.len() as u32;
        if GetUserNameW(buffer.as_mut_ptr(), &mut size) == 0 {
            return Err(format!(
                "cannot determine user name: {}",
                error::io_text(&std::io::Error::last_os_error())
            ));
        }
        Ok(String::from_utf16_lossy(
            &buffer[..size.saturating_sub(1) as usize],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn help_succeeds() {
        assert_eq!(
            args::help_version("whoami", &["--help".into()], HELP, true),
            Some(0)
        );
    }
    #[test]
    fn version_succeeds() {
        assert_eq!(
            args::help_version("whoami", &["--version".into()], HELP, true),
            Some(0)
        );
    }
    #[test]
    fn username_is_nonempty() {
        assert!(!username().unwrap().is_empty());
    }
    #[test]
    fn username_has_no_newline() {
        assert!(!username().unwrap().contains(['\n', '\r']));
    }
    #[test]
    fn username_has_no_domain_separator() {
        assert!(!username().unwrap().contains('\\'));
    }
    #[test]
    fn username_is_trimmed() {
        let name = username().unwrap();
        assert_eq!(name, name.trim());
    }
    #[test]
    fn username_is_stable() {
        assert_eq!(username().unwrap(), username().unwrap());
    }
    #[test]
    fn lookup_failure_reports_error_without_output() {
        let mut errors = Vec::new();
        let mut output = Vec::new();
        let status = report_lookup(
            Err("cannot find name for user ID 12345".into()),
            &mut errors,
            |name| {
                writeln!(output, "{name}").unwrap();
                0
            },
        );
        assert_eq!(status, 1);
        assert!(output.is_empty());
        assert_eq!(errors, b"whoami: cannot find name for user ID 12345\n");
    }
    #[test]
    fn successful_lookup_produces_name_and_propagates_output_status() {
        let mut errors = Vec::new();
        let mut output = Vec::new();
        let status = report_lookup(Ok("example".into()), &mut errors, |name| {
            writeln!(output, "{name}").unwrap();
            0
        });
        assert_eq!(status, 0);
        assert_eq!(output, b"example\n");
        assert!(errors.is_empty());
        assert_eq!(report_lookup(Ok("x".into()), &mut errors, |_| 7), 7);
    }
    #[test]
    fn extra_operand_is_error() {
        assert_eq!(run(&["x".into()]), 1);
    }
}
