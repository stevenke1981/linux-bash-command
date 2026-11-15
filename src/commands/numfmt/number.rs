#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Scale {
    None,
    Auto,
    Si,
    Iec,
    IecI,
}
impl Scale {
    pub fn parse(value: &str, input: bool) -> Result<Self, String> {
        match value {
            "none" => Ok(Self::None),
            "auto" if input => Ok(Self::Auto),
            "si" => Ok(Self::Si),
            "iec" => Ok(Self::Iec),
            "iec-i" => Ok(Self::IecI),
            _ => Err(format!("invalid scaling value '{value}'")),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Round {
    Up,
    Down,
    Away,
    Towards,
    Nearest,
}
impl Round {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "from-zero" => Ok(Self::Away),
            "towards-zero" => Ok(Self::Towards),
            "nearest" => Ok(Self::Nearest),
            _ => Err(format!("invalid rounding method '{value}'")),
        }
    }
}

#[derive(Default)]
pub struct Format {
    pub prefix: String,
    pub suffix: String,
    pub width: usize,
    pub precision: Option<usize>,
    pub zero: bool,
    pub left: bool,
    pub plus: bool,
    pub space: bool,
}
impl Format {
    pub fn parse(value: &str) -> Result<Self, String> {
        let error = || format!("invalid format '{value}', directive must be %f");
        let bytes = value.as_bytes();
        let mut i = 0;
        let mut literal = String::new();
        let mut result = Self::default();
        let mut found = false;
        while i < bytes.len() {
            if bytes[i] != b'%' {
                let c = value[i..].chars().next().unwrap();
                literal.push(c);
                i += c.len_utf8();
                continue;
            }
            i += 1;
            if bytes.get(i) == Some(&b'%') {
                literal.push('%');
                i += 1;
                continue;
            }
            if found {
                return Err(error());
            }
            found = true;
            result.prefix = std::mem::take(&mut literal);
            while let Some(b) = bytes.get(i) {
                match b {
                    b'0' => result.zero = true,
                    b'-' => result.left = true,
                    b'+' => result.plus = true,
                    b' ' => result.space = true,
                    b'\'' => {}
                    _ => break,
                }
                i += 1
            }
            let start = i;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1
            }
            if i > start {
                result.width = value[start..i].parse().map_err(|_| error())?;
            }
            if bytes.get(i) == Some(&b'.') {
                i += 1;
                let start = i;
                while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1
                }
                result.precision = Some(if start == i {
                    0
                } else {
                    value[start..i].parse().map_err(|_| error())?
                });
            }
            if bytes.get(i) != Some(&b'f')
                || result.width > 1_000_000
                || result.precision.is_some_and(|p| p > 1_000_000)
            {
                return Err(error());
            }
            i += 1;
        }
        if !found {
            return Err(error());
        }
        result.suffix = literal;
        Ok(result)
    }
}

pub fn unit(text: &str) -> Result<i128, String> {
    if text.is_empty() {
        return Err("invalid unit size ''".into());
    }
    let start = text.bytes().take_while(u8::is_ascii_digit).count();
    let count = if start == 0 {
        1
    } else {
        text[..start]
            .parse::<i128>()
            .map_err(|_| format!("invalid unit size '{text}'"))?
    };
    let suffix = &text[start..];
    let multiplier = if suffix.is_empty() {
        1
    } else {
        let index = b"KMGTPEZY"
            .iter()
            .position(|b| suffix.as_bytes().first() == Some(b))
            .ok_or_else(|| format!("invalid unit size '{text}'"))?
            + 1;
        let base = if suffix.ends_with('B') && !suffix.ends_with("iB") {
            1000i128
        } else {
            1024i128
        };
        if suffix.len() > 1 && !matches!(&suffix[1..], "B" | "iB" | "i") {
            return Err(format!("invalid unit size '{text}'"));
        }
        base.pow(index as u32)
    };
    count
        .checked_mul(multiplier)
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("invalid unit size '{text}'"))
}
pub struct Number {
    numerator: i128,
    raw: i128,
    denominator: i128,
    precision: usize,
    negative: bool,
}
fn overflow() -> String {
    "value too large to be converted".into()
}
fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        let c = a % b;
        a = b;
        b = c;
    }
    a.abs().max(1)
}
impl Number {
    pub fn parse(text: &str, scale: Scale) -> Result<Self, String> {
        let bytes = text.as_bytes();
        let negative = bytes.first() == Some(&b'-');
        let mut i = usize::from(matches!(bytes.first(), Some(b'-' | b'+')));
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1
        }
        let whole = &text[start..i];
        let mut fraction = "";
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            let start = i;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1
            }
            fraction = &text[start..i];
        }
        if whole.is_empty() && fraction.is_empty() {
            return Err(format!("invalid number: '{text}'"));
        }
        let suffix = &text[i..];
        let (power, base) = if suffix.is_empty() {
            (0, 1i128)
        } else {
            let power = b"KMGTPEZY"
                .iter()
                .position(|b| suffix.as_bytes().first() == Some(b))
                .ok_or_else(|| format!("invalid suffix in input: '{text}'"))?
                + 1;
            let binary = suffix.ends_with('i');
            let valid = match scale {
                Scale::None => false,
                Scale::Auto => suffix.len() == 1 || (binary && suffix.len() == 2),
                Scale::Si | Scale::Iec => suffix.len() == 1,
                Scale::IecI => binary && suffix.len() == 2,
            };
            if !valid {
                return Err(format!("invalid suffix in input: '{text}'"));
            }
            (
                power,
                if binary || matches!(scale, Scale::Iec | Scale::IecI) {
                    1024i128
                } else {
                    1000i128
                },
            )
        };
        if fraction.len() > 18 {
            return Err(overflow());
        }
        let denominator = 10i128.pow(fraction.len() as u32);
        let raw = format!("{}{}", if whole.is_empty() { "0" } else { whole }, fraction)
            .parse::<i128>()
            .map_err(|_| overflow())?;
        let numerator = raw
            .checked_mul(base.pow(power as u32))
            .ok_or_else(overflow)?;
        Ok(Self {
            numerator: if negative { -numerator } else { numerator },
            raw,
            denominator,
            precision: fraction.len().saturating_sub(power * 3),
            negative,
        })
    }
    pub fn render(
        &self,
        from: i128,
        to: i128,
        scale: Scale,
        round: Round,
        format: &Format,
    ) -> Result<String, String> {
        if format.precision.is_some_and(|p| p > 18) {
            return Err("value/precision too large to be printed".into());
        }
        let cancel = gcd(from, self.denominator);
        let mut numerator = self
            .numerator
            .checked_mul(from / cancel)
            .ok_or_else(overflow)?;
        let mut denominator = (self.denominator / cancel)
            .checked_mul(to)
            .ok_or_else(overflow)?;
        let cancel = gcd(numerator, denominator);
        numerator /= cancel;
        denominator /= cancel;
        let base = if scale == Scale::Si {
            1000i128
        } else {
            1024i128
        };
        let mut power = 0;
        if scale != Scale::None {
            while power < 8 && numerator.abs() / denominator >= base {
                denominator = denominator.checked_mul(base).ok_or_else(overflow)?;
                power += 1
            }
        }
        if scale == Scale::None && numerator.abs() / denominator >= 10i128.pow(19) {
            return Err("value too large to be printed (consider using --to)".into());
        }
        let mut precision = format.precision.unwrap_or(if scale == Scale::None {
            self.precision
        } else if numerator.abs() / denominator < 10 {
            1
        } else {
            0
        });
        let mut binary = super::binary::Binary::ratio(self.raw as u128, self.denominator as u128);
        binary.negative = self.negative;
        let input_factor = if self.raw == 0 {
            1
        } else {
            self.numerator.abs() / self.raw
        };
        binary = binary
            .multiply(super::binary::Binary::integer(input_factor))
            .multiply(super::binary::Binary::integer(from))
            .divide(super::binary::Binary::integer(to));
        for _ in 0..power {
            binary = binary.divide(super::binary::Binary::integer(base));
        }
        let mut rounded = binary
            .multiply(super::binary::Binary::integer(10i128.pow(precision as u32)))
            .rounded(round)?;

        if scale != Scale::None
            && power == 8
            && rounded.abs() >= base * 10i128.pow(precision as u32)
        {
            return Err("value too large to be converted".into());
        }
        if scale != Scale::None && power < 8 && rounded.abs() >= base * 10i128.pow(precision as u32)
        {
            power += 1;
            precision = format.precision.unwrap_or(1);
            binary = binary.divide(super::binary::Binary::integer(base));
            rounded = binary
                .multiply(super::binary::Binary::integer(10i128.pow(precision as u32)))
                .rounded(round)?;
        }
        if scale != Scale::None && power == 0 && format.precision.is_none() {
            // The unscaled representation has no fractional digits, after the
            // human-scaling precision has been applied.
            let divisor = 10i128.pow(precision as u32);
            let whole = rounded / divisor;
            let remainder = rounded % divisor;
            rounded = whole
                + if remainder.abs() * 2 > divisor
                    || (remainder.abs() * 2 == divisor && whole % 2 != 0)
                {
                    rounded.signum()
                } else {
                    0
                };
            precision = 0;
        }
        let divisor = 10i128.pow(precision as u32);
        let abs = rounded.unsigned_abs();
        let sign = if rounded < 0 || (rounded == 0 && self.negative && scale != Scale::None) {
            "-"
        } else if format.plus {
            "+"
        } else if format.space {
            " "
        } else {
            ""
        };
        let mut number = format!("{sign}{}", abs / divisor as u128);
        if precision > 0 {
            number.push_str(&format!(".{:0precision$}", abs % divisor as u128));
        }
        let suffix = if power == 0 {
            String::new()
        } else {
            format!(
                "{}{}",
                char::from(b"KMGTPEZY"[power - 1]),
                if scale == Scale::IecI { "i" } else { "" }
            )
        };
        let padding = format.width.saturating_sub(
            number.len()
                + if format.zero && !format.left {
                    0
                } else {
                    suffix.len()
                },
        );
        if format.left {
            number.push_str(&suffix);
            number.push_str(&" ".repeat(padding));
        } else if format.zero {
            number = format!(
                "{sign}{}{}{}",
                "0".repeat(padding),
                &number[sign.len()..],
                suffix
            );
        } else {
            number = format!("{}{number}{suffix}", " ".repeat(padding));
        }
        Ok(number)
    }
}
#[cfg(test)]
fn round_number(
    numerator: i128,
    denominator: i128,
    precision: usize,
    round: Round,
) -> Result<i128, String> {
    let scaled = numerator
        .checked_mul(10i128.pow(precision as u32))
        .ok_or_else(overflow)?;
    let whole = scaled / denominator;
    let remainder = scaled % denominator;
    let increase = match round {
        Round::Up => remainder > 0,
        Round::Down => remainder < 0,
        Round::Away => remainder != 0,
        Round::Towards => false,
        Round::Nearest => {
            remainder.unsigned_abs() >= denominator as u128 - remainder.unsigned_abs()
        }
    };
    Ok(if increase {
        whole + if scaled < 0 { -1 } else { 1 }
    } else {
        whole
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimal() {
        let n = Number::parse("1.234K", Scale::Si).unwrap();
        assert_eq!(
            n.render(1, 1, Scale::None, Round::Away, &Format::default())
                .unwrap(),
            "1234"
        );
    }
    #[test]
    fn negative_rounding() {
        assert_eq!(round_number(-3, 2, 0, Round::Nearest).unwrap(), -2);
    }
    #[test]
    fn reject_exponents() {
        assert!(Number::parse("1e3", Scale::None).is_err());
    }
    #[test]
    fn binary_unit() {
        assert_eq!(unit("KiB").unwrap(), 1024);
    }
    #[test]
    fn empty_unit_is_invalid() {
        assert!(unit("").is_err());
    }
}
