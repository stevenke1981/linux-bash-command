use crate::common::args;
use std::fs;

const HELP: &str = "Usage: [ EXPRESSION ]\nEvaluate EXPRESSION.\nUnary: -b -c -d -e -f -g -G -h -L -k -N -O -p -r -s -S -t -u -w -x -z -n\nBinary: = == != < > -eq -ne -lt -le -gt -ge -nt -ot -ef\nLogical: ! -a -o ( )\n  --help     display this help (only as the sole argument)\n  --version  output version information\nExit status: 0 if true, 1 if false, 2 on invalid expression.\n";

pub fn run(arguments: &[String]) -> i32 {
    evaluate("test", arguments)
}

pub fn run_bracket(arguments: &[String]) -> i32 {
    if let Some(code) = args::help_version("[", arguments, HELP, true) {
        return code;
    }
    if arguments.last().map(String::as_str) != Some("]") {
        eprintln!("[: missing ']'");
        return 2;
    }
    evaluate("[", &arguments[..arguments.len() - 1])
}

fn evaluate(command: &str, arguments: &[String]) -> i32 {
    match expression(arguments) {
        Ok(value) => i32::from(!value),
        Err(error) => {
            eprintln!("{command}: {error}");
            2
        }
    }
}

fn integer(text: &str) -> Result<i128, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("invalid integer '{text}'"))
}

fn integer_order(left: &str, right: &str) -> Result<std::cmp::Ordering, String> {
    fn parts(text: &str) -> Result<(bool, &str), String> {
        let trimmed = text.trim();
        let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("invalid integer '{text}'"));
        }
        let digits = digits.trim_start_matches('0');
        Ok((trimmed.starts_with('-') && !digits.is_empty(), digits))
    }
    let (an, a) = parts(left)?;
    let (bn, b) = parts(right)?;
    if an != bn {
        return Ok(if an {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        });
    }
    let order = a.len().cmp(&b.len()).then_with(|| a.cmp(b));
    Ok(if an { order.reverse() } else { order })
}

fn unary(op: &str) -> bool {
    matches!(
        op,
        "-b" | "-c"
            | "-d"
            | "-e"
            | "-f"
            | "-g"
            | "-G"
            | "-h"
            | "-L"
            | "-k"
            | "-N"
            | "-O"
            | "-p"
            | "-r"
            | "-s"
            | "-S"
            | "-t"
            | "-u"
            | "-w"
            | "-x"
            | "-z"
            | "-n"
    )
}
fn binary(op: &str) -> bool {
    matches!(
        op,
        "=" | "=="
            | "!="
            | "<"
            | ">"
            | "-eq"
            | "-ne"
            | "-lt"
            | "-le"
            | "-gt"
            | "-ge"
            | "-nt"
            | "-ot"
            | "-ef"
    )
}

fn unary_value(op: &str, path: &str) -> Result<bool, String> {
    if op == "-z" {
        return Ok(path.is_empty());
    }
    if op == "-n" {
        return Ok(!path.is_empty());
    }
    if op == "-t" {
        let fd = integer(path)?;
        #[cfg(unix)]
        {
            unsafe extern "C" {
                fn isatty(fd: i32) -> i32;
            }
            return Ok(i32::try_from(fd).is_ok_and(|fd| unsafe { isatty(fd) != 0 }));
        }
        #[cfg(not(unix))]
        {
            use std::io::IsTerminal;
            return Ok(match fd {
                0 => std::io::stdin().is_terminal(),
                1 => std::io::stdout().is_terminal(),
                2 => std::io::stderr().is_terminal(),
                _ => false,
            });
        }
    }
    let meta = if matches!(op, "-h" | "-L") {
        fs::symlink_metadata(path)
    } else {
        fs::metadata(path)
    };
    let Ok(meta) = meta else {
        return Ok(false);
    };
    Ok(match op {
        "-e" => true,
        "-f" => meta.is_file(),
        "-d" => meta.is_dir(),
        "-s" => meta.len() > 0,
        "-h" | "-L" => meta.file_type().is_symlink(),
        "-N" => meta
            .modified()
            .ok()
            .zip(meta.accessed().ok())
            .is_some_and(|(m, a)| m > a),
        "-r" | "-w" | "-x" => {
            #[cfg(unix)]
            {
                use std::ffi::CString;
                unsafe extern "C" {
                    fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
                }
                CString::new(path).is_ok_and(|path| unsafe {
                    access(
                        path.as_ptr(),
                        match op {
                            "-r" => 4,
                            "-w" => 2,
                            _ => 1,
                        },
                    ) == 0
                })
            }
            #[cfg(not(unix))]
            {
                match op {
                    "-r" => true,
                    "-w" => !meta.permissions().readonly(),
                    _ => {
                        meta.is_dir()
                            || std::path::Path::new(path).extension().is_some_and(|e| {
                                matches!(
                                    e.to_string_lossy().to_ascii_lowercase().as_str(),
                                    "exe" | "com" | "bat" | "cmd" | "ps1"
                                )
                            })
                    }
                }
            }
        }
        _ => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::{FileTypeExt, MetadataExt};
                unsafe extern "C" {
                    fn geteuid() -> u32;
                    fn getegid() -> u32;
                }
                match op {
                    "-b" => meta.file_type().is_block_device(),
                    "-c" => meta.file_type().is_char_device(),
                    "-p" => meta.file_type().is_fifo(),
                    "-S" => meta.file_type().is_socket(),
                    "-u" => meta.mode() & 0o4000 != 0,
                    "-g" => meta.mode() & 0o2000 != 0,
                    "-k" => meta.mode() & 0o1000 != 0,
                    "-O" => meta.uid() == unsafe { geteuid() },
                    "-G" => meta.gid() == unsafe { getegid() },
                    _ => false,
                }
            }
            #[cfg(not(unix))]
            {
                matches!(op, "-O" | "-G")
            }
        }
    })
}

fn binary_value(left: &str, op: &str, right: &str) -> Result<bool, String> {
    Ok(match op {
        "=" | "==" => left == right,
        "!=" => left != right,
        "<" => left.as_bytes() < right.as_bytes(),
        ">" => left.as_bytes() > right.as_bytes(),
        "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
            let order = integer_order(left, right)?;
            match op {
                "-eq" => order.is_eq(),
                "-ne" => !order.is_eq(),
                "-lt" => order.is_lt(),
                "-le" => order.is_le(),
                "-gt" => order.is_gt(),
                _ => order.is_ge(),
            }
        }
        "-nt" | "-ot" => {
            let a = fs::metadata(left).and_then(|m| m.modified()).ok();
            let b = fs::metadata(right).and_then(|m| m.modified()).ok();
            if op == "-nt" {
                a.is_some_and(|a| b.is_none_or(|b| a > b))
            } else {
                b.is_some_and(|b| a.is_none_or(|a| a < b))
            }
        }
        "-ef" => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                fs::metadata(left)
                    .ok()
                    .zip(fs::metadata(right).ok())
                    .is_some_and(|(a, b)| a.dev() == b.dev() && a.ino() == b.ino())
            }
            #[cfg(windows)]
            {
                file_identity(left)
                    .zip(file_identity(right))
                    .is_some_and(|(a, b)| a == b)
                    || fs::metadata(left).is_ok_and(|m| m.is_dir())
                        && fs::canonicalize(left)
                            .ok()
                            .zip(fs::canonicalize(right).ok())
                            .is_some_and(|(a, b)| a == b)
            }
            #[cfg(not(any(unix, windows)))]
            {
                fs::canonicalize(left)
                    .ok()
                    .zip(fs::canonicalize(right).ok())
                    .is_some_and(|(a, b)| a == b)
            }
        }
        _ => return Err(format!("unknown binary operator '{op}'")),
    })
}

#[cfg(windows)]
fn file_identity(path: &str) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    #[derive(Default)]
    struct Info {
        attributes: u32,
        creation: [u32; 2],
        access: [u32; 2],
        write: [u32; 2],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut Info) -> i32;
    }
    let file = fs::File::open(path).ok()?;
    let mut info = Info::default();
    // The file owns a live HANDLE, and Info exactly matches BY_HANDLE_FILE_INFORMATION.
    (unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0).then_some((
        info.volume,
        (u64::from(info.index_high) << 32) | u64::from(info.index_low),
    ))
}

fn expression(a: &[String]) -> Result<bool, String> {
    match a.len() {
        0 => return Ok(false),
        1 => return Ok(!a[0].is_empty()),
        2 if a[0] == "!" => return Ok(a[1].is_empty()),
        2 if unary(&a[0]) => return unary_value(&a[0], &a[1]),
        3 if binary(&a[1]) => return binary_value(&a[0], &a[1], &a[2]),
        3 if a[0] == "!" => return expression(&a[1..]).map(|v| !v),
        3 if a[0] == "(" && a[2] == ")" => return expression(&a[1..2]),
        4 if a[0] == "!" => return expression(&a[1..]).map(|v| !v),
        4 if a[0] == "(" && a[3] == ")" => return expression(&a[1..3]),
        _ => {}
    }
    let mut parser = Parser { args: a, at: 0 };
    let result = parser.or()?;
    if parser.at != a.len() {
        return Err(format!("extra argument '{}'", a[parser.at]));
    }
    Ok(result)
}

struct Parser<'a> {
    args: &'a [String],
    at: usize,
}
impl Parser<'_> {
    fn or(&mut self) -> Result<bool, String> {
        let mut value = self.and()?;
        while self.args.get(self.at).is_some_and(|a| a == "-o") {
            self.at += 1;
            let next = self.and()?;
            value |= next;
        }
        Ok(value)
    }
    fn and(&mut self) -> Result<bool, String> {
        let mut value = self.primary()?;
        while self.args.get(self.at).is_some_and(|a| a == "-a") {
            self.at += 1;
            let next = self.primary()?;
            value &= next;
        }
        Ok(value)
    }
    fn primary(&mut self) -> Result<bool, String> {
        let left = self.args.get(self.at).ok_or("missing argument")?.as_str();
        self.at += 1;
        if self.args.get(self.at).is_some_and(|s| binary(s)) {
            let op = &self.args[self.at];
            self.at += 1;
            let right = self
                .args
                .get(self.at)
                .ok_or_else(|| format!("missing argument after '{op}'"))?;
            self.at += 1;
            return binary_value(left, op, right);
        }
        if left == "!" {
            return self.primary().map(|v| !v);
        }
        if left == "(" {
            let value = self.or()?;
            if self.args.get(self.at).map(String::as_str) != Some(")") {
                return Err("')' expected".into());
            }
            self.at += 1;
            return Ok(value);
        }
        if unary(left) {
            let right = self
                .args
                .get(self.at)
                .ok_or_else(|| format!("missing argument after '{left}'"))?;
            self.at += 1;
            return unary_value(left, right);
        }
        Ok(!left.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn t(a: &[&str]) -> Result<bool, String> {
        expression(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn empty() {
        assert_eq!(t(&[]), Ok(false));
        assert_eq!(t(&[""]), Ok(false));
    }
    #[test]
    fn singleton_operator() {
        assert_eq!(t(&["!"]), Ok(true));
        assert_eq!(t(&["--help"]), Ok(true));
    }
    #[test]
    fn strings() {
        assert_eq!(t(&["a", "<", "b"]), Ok(true));
        assert_eq!(t(&["-z", ""]), Ok(true));
    }
    #[test]
    fn integers() {
        assert_eq!(t(&[" +2 ", "-eq", "02"]), Ok(true));
        assert!(t(&["x", "-eq", "2"]).is_err());
    }
    #[test]
    fn precedence() {
        assert_eq!(t(&["", "-a", "x", "-o", "y"]), Ok(true));
    }
    #[test]
    fn negation() {
        assert_eq!(t(&["!", "(", "", "-o", "", ")"]), Ok(true));
    }
    #[test]
    fn argument_count() {
        assert_eq!(t(&["!", "=", "!"]), Ok(true));
        assert_eq!(t(&["(", "-n", ")"]), Ok(true));
    }
    #[test]
    fn bracket_missing() {
        assert_eq!(run_bracket(&[]), 2);
    }
    #[test]
    fn bracket_empty() {
        assert_eq!(run_bracket(&["]".into()]), 1);
    }
    #[test]
    fn bracket_expression() {
        assert_eq!(
            run_bracket(&["a".into(), "=".into(), "a".into(), "]".into()]),
            0
        );
    }
    #[test]
    fn bracket_false_expression() {
        assert_eq!(run_bracket(&["".into(), "]".into()]), 1);
    }
    #[test]
    fn bracket_invalid_integer() {
        assert_eq!(
            run_bracket(&["x".into(), "-eq".into(), "1".into(), "]".into()]),
            2
        );
    }
    #[test]
    fn large_integers() {
        assert!(integer_order(
            "10000000000000000000000000000000000000000000000",
            "9999999999999999999999999999999999999999999999"
        )
        .unwrap()
        .is_gt());
    }
}
