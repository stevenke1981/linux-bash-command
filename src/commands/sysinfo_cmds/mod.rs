//! System information commands: `nproc`, `hostname`, `arch`, `whoami`, `uname`, `uptime`
//! and `free`. Values come from `sysinfo`, `libc` on Unix and `windows-sys` on Windows.

mod free;
mod uname;
mod uptime;

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, unsupported, EXIT_FAIL, EXIT_OK};

pub fn nproc(args: &[String]) -> i32 {
    const NAME: &str = "nproc";
    const OPTIONS: &[OptSpec] = &[
        OptSpec::flag("all", None, Some("all")),
        OptSpec::req("ignore", None, Some("ignore")),
        OptSpec::flag("help", None, Some("help")),
        OptSpec::flag("version", None, Some("version")),
    ];
    let items = match Parser::new(NAME, OPTIONS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(NAME, &error, EXIT_FAIL),
    };
    let mut all = false;
    let mut ignore = 0usize;
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{NPROC_HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(NAME),
            Item::Opt { id: "all", .. } => all = true,
            Item::Opt {
                id: "ignore",
                value,
            } => {
                let text = value.unwrap_or_default();
                match text.parse::<usize>() {
                    Ok(count) => ignore = count,
                    Err(_) => {
                        eprintln!("{NAME}: invalid number: '{text}'");
                        return EXIT_FAIL;
                    }
                }
            }
            Item::Opt { .. } => {}
            Item::Operand(extra) => {
                let error = UsageError {
                    message: format!("extra operand '{extra}'"),
                };
                return report_usage(NAME, &error, EXIT_FAIL);
            }
        }
    }
    let count = processor_count(all, &std::env::vars_os().collect::<Vec<_>>());
    println!("{}", count.saturating_sub(ignore).max(1));
    EXIT_OK
}

const NPROC_HELP: &str = "\
Usage: nproc [OPTION]...
Print the number of processing units available to the current process,
which may be less than the number of online processors

      --all      print the number of installed processors
      --ignore=N  if possible, exclude N processing units
      --help        display this help and exit
      --version     output version information and exit
";

/// Processing units for `nproc`. `OMP_NUM_THREADS` (first comma-separated value) wins
/// unless `--all`; `OMP_THREAD_LIMIT` caps the count unless `--all`.
fn processor_count(all: bool, env: &[(std::ffi::OsString, std::ffi::OsString)]) -> usize {
    let get = |key: &str| -> Option<usize> {
        let (_, value) = env.iter().find(|(k, _)| k == key)?;
        let text = value.to_str()?;
        let first = text.split(',').next()?.trim();
        first.parse::<usize>().ok().filter(|&n| n > 0)
    };
    let available = if all {
        installed_count()
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    };
    if all {
        return available;
    }
    if let Some(threads) = get("OMP_NUM_THREADS") {
        return threads;
    }
    match get("OMP_THREAD_LIMIT") {
        Some(limit) => available.min(limit),
        None => available,
    }
}

#[cfg(unix)]
fn installed_count() -> usize {
    // SAFETY: sysconf has no preconditions.
    let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_CONF) };
    usize::try_from(n).unwrap_or(1).max(1)
}

#[cfg(not(unix))]
fn installed_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

pub fn hostname(args: &[String]) -> i32 {
    const NAME: &str = "hostname";
    const OPTIONS: &[OptSpec] = &[
        OptSpec::flag("short", Some('s'), Some("short")),
        OptSpec::flag("fqdn", Some('f'), Some("fqdn")),
        OptSpec::flag("long", None, Some("long")),
        OptSpec::flag("alias", Some('a'), Some("alias")),
        OptSpec::flag("all-fqdns", Some('A'), Some("all-fqdns")),
        OptSpec::flag("domain", Some('d'), Some("domain")),
        OptSpec::flag("ip", Some('i'), Some("ip-address")),
        OptSpec::flag("all-ip", Some('I'), Some("all-ip-addresses")),
        OptSpec::flag("yp", Some('y'), Some("yp")),
        OptSpec::flag("help", None, Some("help")),
        OptSpec::flag("version", Some('V'), Some("version")),
    ];
    let items = match Parser::new(NAME, OPTIONS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(NAME, &error, EXIT_FAIL),
    };
    let mut short = false;
    let mut operands = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HOSTNAME_HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(NAME),
            Item::Opt { id: "short", .. } => short = true,
            Item::Opt { id: "fqdn", .. } | Item::Opt { id: "long", .. } => {}
            Item::Opt { id, .. } => {
                return unsupported(NAME, &format!("-{}", id_to_flag(id)));
            }
            Item::Operand(operand) => operands.push(operand),
        }
    }
    if !operands.is_empty() {
        eprintln!("{NAME}: setting the host name is not supported by agentcmd");
        return EXIT_FAIL;
    }
    let full = sysinfo::System::host_name().unwrap_or_else(|| "localhost".to_owned());
    if short {
        println!("{}", full.split('.').next().unwrap_or(&full));
    } else {
        println!("{full}");
    }
    EXIT_OK
}

/// The name a GNU-style message uses for an option id that is recognised but not
/// implemented.
fn id_to_flag(id: &str) -> &str {
    match id {
        "alias" => "a",
        "all-fqdns" => "A",
        "domain" => "d",
        "ip" => "i",
        "all-ip" => "I",
        "yp" => "y",
        _ => id,
    }
}

const HOSTNAME_HELP: &str = "\
Usage: hostname [-s|-f] [NAME]
Display the host name.

  -s, --short    short host name
  -f, --fqdn, --long
                 long host name (FQDN); best effort, same as the host name when
                 no domain is known

      --help     display this help and exit
  -V, --version  output version information and exit
";

pub fn arch(args: &[String]) -> i32 {
    const NAME: &str = "arch";
    let options = [
        OptSpec::flag("help", None, Some("help")),
        OptSpec::flag("version", None, Some("version")),
    ];
    let items = match Parser::new(NAME, &options).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(NAME, &error, EXIT_FAIL),
    };
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("Usage: arch [OPTION]...\nPrint machine architecture.\n\n      --help        display this help and exit\n      --version     output version information and exit\n");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(NAME),
            Item::Opt { .. } => {}
            Item::Operand(extra) => {
                let error = UsageError {
                    message: format!("extra operand '{extra}'"),
                };
                return report_usage(NAME, &error, EXIT_FAIL);
            }
        }
    }
    println!("{}", machine());
    EXIT_OK
}

pub fn whoami(args: &[String]) -> i32 {
    const NAME: &str = "whoami";
    let options = [
        OptSpec::flag("help", None, Some("help")),
        OptSpec::flag("version", None, Some("version")),
    ];
    let items = match Parser::new(NAME, &options).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(NAME, &error, EXIT_FAIL),
    };
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("Usage: whoami [OPTION]...\nPrint the user name associated with the current effective user ID.\nSame as id -un.\n\n      --help        display this help and exit\n      --version     output version information and exit\n");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(NAME),
            Item::Opt { .. } => {}
            Item::Operand(extra) => {
                let error = UsageError {
                    message: format!("extra operand '{extra}'"),
                };
                return report_usage(NAME, &error, EXIT_FAIL);
            }
        }
    }
    match current_user_name() {
        Some(name) => {
            println!("{name}");
            EXIT_OK
        }
        None => {
            eprintln!("{NAME}: cannot find name for user ID");
            EXIT_FAIL
        }
    }
}

/// The machine hardware name as `uname -m` reports it.
pub(crate) fn machine() -> String {
    #[cfg(unix)]
    {
        if let Some(fields) = uname::utsname() {
            return fields[4].clone();
        }
    }
    match std::env::consts::ARCH {
        "x86" => "i686".to_owned(),
        "arm" => "arm".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(unix)]
fn current_user_name() -> Option<String> {
    // SAFETY: geteuid has no preconditions; getpwuid returns a pointer into static
    // storage that is read before any other libc call that could replace it.
    unsafe {
        let entry = libc::getpwuid(libc::geteuid());
        if entry.is_null() {
            return None;
        }
        let name = std::ffi::CStr::from_ptr((*entry).pw_name);
        Some(name.to_string_lossy().into_owned())
    }
}

#[cfg(windows)]
fn current_user_name() -> Option<String> {
    use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;
    let mut buffer = [0u16; 257];
    let mut len = buffer.len() as u32;
    // SAFETY: the buffer and length describe valid, writable storage.
    let ok = unsafe { GetUserNameW(buffer.as_mut_ptr(), &mut len) };
    if ok == 0 {
        return None;
    }
    let end = (len as usize).saturating_sub(1).min(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

pub fn uname(args: &[String]) -> i32 {
    uname::run(args)
}

pub fn uptime(args: &[String]) -> i32 {
    uptime::run(args)
}

pub fn free(args: &[String]) -> i32 {
    free::run(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omp_variables_choose_the_count() {
        let env = |pairs: &[(&str, &str)]| -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
            pairs.iter().map(|(k, v)| (k.into(), v.into())).collect()
        };
        assert_eq!(
            processor_count(false, &env(&[("OMP_NUM_THREADS", "3,4")])),
            3
        );
        assert!(processor_count(false, &env(&[("OMP_NUM_THREADS", "0")])) >= 1);
        let limited = processor_count(false, &env(&[("OMP_THREAD_LIMIT", "1")]));
        assert_eq!(limited, 1);
        assert!(processor_count(true, &env(&[("OMP_NUM_THREADS", "2")])) >= 1);
    }

    #[test]
    fn machine_is_never_empty() {
        assert!(!machine().is_empty());
    }

    #[test]
    fn unsupported_hostname_options_map_to_their_flags() {
        assert_eq!(id_to_flag("alias"), "a");
        assert_eq!(id_to_flag("all-ip"), "I");
        assert_eq!(id_to_flag("other"), "other");
    }
}
