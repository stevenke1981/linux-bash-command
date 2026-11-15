use crate::common::{args, error, io};

const HELP: &str = "Usage: whoami [OPTION]...\nPrint the effective user name.\n      --help     display this help and exit\n      --version  output version information and exit\n";

pub fn run(arguments: &[String]) -> i32 {
    if !arguments.is_empty() {
        return match args::help_version("whoami", arguments, HELP, true) {
            Some(code) => code,
            None => error::usage("whoami", "extra operand", 1),
        };
    }
    let name = username();
    io::output("whoami", |out| writeln!(out, "{name}"))
}

#[cfg(unix)]
fn username() -> String {
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
            if code != 0 || result.is_null() {
                return uid.to_string();
            }
            return std::ffi::CStr::from_ptr((*result).pw_name)
                .to_string_lossy()
                .into_owned();
        }
    }
}

#[cfg(windows)]
fn username() -> String {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetUserNameW(buffer: *mut u16, size: *mut u32) -> i32;
    }
    unsafe {
        let mut buffer = vec![0u16; 512];
        let mut size = buffer.len() as u32;
        if GetUserNameW(buffer.as_mut_ptr(), &mut size) == 0 {
            return "unknown".into();
        }
        String::from_utf16_lossy(&buffer[..size.saturating_sub(1) as usize])
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
        assert!(!username().is_empty());
    }
    #[test]
    fn username_has_no_newline() {
        assert!(!username().contains(['\n', '\r']));
    }
    #[test]
    fn username_has_no_domain_separator() {
        assert!(!username().contains('\\'));
    }
    #[test]
    fn username_is_trimmed() {
        assert_eq!(username(), username().trim());
    }
    #[test]
    fn username_is_stable() {
        assert_eq!(username(), username());
    }
    #[test]
    fn extra_operand_is_error() {
        assert_eq!(run(&["x".into()]), 1);
    }
}
