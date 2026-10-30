use crate::common::{
    args::{parse_command, Item},
    error::usage,
};
use std::time::Duration;
const HELP: &str = "Usage: sleep NUMBER[SUFFIX]...\nPause for the sum of NUMBERs. SUFFIX is s (seconds), m, h or d.\n\n  --help     display this help\n  --version  output version information\n";

fn seconds(text: &str) -> Option<f64> {
    let (number, factor) = match text.as_bytes().last() {
        Some(b's') => (&text[..text.len() - 1], 1.0),
        Some(b'm') => (&text[..text.len() - 1], 60.0),
        Some(b'h') => (&text[..text.len() - 1], 3600.0),
        Some(b'd') => (&text[..text.len() - 1], 86400.0),
        _ => (text, 1.0),
    };
    let value = number.parse::<f64>().ok()? * factor;
    (value >= 0.0 && !value.is_nan()).then_some(value)
}

pub fn run(args: &[String]) -> i32 {
    let args = match parse_command("sleep", args, HELP, &[], true) {
        Ok(items) => items
            .into_iter()
            .map(|i| match i {
                Item::Operand(s) => s,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>(),
        Err(code) => return code,
    };
    let mut total = 0.0;
    let mut count = 0;
    for arg in &args {
        let Some(value) = seconds(arg) else {
            return usage("sleep", format!("invalid time interval '{arg}'"), 1);
        };
        total += value;
        count += 1;
    }
    if count == 0 {
        return usage("sleep", "missing operand", 1);
    }
    if total.is_infinite() {
        loop {
            std::thread::sleep(Duration::from_secs(86400));
        }
    }
    let Ok(duration) = Duration::try_from_secs_f64(total) else {
        return usage("sleep", "invalid time interval", 1);
    };
    std::thread::sleep(duration);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractions() {
        assert_eq!(seconds("0.25"), Some(0.25));
    }
    #[test]
    fn suffixes() {
        assert_eq!(seconds("2m"), Some(120.0));
        assert_eq!(seconds("1h"), Some(3600.0));
        assert_eq!(seconds("1d"), Some(86400.0));
    }
    #[test]
    fn infinity() {
        assert_eq!(seconds("infinity"), Some(f64::INFINITY));
    }
    #[test]
    fn negatives() {
        assert_eq!(seconds("-1"), None);
    }
    #[test]
    fn invalid() {
        assert_eq!(seconds("NaN"), None);
        assert_eq!(seconds("1x"), None);
    }
}
