use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};

const HELP: &str = "Usage: uname [OPTION]...\nPrint certain system information.\n  -a, --all                print all information\n  -s, --kernel-name        print the kernel name\n  -n, --nodename           print the network node hostname\n  -r, --kernel-release     print the kernel release\n  -v, --kernel-version     print the kernel version\n  -m, --machine            print the machine hardware name\n  -p, --processor          print the processor type or unknown\n  -i, --hardware-platform  print the hardware platform or unknown\n  -o, --operating-system   print the operating system\n      --help               display this help\n      --version            output version information\n";

#[derive(Clone, Copy, Default, Debug)]
struct Selection {
    all: bool,
    fields: [bool; 8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UnameFields {
    sysname: String,
    nodename: String,
    release: String,
    version: String,
    machine: String,
    processor: String,
    hardware: String,
    operating_system: String,
}

impl UnameFields {
    fn values(&self) -> [&str; 8] {
        [
            &self.sysname,
            &self.nodename,
            &self.release,
            &self.version,
            &self.machine,
            &self.processor,
            &self.hardware,
            &self.operating_system,
        ]
    }
}

fn parse(arguments: &[String]) -> Result<Selection, i32> {
    let specs = [
        OptSpec::flag("all", 'a'),
        OptSpec::flag("kernel-name", 's'),
        OptSpec::flag("nodename", 'n'),
        OptSpec::flag("kernel-release", 'r'),
        OptSpec::flag("kernel-version", 'v'),
        OptSpec::flag("machine", 'm'),
        OptSpec::flag("processor", 'p'),
        OptSpec::flag("hardware-platform", 'i'),
        OptSpec::flag("operating-system", 'o'),
    ];
    let items = args::parse_command("uname", arguments, HELP, &specs, true)?;
    let mut selection = Selection::default();
    for item in items {
        let Item::Opt(id, _) = item else {
            return Err(error::usage("uname", "extra operand", 1));
        };
        match id {
            "all" => selection.all = true,
            "kernel-name" => selection.fields[0] = true,
            "nodename" => selection.fields[1] = true,
            "kernel-release" => selection.fields[2] = true,
            "kernel-version" => selection.fields[3] = true,
            "machine" => selection.fields[4] = true,
            "processor" => selection.fields[5] = true,
            "hardware-platform" => selection.fields[6] = true,
            "operating-system" => selection.fields[7] = true,
            _ => unreachable!(),
        }
    }
    if !selection.all && !selection.fields.iter().any(|set| *set) {
        selection.fields[0] = true;
    }
    Ok(selection)
}

fn output(fields: &UnameFields, selection: Selection) -> String {
    let values = fields.values();
    let selected = if selection.all {
        [true; 8]
    } else {
        selection.fields
    };
    values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            if selected[index] && !(selection.all && matches!(index, 5 | 6) && *value == "unknown")
            {
                Some(*value)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(unix)]
fn native_fields() -> Result<UnameFields, String> {
    let mut value = std::mem::MaybeUninit::<libc::utsname>::uninit();
    if unsafe { libc::uname(value.as_mut_ptr()) } != 0 {
        return Err(error::io_text(&std::io::Error::last_os_error()).to_string());
    }
    let value = unsafe { value.assume_init() };
    let field = |bytes: &[libc::c_char]| unsafe {
        std::ffi::CStr::from_ptr(bytes.as_ptr())
            .to_string_lossy()
            .into_owned()
    };
    let machine = field(&value.machine);
    let system = field(&value.sysname);
    let operating_system = if cfg!(target_os = "linux") {
        "GNU/Linux".into()
    } else if cfg!(target_os = "macos") {
        "Darwin".into()
    } else {
        "unknown".into()
    };
    Ok(UnameFields {
        sysname: system,
        nodename: field(&value.nodename),
        release: field(&value.release),
        version: field(&value.version),
        machine: machine.clone(),
        processor: "unknown".into(),
        hardware: "unknown".into(),
        operating_system,
    })
}

#[cfg(windows)]
fn native_fields() -> Result<UnameFields, String> {
    let host = sysinfo::System::host_name().unwrap_or_else(|| "unknown".into());
    let os_version = sysinfo::System::os_version();
    let long_version = sysinfo::System::long_os_version();
    let build = windows_build()
        .or_else(|| {
            os_version
                .as_deref()
                .and_then(parse_windows_build)
                .or_else(|| long_version.as_deref().and_then(parse_windows_build))
        })
        .ok_or_else(|| "unable to determine native Windows build number".to_owned())?;
    let release = format!("10.0.{build}");
    let version = long_version.unwrap_or_else(|| release.clone());
    let machine = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "x86" => "i686",
        _ => "unknown",
    }
    .to_owned();
    Ok(UnameFields {
        sysname: "Windows_NT".into(),
        nodename: host,
        release,
        version,
        machine,
        processor: "unknown".into(),
        hardware: "unknown".into(),
        operating_system: "Windows".into(),
    })
}

#[cfg(windows)]
fn windows_build() -> Option<u32> {
    #[repr(C)]
    struct OsVersionInfoW {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfoW) -> i32;
    }
    let mut info = OsVersionInfoW {
        size: std::mem::size_of::<OsVersionInfoW>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform: 0,
        service_pack: [0; 128],
    };
    let status = unsafe { RtlGetVersion(&mut info) };
    (status >= 0 && info.major == 10 && info.minor == 0 && info.build > 0).then_some(info.build)
}

#[cfg(any(windows, test))]
fn parse_windows_build(value: &str) -> Option<u32> {
    let tail = value.split("10.0.").nth(1)?;
    let digits = tail
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    let build = digits.parse::<u32>().ok()?;
    (build > 0).then_some(build)
}

pub fn run(arguments: &[String]) -> i32 {
    let selection = match parse(arguments) {
        Ok(value) => value,
        Err(code) => return code,
    };
    let fields = match native_fields() {
        Ok(value) => value,
        Err(message) => {
            eprintln!("uname: {message}");
            return 1;
        }
    };
    let line = output(&fields, selection);
    io::output("uname", |out| writeln!(out, "{line}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fields() -> UnameFields {
        UnameFields {
            sysname: "Linux".into(),
            nodename: "node".into(),
            release: "6.8".into(),
            version: "#1".into(),
            machine: "x86_64".into(),
            processor: "unknown".into(),
            hardware: "unknown".into(),
            operating_system: "GNU/Linux".into(),
        }
    }
    #[test]
    fn default_selects_kernel_name() {
        assert_eq!(output(&fields(), parse(&[]).unwrap()), "Linux");
    }
    #[test]
    fn all_omits_unknown_platform_values() {
        assert_eq!(
            output(
                &fields(),
                Selection {
                    all: true,
                    ..Selection::default()
                }
            ),
            "Linux node 6.8 #1 x86_64 GNU/Linux"
        );
    }
    #[test]
    fn selection_preserves_uname_order() {
        let s = parse(&["-omr".into()]).unwrap();
        assert_eq!(output(&fields(), s), "6.8 x86_64 GNU/Linux");
    }
    #[test]
    fn long_flags_select_fields() {
        let s = parse(&["--nodename".into(), "--machine".into()]).unwrap();
        assert_eq!(output(&fields(), s), "node x86_64");
    }
    #[test]
    fn all_flag_overrides_individual_selection() {
        let s = parse(&["-s".into(), "-a".into()]).unwrap();
        assert_eq!(output(&fields(), s), "Linux node 6.8 #1 x86_64 GNU/Linux");
    }
    #[test]
    fn processor_flag_can_print_unknown() {
        let s = parse(&["-p".into()]).unwrap();
        assert_eq!(output(&fields(), s), "unknown");
    }
    #[test]
    fn hardware_flag_can_print_unknown() {
        let s = parse(&["-i".into()]).unwrap();
        assert_eq!(output(&fields(), s), "unknown");
    }
    #[test]
    fn operating_system_long_flag() {
        assert_eq!(
            output(&fields(), parse(&["--operating-system".into()]).unwrap()),
            "GNU/Linux"
        );
    }
    #[test]
    fn kernel_release_short_flag() {
        assert_eq!(output(&fields(), parse(&["-r".into()]).unwrap()), "6.8");
    }
    #[test]
    fn kernel_version_short_flag() {
        assert_eq!(output(&fields(), parse(&["-v".into()]).unwrap()), "#1");
    }
    #[test]
    fn nodename_short_flag() {
        assert_eq!(output(&fields(), parse(&["-n".into()]).unwrap()), "node");
    }
    #[test]
    fn machine_short_flag() {
        assert_eq!(output(&fields(), parse(&["-m".into()]).unwrap()), "x86_64");
    }
    #[test]
    fn kernel_name_short_flag() {
        assert_eq!(output(&fields(), parse(&["-s".into()]).unwrap()), "Linux");
    }
    #[test]
    fn bundled_flags_parse() {
        assert_eq!(
            output(&fields(), parse(&["-sn".into()]).unwrap()),
            "Linux node"
        );
    }
    #[test]
    fn options_permute_after_operand_then_report_extra() {
        assert_eq!(parse(&["operand".into(), "-s".into()]).unwrap_err(), 1);
    }
    #[test]
    fn unknown_option_fails() {
        assert_eq!(parse(&["--bad".into()]).unwrap_err(), 1);
    }
    #[test]
    fn help_is_supported() {
        assert_eq!(
            args::help_version("uname", &["--help".into()], HELP, true),
            Some(0)
        );
    }
    #[test]
    fn version_is_supported() {
        assert_eq!(
            args::help_version("uname", &["--version".into()], HELP, true),
            Some(0)
        );
    }
    #[test]
    fn all_fields_are_ordered() {
        assert_eq!(
            fields().values(),
            [
                "Linux",
                "node",
                "6.8",
                "#1",
                "x86_64",
                "unknown",
                "unknown",
                "GNU/Linux"
            ]
        );
    }
    #[test]
    fn unknown_fields_are_not_removed_from_explicit_selection() {
        let s = parse(&["-ip".into()]).unwrap();
        assert_eq!(output(&fields(), s), "unknown unknown");
    }
    #[test]
    fn extracts_native_build_from_sysinfo_version() {
        assert_eq!(parse_windows_build("10.0.22631"), Some(22631));
        assert_eq!(parse_windows_build("Windows 11 (10.0.26100)"), Some(26100));
    }
    #[test]
    fn malformed_version_does_not_invent_build() {
        assert_eq!(parse_windows_build("Windows 11"), None);
        assert_eq!(parse_windows_build("10.0.0"), None);
    }
    #[cfg(windows)]
    #[test]
    fn windows_release_is_native_and_formatted() {
        let release = native_fields().unwrap().release;
        assert!(release
            .strip_prefix("10.0.")
            .is_some_and(|n| n.parse::<u32>().is_ok_and(|b| b > 0)));
    }
}
