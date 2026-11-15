pub fn run(arguments: &[String]) -> i32 {
    super::pgrep::run_pkill(arguments)
}

#[cfg(test)]
mod tests {
    use super::super::pgrep::normalize_signal;
    #[test]
    fn named_and_numeric_signals() {
        for (input, expected) in [
            ("-TERM", "--signal=TERM"),
            ("-9", "--signal=9"),
            ("-0", "--signal=0"),
            ("-SIGUSR1", "--signal=SIGUSR1"),
        ] {
            assert_eq!(normalize_signal(&[input.into()], true), [expected]);
        }
    }
    #[test]
    fn required_option_values_are_not_reinterpreted() {
        for option in ["-P", "-u", "-U", "--parent", "--euid", "--uid"] {
            let args = [option.to_string(), "-9".into()];
            assert_eq!(normalize_signal(&args, true), args);
        }
    }
    #[test]
    fn terminator_and_pgrep_leave_patterns_alone() {
        let args = ["--".into(), "-TERM".into()];
        assert_eq!(normalize_signal(&args, true), args);
        let args = ["-9".into()];
        assert_eq!(normalize_signal(&args, false), args);
    }
}
