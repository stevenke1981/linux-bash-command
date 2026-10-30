/// Split a supported size suffix while preserving each command's unit conventions.
pub fn split_suffix<'a>(text: &'a str, units: &[(&str, u64)], default: u64) -> (&'a str, u64) {
    units
        .iter()
        .find_map(|(suffix, multiplier)| {
            text.strip_suffix(suffix)
                .map(|digits| (digits, *multiplier))
        })
        .unwrap_or((text, default))
}

/// Formats like GNU `ls -h`: plain bytes below 1024, otherwise one decimal below 10
/// and a rounded-up integer above, using K, M, G, T, P, E. Values are rounded up.
pub fn human_size(bytes: u64) -> String {
    const SUFFIXES: [&str; 6] = ["K", "M", "G", "T", "P", "E"];
    if bytes < 1024 {
        return bytes.to_string();
    }
    let bytes = u128::from(bytes);
    let mut unit: u128 = 1024;
    let mut index = 0;
    loop {
        let tenths = ceil_div(bytes * 10, unit);
        let whole = ceil_div(tenths, 10);
        if whole >= 1024 && index + 1 < SUFFIXES.len() {
            unit *= 1024;
            index += 1;
            continue;
        }
        let suffix = SUFFIXES[index];
        return if tenths < 100 {
            format!("{}.{}{suffix}", tenths / 10, tenths % 10)
        } else {
            format!("{whole}{suffix}")
        };
    }
}

fn ceil_div(numerator: u128, denominator: u128) -> u128 {
    numerator.div_ceil(denominator)
}

/// GNU-style human size: one decimal below 10 units, rounded up, with the suffix.
pub fn human(bytes: u64, base: u64) -> String {
    let suffixes: [&str; 6] = if base == 1000 {
        ["", "k", "M", "G", "T", "P"]
    } else {
        ["", "K", "M", "G", "T", "P"]
    };
    let base = u128::from(base);
    let bytes = u128::from(bytes);
    let mut divisor: u128 = 1;
    let mut unit = 0;
    while unit + 1 < suffixes.len() && bytes >= divisor * base {
        divisor *= base;
        unit += 1;
    }
    if unit == 0 {
        return bytes.to_string();
    }
    let tenths = (bytes * 10).div_ceil(divisor);
    if tenths < 100 {
        format!("{}.{}{}", tenths / 10, tenths % 10, suffixes[unit])
    } else {
        format!("{}{}", bytes.div_ceil(divisor), suffixes[unit])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_sizes() {
        assert_eq!(super::human_size(1025), "1.1K");
        assert_eq!(super::human(1500, 1000), "1.5k");
    }

    #[test]
    fn splits_supported_suffixes_without_changing_unknown_values() {
        let units = [("KB", 1000), ("K", 1024), ("b", 512)];
        assert_eq!(super::split_suffix("5KB", &units, 1), ("5", 1000));
        assert_eq!(super::split_suffix("5K", &units, 1), ("5", 1024));
        assert_eq!(super::split_suffix("5x", &units, 1), ("5x", 1));
        assert_eq!(super::split_suffix("5", &units, 512), ("5", 512));
    }
}
