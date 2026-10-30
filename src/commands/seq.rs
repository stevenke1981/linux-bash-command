use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};

const HELP: &str = "Usage: seq [OPTION]... LAST\n  or: seq [OPTION]... FIRST [INCREMENT] LAST\nPrint a sequence of numbers.\n  -f, --format=FORMAT     printf floating conversion\n  -s, --separator=STRING  separate numbers with STRING\n  -w, --equal-width       pad with zeroes to equal width\n      --help              display this help\n      --version           output version information\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Decimal {
    value: i128,
    places: u32,
}

impl Decimal {
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let neg = s.starts_with('-');
        let s = s.strip_prefix(['-', '+']).unwrap_or(s);
        let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
        if whole.is_empty() && frac.is_empty()
            || !whole
                .bytes()
                .chain(frac.bytes())
                .all(|c| c.is_ascii_digit())
        {
            return None;
        }
        let digits = format!("{whole}{frac}");
        let value = digits.parse::<i128>().ok()?;
        Some(Self {
            value: if neg { -value } else { value },
            places: frac.len().try_into().ok()?,
        })
    }
    fn scaled(self, places: u32) -> Option<i128> {
        self.value
            .checked_mul(10i128.checked_pow(places - self.places)?)
    }
}

fn fixed(value: i128, places: u32, width: usize) -> String {
    let mut digits = value.unsigned_abs().to_string();
    let places = places as usize;
    if places > 0 {
        if digits.len() <= places {
            digits = format!("{}{digits}", "0".repeat(places + 1 - digits.len()));
        }
        digits.insert(digits.len() - places, '.');
    }
    if value < 0 {
        digits.insert(0, '-');
    }
    if digits.len() < width {
        let zeros = "0".repeat(width - digits.len());
        if value < 0 {
            digits.insert_str(1, &zeros);
        } else {
            digits.insert_str(0, &zeros);
        }
    }
    digits
}

fn valid_format(s: &str) -> bool {
    let mut count = 0;
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b != b'%' {
            continue;
        }
        let Some(mut b) = bytes.next() else {
            return false;
        };
        if b == b'%' {
            continue;
        }
        while b"-+ #0.0123456789".contains(&b) {
            let Some(next) = bytes.next() else {
                return false;
            };
            b = next;
        }
        if !b"fFeEgGaA".contains(&b) {
            return false;
        }
        count += 1;
    }
    count == 1
}

pub fn run(arguments: &[String]) -> i32 {
    let escaped: Vec<_> = arguments
        .iter()
        .map(|s| {
            if s.starts_with('-')
                && (s[1..].starts_with(|c: char| c.is_ascii_digit() || c == '.')
                    || s == "-inf"
                    || s == "-infinity")
            {
                format!("\u{1}{s}")
            } else {
                s.clone()
            }
        })
        .collect();
    let specs = [
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
        OptSpec::value("format", 'f'),
        OptSpec::value("separator", 's'),
        OptSpec::flag("equal-width", 'w'),
    ];
    let parsed = match args::parse(&escaped, &specs, true) {
        Ok(p) => p,
        Err(e) => return usage("seq", &e),
    };
    let (mut format, mut separator, mut equal, mut values) =
        (None, "\n".to_string(), false, Vec::new());
    for item in parsed {
        match item {
            Item::Opt(id @ ("help" | "version"), _) => {
                return args::help_version("seq", &[format!("--{id}")], HELP, true).unwrap()
            }
            Item::Opt("format", Some(v)) => format = Some(v),
            Item::Opt("separator", Some(v)) => separator = v,
            Item::Opt("equal-width", _) => equal = true,
            Item::Operand(v) => values.push(v.strip_prefix('\u{1}').unwrap_or(&v).to_string()),
            _ => unreachable!(),
        }
    }
    if values.is_empty() {
        return usage("seq", "missing operand");
    }
    if values.len() > 3 {
        return usage("seq", &format!("extra operand '{}'", values[3]));
    }
    if equal && format.is_some() {
        return usage(
            "seq",
            "format string may not be specified when printing equal width strings",
        );
    }
    if format.as_ref().is_some_and(|f| !valid_format(f)) {
        return usage("seq", "format must have exactly one floating conversion");
    }
    let (first, increment, last) = match values.as_slice() {
        [last] => ("1", "1", last.as_str()),
        [first, last] => (first.as_str(), "1", last.as_str()),
        [first, increment, last] => (first.as_str(), increment.as_str(), last.as_str()),
        _ => unreachable!(),
    };
    let decimals = [first, increment, last].map(Decimal::parse);
    if format.is_none() && decimals.iter().all(Option::is_some) {
        let numbers = decimals.map(Option::unwrap);
        let places = numbers.iter().map(|n| n.places).max().unwrap();
        let Some(numbers) = numbers
            .iter()
            .map(|n| n.scaled(places))
            .collect::<Option<Vec<_>>>()
        else {
            return usage("seq", "numeric value is out of range");
        };
        let (start, step, last) = (numbers[0], numbers[1], numbers[2]);
        if step == 0 {
            return usage("seq", "invalid Zero increment value: '0'");
        }
        let width = if equal {
            fixed(start, places, 0)
                .len()
                .max(fixed(last, places, 0).len())
        } else {
            0
        };
        return io::output("seq", |out| {
            let (mut current, mut printed) = (start, false);
            while if step > 0 {
                current <= last
            } else {
                current >= last
            } {
                if printed {
                    out.write_all(separator.as_bytes())?;
                }
                printed = true;
                out.write_all(fixed(current, places, width).as_bytes())?;
                let Some(next) = current.checked_add(step) else {
                    break;
                };
                current = next;
            }
            if printed {
                out.write_all(b"\n")?;
            }
            Ok(())
        });
    }
    let floats = [first, increment, last].map(|s| s.parse::<f64>());
    let [Ok(start), Ok(step), Ok(last)] = floats else {
        return usage("seq", "invalid floating point argument");
    };
    if start.is_nan()
        || step.is_nan()
        || last.is_nan()
        || step == 0.0
        || !start.is_finite()
        || !step.is_finite()
    {
        return usage("seq", "invalid floating point argument");
    }
    let width = if equal {
        super::printf::float_format(start, b'g', None, false)
            .len()
            .max(super::printf::float_format(last, b'g', None, false).len())
    } else {
        0
    };
    io::output("seq", |out| {
        let (mut index, mut printed) = (0u128, false);
        loop {
            let value = start + index as f64 * step;
            if if step > 0.0 {
                value > last
            } else {
                value < last
            } {
                break;
            }
            if printed {
                out.write_all(separator.as_bytes())?;
            }
            printed = true;
            if let Some(format) = &format {
                super::printf::render(format.as_bytes(), &[value.to_string()], out)?;
            } else {
                let mut text = super::printf::float_format(value.abs(), b'g', None, false);
                if value.is_sign_negative() {
                    text.insert(0, '-');
                }
                if text.len() < width {
                    let zeros = "0".repeat(width - text.len());
                    text.insert_str(usize::from(value.is_sign_negative()), &zeros);
                }
                out.write_all(text.as_bytes())?;
            }
            index += 1;
        }
        if printed {
            out.write_all(b"\n")?;
        }
        Ok(())
    })
}

fn usage(command: &str, message: &str) -> i32 {
    error::usage(command, message, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal() {
        assert_eq!(
            Decimal::parse("-0.10"),
            Some(Decimal {
                value: -10,
                places: 2
            })
        );
    }
    #[test]
    fn scale() {
        assert_eq!(Decimal::parse("1.2").unwrap().scaled(3), Some(1200));
    }
    #[test]
    fn fixed_fraction() {
        assert_eq!(fixed(1, 3, 0), "0.001");
    }
    #[test]
    fn negative_pad() {
        assert_eq!(fixed(-12, 1, 6), "-001.2");
    }
    #[test]
    fn invalid_decimal() {
        assert!(Decimal::parse("1e2").is_none());
        assert!(Decimal::parse(".").is_none());
    }
    #[test]
    fn formats() {
        assert!(valid_format("x%06.2f%%"));
        assert!(!valid_format("%d"));
        assert!(!valid_format("%f%g"));
        assert!(!valid_format("%*f"));
    }
}
