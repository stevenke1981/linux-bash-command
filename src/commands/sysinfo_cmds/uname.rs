//! `uname`: print kernel and machine information.

use crate::common::args::{report_usage, Item, OptSpec, Parser, UsageError};
use crate::common::error::{print_version, EXIT_FAIL, EXIT_OK};

const NAME: &str = "uname";

const HELP: &str = "\
Usage: uname [OPTION]...
Print certain system information.  With no OPTION, same as -s.

  -a, --all                print all information, in the following order,
                             except omit -p and -i if unknown:
  -s, --kernel-name        print the kernel name
  -n, --nodename           print the network node hostname
  -r, --kernel-release     print the kernel release
  -v, --kernel-version     print the kernel version
  -m, --machine            print the machine hardware name
  -p, --processor          print the processor type (non-portable)
  -i, --hardware-platform  print the hardware platform (non-portable)
  -o, --operating-system   print the operating system
      --help        display this help and exit
      --version     output version information and exit
";

const OPTIONS: &[OptSpec] = &[
    OptSpec::flag("all", Some('a'), Some("all")),
    OptSpec::flag("kernel", Some('s'), Some("kernel-name")),
    OptSpec::flag("nodename", Some('n'), Some("nodename")),
    OptSpec::flag("release", Some('r'), Some("kernel-release")),
    OptSpec::flag("version", Some('v'), Some("kernel-version")),
    OptSpec::flag("machine", Some('m'), Some("machine")),
    OptSpec::flag("processor", Some('p'), Some("processor")),
    OptSpec::flag("hardware", Some('i'), Some("hardware-platform")),
    OptSpec::flag("os", Some('o'), Some("operating-system")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("show-version", None, Some("version")),
];

/// The fields in GNU's output order.
const FIELD_ORDER: &[&str] = &[
    "kernel",
    "nodename",
    "release",
    "version",
    "machine",
    "processor",
    "hardware",
    "os",
];

/// Entry point for `uname`.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(NAME, OPTIONS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(NAME, &error, EXIT_FAIL),
    };
    let mut all = false;
    let mut selected: Vec<&str> = Vec::new();
    for item in items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt {
                id: "show-version", ..
            } => return print_version(NAME),
            Item::Opt { id: "all", .. } => all = true,
            Item::Opt { id, .. } => selected.push(id),
            Item::Operand(extra) => {
                let error = UsageError {
                    message: format!("extra operand '{extra}'"),
                };
                return report_usage(NAME, &error, EXIT_FAIL);
            }
        }
    }
    if selected.is_empty() && !all {
        selected.push("kernel");
    }
    let Some(values) = values() else {
        eprintln!("{NAME}: cannot get system name");
        return EXIT_FAIL;
    };
    let mut parts = Vec::new();
    for field in FIELD_ORDER {
        let named = selected.contains(field);
        if !(all || named) {
            continue;
        }
        let value = values.get(field);
        // `-a` leaves out processor and hardware platform when they are unknown.
        let optional = matches!(*field, "processor" | "hardware");
        if all && !named && optional && value == "unknown" {
            continue;
        }
        parts.push(value);
    }
    println!("{}", parts.join(" "));
    EXIT_OK
}

/// The strings for each field.
struct Values {
    kernel: String,
    nodename: String,
    release: String,
    version: String,
    machine: String,
    processor: String,
    hardware: String,
    os: String,
}

impl Values {
    fn get(&self, field: &str) -> String {
        match field {
            "kernel" => self.kernel.clone(),
            "nodename" => self.nodename.clone(),
            "release" => self.release.clone(),
            "version" => self.version.clone(),
            "machine" => self.machine.clone(),
            "processor" => self.processor.clone(),
            "hardware" => self.hardware.clone(),
            _ => self.os.clone(),
        }
    }
}

#[cfg(unix)]
fn values() -> Option<Values> {
    let fields = utsname()?;
    let machine = fields[4].clone();
    // Linux reports the machine name for both; other Unix systems do not expose them.
    let (processor, hardware) = if cfg!(target_os = "linux") {
        (machine.clone(), machine.clone())
    } else {
        ("unknown".to_owned(), "unknown".to_owned())
    };
    let os = if cfg!(target_os = "linux") {
        "GNU/Linux".to_owned()
    } else {
        fields[0].clone()
    };
    Some(Values {
        kernel: fields[0].clone(),
        nodename: fields[1].clone(),
        release: fields[2].clone(),
        version: fields[3].clone(),
        machine,
        processor,
        hardware,
        os,
    })
}

#[cfg(windows)]
fn values() -> Option<Values> {
    let host = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_owned());
    let build = sysinfo::System::os_version()
        .and_then(|text| {
            let start = text.find('(')? + 1;
            let end = text[start..].find(')')? + start;
            Some(text[start..end].to_owned())
        })
        .unwrap_or_else(|| "0".to_owned());
    Some(Values {
        kernel: "Windows_NT".to_owned(),
        nodename: host,
        release: format!("10.0.{build}"),
        version: sysinfo::System::long_os_version().unwrap_or_else(|| "unknown".to_owned()),
        machine: super::machine(),
        processor: "unknown".to_owned(),
        hardware: "unknown".to_owned(),
        os: "Windows".to_owned(),
    })
}

/// The five `utsname` strings: sysname, nodename, release, version, machine.
#[cfg(unix)]
pub(super) fn utsname() -> Option<[String; 5]> {
    // SAFETY: utsname is plain data; a zeroed value is a valid starting point and
    // uname fills it in on success.
    let mut buf: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: buf is valid for writes of its full size.
    if unsafe { libc::uname(&mut buf) } != 0 {
        return None;
    }
    let field = |chars: &[libc::c_char]| -> String {
        let bytes: Vec<u8> = chars
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    Some([
        field(&buf.sysname),
        field(&buf.nodename),
        field(&buf.release),
        field(&buf.version),
        field(&buf.machine),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_selects_every_field_in_order() {
        assert_eq!(FIELD_ORDER.len(), 8);
        assert_eq!(FIELD_ORDER[0], "kernel");
        assert_eq!(FIELD_ORDER[7], "os");
    }

    #[test]
    fn values_are_populated() {
        let values = values().expect("system name");
        assert!(!values.kernel.is_empty());
        assert!(!values.machine.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn utsname_reports_the_kernel_name() {
        let fields = utsname().expect("uname");
        assert!(!fields[0].is_empty());
    }
}
