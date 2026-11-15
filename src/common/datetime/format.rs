use jiff::Zoned;

/// Format GNU date directives with fixed C-locale composite formats.
pub fn format(date: &Zoned, pattern: &str) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }
        let mut flags = String::new();
        while matches!(chars.peek(), Some('-' | '_' | '0' | '^' | '#')) {
            flags.push(chars.next().unwrap());
        }
        let mut width = String::new();
        while matches!(chars.peek(), Some('0'..='9')) {
            width.push(chars.next().unwrap());
        }
        let mut colons = String::new();
        while chars.peek() == Some(&':') {
            colons.push(chars.next().unwrap());
        }
        let Some(spec) = chars.next() else {
            result.push('%');
            result.push_str(&flags);
            result.push_str(&width);
            result.push_str(&colons);
            break;
        };
        if !"aAbBcCdDeFgGhHIjklmMnNpPqrRsStTuUVwWxXyYzZ%".contains(spec) {
            result.push('%');
            result.push_str(&flags);
            result.push_str(&width);
            result.push_str(&colons);
            result.push(spec);
            continue;
        }
        if matches!(spec, 'C' | 's') && colons.is_empty() {
            let number = if spec == 'C' {
                i32::from(date.year()).div_euclid(100).to_string()
            } else {
                date.timestamp()
                    .as_nanosecond()
                    .div_euclid(1_000_000_000)
                    .to_string()
            };
            let width: usize = if flags.contains('-') {
                0
            } else if width.is_empty() {
                if spec == 'C' {
                    2
                } else {
                    0
                }
            } else {
                width.parse().map_err(|_| "invalid width")?
            };
            let padding = if flags.contains('_') { ' ' } else { '0' };
            let count = width.saturating_sub(number.len());
            if number.starts_with('-') && padding == '0' && count > 0 {
                result.push('-');
                result.extend(std::iter::repeat_n(padding, count));
                result.push_str(&number[1..]);
            } else {
                result.extend(std::iter::repeat_n(padding, count));
                result.push_str(&number);
            }
            continue;
        }
        if spec == 'N' && colons.is_empty() {
            let digits: usize = if width.is_empty() {
                9
            } else {
                width.parse().map_err(|_| "invalid width")?
            };
            let fraction = format!("{:09}", date.subsec_nanosecond());
            if digits <= 9 {
                result.push_str(&fraction[..digits]);
            } else {
                result.push_str(&fraction);
                result.push_str(&"0".repeat(digits - 9));
            }
            continue;
        }
        let replacement = match spec {
            'c' => Some("%a %b %e %H:%M:%S %Y"),
            'x' => Some("%m/%d/%y"),
            'X' => Some("%H:%M:%S"),
            'r' => Some("%I:%M:%S %p"),
            'D' => Some("%m/%d/%y"),
            _ => None,
        };
        if let Some(directive) = replacement {
            let mut rendered =
                jiff::fmt::strtime::format(directive, date).map_err(|e| e.to_string())?;
            if flags.contains('^') {
                rendered.make_ascii_uppercase();
            }
            let width: usize = if width.is_empty() {
                0
            } else {
                width.parse().map_err(|_| "invalid width")?
            };
            let padding = flags.chars().rev().find(|ch| matches!(ch, '-' | '_' | '0'));
            if padding != Some('-') {
                let fill = if padding == Some('0') { '0' } else { ' ' };
                result.extend(std::iter::repeat_n(
                    fill,
                    width.saturating_sub(rendered.len()),
                ));
            }
            result.push_str(&rendered);
        } else {
            let directive = format!("%{flags}{width}{colons}{spec}");
            let rendered =
                jiff::fmt::strtime::format(&directive, date).map_err(|e| e.to_string())?;
            result.push_str(&rendered);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn date() -> Zoned {
        "2026-01-02T03:04:05.123456789+00:00[UTC]".parse().unwrap()
    }
    #[test]
    fn composite_c_locale() {
        assert_eq!(
            format(&date(), "%c|%x|%r").unwrap(),
            "Fri Jan  2 03:04:05 2026|01/02/26|03:04:05 AM"
        );
    }
    #[test]
    fn nanos_width() {
        assert_eq!(
            format(&date(), "%N|%3N|%10N").unwrap(),
            "123456789|123|1234567890"
        );
    }
    #[test]
    fn offset_variants() {
        assert_eq!(
            format(&date(), "%z|%:z|%::z|%:::z").unwrap(),
            "+0000|+00:00|+00:00:00|+00"
        );
    }
    #[test]
    fn formatting_flags() {
        assert_eq!(format(&date(), "%-d|%_H|%^b").unwrap(), "2| 3|JAN");
    }
    #[test]
    fn iso_week_and_seconds() {
        assert_eq!(
            format(&date(), "%G-%V-%u|%s").unwrap(),
            "2026-01-5|1767323045"
        );
    }
    #[test]
    fn unknown_conversions_are_literal() {
        assert_eq!(format(&date(), "%Q %f %J").unwrap(), "%Q %f %J");
    }
    #[test]
    fn century_padding() {
        let date: Zoned = "0009-01-01T00:00:00+00:00[UTC]".parse().unwrap();
        assert_eq!(format(&date, "%C|%4C|%_4C|%-C").unwrap(), "00|0000|   0|0");
    }
    #[test]
    fn composite_case_and_width() {
        assert_eq!(format(&date(), "%^c|%30c|%030c|%-30c").unwrap(), "FRI JAN  2 03:04:05 2026|      Fri Jan  2 03:04:05 2026|000000Fri Jan  2 03:04:05 2026|Fri Jan  2 03:04:05 2026");
    }
}
