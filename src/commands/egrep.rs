pub fn run(args: &[String]) -> i32 {
    crate::grep_command::run_alias(&arguments(args))
}
fn arguments(args: &[String]) -> Vec<String> {
    let mut forwarded = Vec::with_capacity(args.len() + 1);
    forwarded.push("-E".into());
    forwarded.extend_from_slice(args);
    forwarded
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty() {
        assert_eq!(arguments(&[]), ["-E"]);
    }
    #[test]
    fn operand() {
        assert_eq!(arguments(&["a".into()]), ["-E", "a"]);
    }
    #[test]
    fn options() {
        assert_eq!(arguments(&["-i".into(), "a".into()]), ["-E", "-i", "a"]);
    }
    #[test]
    fn terminator() {
        assert_eq!(arguments(&["--".into(), "-x".into()]), ["-E", "--", "-x"]);
    }
    #[test]
    fn override_syntax() {
        assert_eq!(arguments(&["-G".into(), "a".into()]), ["-E", "-G", "a"]);
    }
}
