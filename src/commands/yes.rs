use crate::common::{
    args::{parse_command, Item},
    io::output,
};
const HELP: &str = "Usage: yes [STRING]...\nRepeatedly output a line with all specified STRINGs, or 'y'.\n\n  --help     display this help\n  --version  output version information\n";

fn block(args: &[String]) -> Vec<u8> {
    let mut line = if args.is_empty() {
        b"y".to_vec()
    } else {
        args.join(" ").into_bytes()
    };
    line.push(b'\n');
    line.repeat(8192usize.div_ceil(line.len()))
}

pub fn run(args: &[String]) -> i32 {
    let args = match parse_command("yes", args, HELP, &[], true) {
        Ok(items) => items
            .into_iter()
            .map(|i| match i {
                Item::Operand(s) => s,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>(),
        Err(code) => return code,
    };
    let bytes = block(&args);
    output("yes", |out| loop {
        out.write_all(&bytes)?;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default() {
        assert!(block(&[]).starts_with(b"y\ny\n"));
    }
    #[test]
    fn size() {
        assert!(block(&[]).len() >= 8192);
    }
    #[test]
    fn empty_operand() {
        assert!(block(&["".into()]).iter().all(|b| *b == b'\n'));
    }
    #[test]
    fn joined() {
        assert!(block(&["a".into(), "b".into()]).starts_with(b"a b\na b\n"));
    }
    #[test]
    fn large_line() {
        assert_eq!(block(&["a".repeat(9000)]).len(), 9001);
    }
}
