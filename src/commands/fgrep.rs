pub fn run(args: &[String]) -> i32 {
    crate::grep_command::run_alias(&arguments(args))
}
fn arguments(args: &[String]) -> Vec<String> {
    let mut forwarded = Vec::with_capacity(args.len() + 1);
    forwarded.push("-F".into());
    forwarded.extend_from_slice(args);
    forwarded
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty() {
        assert_eq!(arguments(&[]), ["-F"]);
    }
    #[test]
    fn operand() {
        assert_eq!(arguments(&["a".into()]), ["-F", "a"]);
    }
    #[test]
    fn options() {
        assert_eq!(arguments(&["-i".into(), "a".into()]), ["-F", "-i", "a"]);
    }
    #[test]
    fn terminator() {
        assert_eq!(arguments(&["--".into(), "-x".into()]), ["-F", "--", "-x"]);
    }
    #[test]
    fn override_syntax() {
        assert_eq!(arguments(&["-G".into(), "a".into()]), ["-F", "-G", "a"]);
    }
}
