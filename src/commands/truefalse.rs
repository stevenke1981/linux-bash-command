use crate::common::args::help_version;

fn run(name: &str, args: &[String], status: i32) -> i32 {
    let help = format!("Usage: {name} [ignored command line arguments]\nExit with status {status}.\n\n  --help     display this help\n  --version  output version information\n");
    if let Some(code) = help_version(name, args, &help, true) {
        if code != 0 {
            return code;
        }
    }
    status
}

pub fn run_true(args: &[String]) -> i32 {
    run("true", args, 0)
}
pub fn run_false(args: &[String]) -> i32 {
    run("false", args, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn true_empty() {
        assert_eq!(run_true(&[]), 0);
    }
    #[test]
    fn true_unknown() {
        assert_eq!(run_true(&["--unknown".into()]), 0);
    }
    #[test]
    fn true_operand() {
        assert_eq!(run_true(&["false".into()]), 0);
    }
    #[test]
    fn true_dash() {
        assert_eq!(run_true(&["--".into()]), 0);
    }
    #[test]
    fn true_help_operand() {
        assert_eq!(run_true(&["--help".into(), "x".into()]), 0);
    }
    #[test]
    fn false_empty() {
        assert_eq!(run_false(&[]), 1);
    }
    #[test]
    fn false_unknown() {
        assert_eq!(run_false(&["--unknown".into()]), 1);
    }
    #[test]
    fn false_operand() {
        assert_eq!(run_false(&["true".into()]), 1);
    }
    #[test]
    fn false_dash() {
        assert_eq!(run_false(&["--".into()]), 1);
    }
    #[test]
    fn false_help_operand() {
        assert_eq!(run_false(&["--help".into(), "x".into()]), 1);
    }
}
