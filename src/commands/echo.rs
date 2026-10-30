use crate::common::{
    args::help_version,
    escape::{decode, Mode},
    io::output,
};

const HELP: &str = "Usage: echo [SHORT-OPTION]... [STRING]...\nEcho STRINGs to standard output.\n\n  -n         do not output the trailing newline\n  -e         enable backslash escapes\n  -E         disable backslash escapes (default)\n  --help     display this help (only as sole argument)\n  --version  output version information (only as sole argument)\n";

fn render(args: &[String]) -> Vec<u8> {
    let (mut newline, mut escapes, mut index) = (true, false, 0);
    while let Some(arg) = args.get(index) {
        let flags = arg.strip_prefix('-').unwrap_or("");
        if flags.is_empty() || !flags.bytes().all(|b| b"neE".contains(&b)) {
            break;
        }
        for flag in flags.bytes() {
            match flag {
                b'n' => newline = false,
                b'e' => escapes = true,
                b'E' => escapes = false,
                _ => unreachable!(),
            }
        }
        index += 1;
    }
    let text = args[index..].join(" ");
    let (mut bytes, stopped) = if escapes {
        decode(text.as_bytes(), Mode::Echo).expect("echo escapes cannot fail")
    } else {
        (text.into_bytes(), false)
    };
    if newline && !stopped {
        bytes.push(b'\n');
    }
    bytes
}

pub fn run(args: &[String]) -> i32 {
    if let Some(code) = help_version("echo", args, HELP, true) {
        return code;
    }
    output("echo", |out| out.write_all(&render(args)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(a: &[&str]) -> Vec<u8> {
        render(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn empty() {
        assert_eq!(r(&[]), b"\n");
    }
    #[test]
    fn flags() {
        assert_eq!(r(&["-ne", "a\\tb"]), b"a\tb");
    }
    #[test]
    fn invalid_flag() {
        assert_eq!(r(&["-nx", "x"]), b"-nx x\n");
    }
    #[test]
    fn stop() {
        assert_eq!(r(&["-e", "a\\cb"]), b"a");
    }
    #[test]
    fn disable() {
        assert_eq!(r(&["-eE", "\\n"]), b"\\n\n");
    }
    #[test]
    fn no_permute() {
        assert_eq!(r(&["x", "-n"]), b"x -n\n");
    }
}
