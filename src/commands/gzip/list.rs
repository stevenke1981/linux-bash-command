//! `gzip -l` table layout.
//!
//! GNU gzip reports the ratio from the compressed size minus the container overhead (the
//! header and trailer of a single member; none for a multi-member file). The totals line
//! subtracts the overhead of the last file listed. These rules were measured against
//! GNU gzip 1.12 output.

use std::time::SystemTime;

use crate::common::datetime::{format_in, from_system_time, zone_from_env};

const SHORT_HEADER: &str = "         compressed        uncompressed  ratio uncompressed_name";
const LONG_HEADER: &str =
    "method  crc     date  time           compressed        uncompressed  ratio uncompressed_name";
/// Blank prefix that lines up the totals row under the long layout.
const LONG_BLANK: &str = "                            ";

/// One listed file.
#[derive(Clone, Debug)]
pub struct Row {
    pub compressed: u64,
    pub uncompressed: u64,
    pub crc: u32,
    pub modified: Option<SystemTime>,
    /// Container overhead subtracted for the ratio: 0 for multi-member files.
    pub overhead: u64,
    pub name: String,
}

/// Ratio in tenths of a percent, rounded to the nearest value (ties away from zero).
pub fn ratio_text(uncompressed: u64, compressed_for_ratio: i128) -> String {
    if uncompressed == 0 {
        return "0.0%".to_string();
    }
    let saved = uncompressed as i128 - compressed_for_ratio;
    let tenths = saved as f64 * 1000.0 / uncompressed as f64;
    // The sign comes from the unrounded value: GNU prints "-0.0%" for a small loss.
    let sign = if tenths < 0.0 { "-" } else { "" };
    let magnitude = tenths.abs().round() as u64;
    format!("{sign}{}.{}%", magnitude / 10, magnitude % 10)
}

/// Header line for the chosen layout.
pub fn header(verbose: bool) -> String {
    if verbose {
        LONG_HEADER.to_string()
    } else {
        SHORT_HEADER.to_string()
    }
}

/// Formats one file's line, without the trailing newline.
pub fn format_row(verbose: bool, row: &Row) -> String {
    let ratio = ratio_text(
        row.uncompressed,
        i128::from(row.compressed) - i128::from(row.overhead),
    );
    let numbers = format!(
        "{:>19}{:>20} {:>6} {}",
        row.compressed, row.uncompressed, ratio, row.name
    );
    if !verbose {
        return numbers;
    }
    let date = row
        .modified
        .map(|t| format_in("%b %e %H:%M", from_system_time(t), &zone_from_env()))
        .unwrap_or_default();
    format!("defla {:08x} {date} {numbers}", row.crc)
}

/// Totals line over every listed file. `last_overhead` is the overhead of the last file
/// that was listed successfully.
pub fn format_totals(
    verbose: bool,
    compressed: u64,
    uncompressed: u64,
    last_overhead: u64,
) -> String {
    let ratio = ratio_text(
        uncompressed,
        i128::from(compressed) - i128::from(last_overhead),
    );
    let numbers = format!("{compressed:>19}{uncompressed:>20} {ratio:>6} (totals)");
    if verbose {
        format!("{LONG_BLANK}{numbers}")
    } else {
        numbers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(compressed: u64, uncompressed: u64, overhead: u64, name: &str) -> Row {
        Row {
            compressed,
            uncompressed,
            crc: 0x1234_abcd,
            modified: None,
            overhead,
            name: name.to_string(),
        }
    }

    #[test]
    fn ratio_matches_gnu_samples() {
        assert_eq!(ratio_text(1, 3), "-200.0%");
        assert_eq!(ratio_text(10, 5), "50.0%");
        assert_eq!(ratio_text(100, 6), "94.0%");
        assert_eq!(ratio_text(3000, 3005), "-0.2%");
        assert_eq!(ratio_text(0, 2), "0.0%");
    }

    #[test]
    fn short_row_has_gnu_columns() {
        let line = format_row(false, &row(21, 1, 18, "f1"));
        assert_eq!(line, "                 21                   1 -200.0% f1");
    }

    #[test]
    fn totals_row_uses_the_last_overhead() {
        let line = format_totals(false, 52, 11, 26);
        assert_eq!(
            line,
            "                 52                  11 -136.4% (totals)"
        );
    }

    #[test]
    fn header_lines_are_fixed() {
        assert!(header(false).ends_with("uncompressed_name"));
        assert!(header(true).starts_with("method  crc"));
        assert_eq!(header(false).len(), 64);
    }
}
