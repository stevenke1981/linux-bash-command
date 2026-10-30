//! `egrep` and `fgrep`: `grep -E` and `grep -F`.

fn with_flag(flag: &str, args: &[String]) -> Vec<String> {
    let mut full = Vec::with_capacity(args.len() + 1);
    full.push(flag.to_owned());
    full.extend_from_slice(args);
    full
}

pub fn egrep(args: &[String]) -> i32 {
    crate::grep_command::run(&with_flag("-E", args))
}

pub fn fgrep(args: &[String]) -> i32 {
    crate::grep_command::run(&with_flag("-F", args))
}
