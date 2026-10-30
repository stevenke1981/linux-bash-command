//! Row composition for multi-column output: column heights, tab-aligned padding, and the
//! column separators (`-s`, `-S`, `-J`).

use std::io::{self, Write};

/// How adjacent columns are separated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Separator {
    /// Tab-aligned columns (the default): padding reaches each column start.
    Aligned,
    /// `-S STRING`: aligned, with STRING placed just before each column start.
    AlignedString(Vec<u8>),
    /// `-s CHAR` and `-J`: the separator follows the cell directly, no alignment.
    Plain(Vec<u8>),
}

/// Lines of one column for a page, as `(index into the page entries)` ranges.
pub(super) fn down_columns(count: usize, columns: usize) -> Vec<(usize, usize)> {
    let mut ranges = Vec::with_capacity(columns);
    let mut remaining = count;
    let mut offset = 0;
    for left in (1..=columns).rev() {
        let height = remaining.div_ceil(left);
        ranges.push((offset, offset + height));
        offset += height;
        remaining -= height;
    }
    ranges
}

/// Writes one composed row. `cells` holds the text of each column (`None` when the column
/// has no entry in this row). `pad_missing` also pads for missing cells (merge mode).
pub(super) fn write_row(
    out: &mut impl Write,
    margin: usize,
    cells: &[Option<&[u8]>],
    column_width: usize,
    separator: &Separator,
    pad_missing: bool,
) -> io::Result<()> {
    out.write_all(&vec![b' '; margin])?;
    let last_present = cells.iter().rposition(Option::is_some);
    let mut position = margin;
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            let present = cell.is_some();
            if !present && !pad_missing {
                // A missing trailing cell needs no separator.
                if last_present.is_none_or(|last| index > last) {
                    break;
                }
            }
            let target = margin + index * column_width;
            position = write_separator(out, separator, position, target)?;
        }
        if let Some(text) = cell {
            out.write_all(text)?;
            position += text.len();
        }
    }
    out.write_all(b"\n")
}

/// Emits whatever moves the output from `position` to the start of a column at `target`.
fn write_separator(
    out: &mut impl Write,
    separator: &Separator,
    position: usize,
    target: usize,
) -> io::Result<usize> {
    match separator {
        Separator::Plain(text) => {
            out.write_all(text)?;
            Ok(position + text.len())
        }
        Separator::AlignedString(text) => {
            let aim = target.saturating_sub(text.len());
            let mut at = position;
            let padded = pad_to(out, at, aim)?;
            at = padded;
            out.write_all(text)?;
            Ok(at + text.len())
        }
        Separator::Aligned => pad_to(out, position, target),
    }
}

/// Writes tabs (to 8-column stops) and then spaces from `position` up to `target`. Always
/// writes at least one space so that adjacent cells never run together.
fn pad_to(out: &mut impl Write, position: usize, target: usize) -> io::Result<usize> {
    let mut at = position;
    if at >= target {
        out.write_all(b" ")?;
        return Ok(at + 1);
    }
    loop {
        let next_stop = (at / 8 + 1) * 8;
        if next_stop <= target && next_stop > at {
            out.write_all(b"\t")?;
            at = next_stop;
        } else {
            break;
        }
    }
    out.write_all(&vec![b' '; target - at])?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_balance_over_remaining_columns() {
        assert_eq!(down_columns(7, 3), vec![(0, 3), (3, 5), (5, 7)]);
        assert_eq!(down_columns(4, 2), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn aligned_cells_pad_with_tabs_then_spaces() {
        let mut out = Vec::new();
        write_row(
            &mut out,
            0,
            &[Some(b"1"), Some(b"5")],
            10,
            &Separator::Aligned,
            false,
        )
        .unwrap();
        assert_eq!(out, b"1\t  5\n");
    }

    #[test]
    fn plain_separator_follows_each_cell() {
        let mut out = Vec::new();
        write_row(
            &mut out,
            0,
            &[Some(b"1"), Some(b"5")],
            10,
            &Separator::Plain(b",".to_vec()),
            false,
        )
        .unwrap();
        assert_eq!(out, b"1,5\n");
    }
}
