use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    datetime, error,
};
use filetime::{set_file_times, set_symlink_file_times, FileTime};
use std::{
    fs::{self, OpenOptions},
    path::Path,
};
const HELP: &str = "Usage: touch [OPTION]... FILE...\nUpdate access and modification times.\n  -a                    change only access time\n  -c, --no-create       do not create files\n  -d, --date=STRING     use parsed date\n  -f                    accepted for compatibility\n  -h, --no-dereference  affect symlinks instead of targets\n  -m                    change only modification time\n  -r, --reference=FILE  use times from FILE\n  -t STAMP              use [[CC]YY]MMDDhhmm[.ss]\n      --time=WORD       access, atime, use, modify or mtime\n      --help            display this help\n      --version         output version information\n";

fn stamp(input: &str, now: &jiff::Zoned) -> Option<jiff::Zoned> {
    let (digits, seconds) = input.split_once('.').unwrap_or((input, "00"));
    if !digits.bytes().all(|b| b.is_ascii_digit())
        || seconds.len() != 2
        || !seconds.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let full = match digits.len() {
        8 => format!("{:04}{digits}{seconds}", now.year()),
        10 => {
            let yy = digits[..2].parse::<u32>().ok()?;
            format!(
                "{}{}{}",
                if yy >= 69 { "19" } else { "20" },
                digits,
                seconds
            )
        }
        12 => format!("{digits}{seconds}"),
        _ => return None,
    };
    jiff::civil::DateTime::strptime("%Y%m%d%H%M%S", full)
        .ok()?
        .to_zoned(now.time_zone().clone())
        .ok()
}

#[cfg(unix)]
fn touch_stdout(atime: Option<FileTime>, mtime: Option<FileTime>) -> std::io::Result<()> {
    use std::os::fd::AsFd;
    let file = std::fs::File::from(std::io::stdout().as_fd().try_clone_to_owned()?);
    filetime::set_file_handle_times(&file, atime, mtime)
}
#[cfg(windows)]
fn touch_stdout(atime: Option<FileTime>, mtime: Option<FileTime>) -> std::io::Result<()> {
    use std::os::windows::io::AsHandle;
    let file = std::fs::File::from(std::io::stdout().as_handle().try_clone_to_owned()?);
    filetime::set_file_handle_times(&file, atime, mtime)
}
#[cfg(not(any(unix, windows)))]
fn touch_stdout(_: Option<FileTime>, _: Option<FileTime>) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

pub fn run(argv: &[String]) -> i32 {
    let short = |id, ch| OptSpec {
        id,
        short: ch,
        long: "",
        has_arg: HasArg::No,
    };
    let specs = [
        short("access", 'a'),
        OptSpec::flag("no-create", 'c'),
        OptSpec::value("date", 'd'),
        short("force", 'f'),
        OptSpec::flag("no-dereference", 'h'),
        short("modify", 'm'),
        OptSpec::value("reference", 'r'),
        OptSpec {
            id: "stamp",
            short: 't',
            long: "",
            has_arg: HasArg::Required,
        },
        OptSpec::value("time", '\0'),
    ];
    let items = match args::parse_command("touch", argv, HELP, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let (mut access, mut modify, mut no_create, mut no_deref) = (false, false, false, false);
    let (mut date, mut reference, mut stamp_value, mut files) = (None, None, None, Vec::new());
    for item in items {
        match item {
            Item::Opt("access", _) => access = true,
            Item::Opt("modify", _) => modify = true,
            Item::Opt("no-create", _) => no_create = true,
            Item::Opt("no-dereference", _) => no_deref = true,
            Item::Opt("force", _) => {}
            Item::Opt("date", v) => date = v,
            Item::Opt("reference", v) => reference = v,
            Item::Opt("stamp", v) => stamp_value = v,
            Item::Opt("time", Some(v)) => match v.as_str() {
                "access" | "atime" | "use" => access = true,
                "modify" | "mtime" => modify = true,
                _ => {
                    return error::usage("touch", format!("invalid argument '{v}' for '--time'"), 1)
                }
            },
            Item::Operand(v) => files.push(v),
            _ => unreachable!(),
        }
    }
    if files.is_empty() {
        return error::usage("touch", "missing file operand", 1);
    }
    if !access && !modify {
        access = true;
        modify = true;
    }
    if stamp_value.is_some() && (date.is_some() || reference.is_some()) {
        return error::usage("touch", "cannot specify times from more than one source", 1);
    }
    let reference_meta = if let Some(path) = reference.as_deref() {
        let meta = if no_deref {
            fs::symlink_metadata(path)
        } else {
            fs::metadata(path)
        };
        match meta {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!(
                    "touch: failed to get attributes of '{}': {}",
                    path,
                    error::io_text(&e)
                );
                return 1;
            }
        }
    } else {
        None
    };
    let explicit = if let Some(text) = date {
        let parsed = if let Some(meta) = reference_meta.as_ref() {
            meta.accessed()
                .and_then(|a| meta.modified().map(|m| (a, m)))
                .ok()
                .and_then(|(a, m)| {
                    Some((
                        datetime::from_system_time(a).ok()?,
                        datetime::from_system_time(m).ok()?,
                    ))
                })
                .and_then(|(a, m)| {
                    Some((
                        datetime::parse_at(&text, &a).ok()?,
                        datetime::parse_at(&text, &m).ok()?,
                    ))
                })
        } else {
            datetime::parse(&text).ok().map(|d| (d.clone(), d))
        };
        match parsed {
            Some((a, m)) => {
                let a: std::time::SystemTime = a.timestamp().into();
                let m: std::time::SystemTime = m.timestamp().into();
                (FileTime::from_system_time(a), FileTime::from_system_time(m))
            }
            None => {
                eprintln!("touch: invalid date format '{text}'");
                return 1;
            }
        }
    } else if let Some(text) = stamp_value {
        let now = match datetime::from_system_time(std::time::SystemTime::now()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("touch: {e}");
                return 1;
            }
        };
        match stamp(&text, &now) {
            Some(z) => {
                let t: std::time::SystemTime = z.timestamp().into();
                (FileTime::from_system_time(t), FileTime::from_system_time(t))
            }
            None => {
                eprintln!("touch: invalid date format '{text}'");
                return 1;
            }
        }
    } else if let Some(meta) = reference_meta {
        (
            FileTime::from_last_access_time(&meta),
            FileTime::from_last_modification_time(&meta),
        )
    } else {
        let now = FileTime::now();
        (now, now)
    };
    let mut failed = false;
    for name in files {
        if name == "-" {
            let (atime, mtime) = explicit;
            if let Err(e) = touch_stdout(access.then_some(atime), modify.then_some(mtime)) {
                eprintln!("touch: setting times of '-': {}", error::io_text(&e));
                failed = true;
            }
            continue;
        }
        let path = Path::new(&name);
        let metadata = if no_deref {
            fs::symlink_metadata(path)
        } else {
            fs::metadata(path)
        };
        let metadata = match metadata {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && no_create => continue,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .and_then(|f| f.metadata())
            {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("touch: cannot touch '{}': {}", name, error::io_text(&e));
                    failed = true;
                    continue;
                }
            },
            Err(e) => {
                eprintln!("touch: cannot touch '{}': {}", name, error::io_text(&e));
                failed = true;
                continue;
            }
        };
        let (atime, mtime) = explicit;
        let atime = if access {
            atime
        } else {
            FileTime::from_last_access_time(&metadata)
        };
        let mtime = if modify {
            mtime
        } else {
            FileTime::from_last_modification_time(&metadata)
        };
        let result = if no_deref {
            set_symlink_file_times(path, atime, mtime)
        } else {
            set_file_times(path, atime, mtime)
        };
        if let Err(e) = result {
            eprintln!("touch: setting times of '{}': {}", name, error::io_text(&e));
            failed = true;
        }
    }
    i32::from(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn now() -> jiff::Zoned {
        "2026-01-02T03:04:05+00:00[UTC]".parse().unwrap()
    }
    #[test]
    fn full_year() {
        assert_eq!(
            stamp("202401020304.05", &now())
                .unwrap()
                .strftime("%F %T")
                .to_string(),
            "2024-01-02 03:04:05"
        );
    }
    #[test]
    fn this_year() {
        assert_eq!(stamp("01020304", &now()).unwrap().year(), 2026);
    }
    #[test]
    fn pivot_old() {
        assert_eq!(stamp("6901020304", &now()).unwrap().year(), 1969);
    }
    #[test]
    fn pivot_new() {
        assert_eq!(stamp("6801020304", &now()).unwrap().year(), 2068);
    }
    #[test]
    fn invalid() {
        assert!(stamp("202413010000", &now()).is_none());
    }
}
