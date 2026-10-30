//! Arrangement of `ls` items on lines: columns (`-C`, sorted down), rows (`-x`, across) and
//! comma separated lists (`-m`), following GNU `ls` for widths, wrapping and tab padding.

/// Columns are never narrower than this (GNU's `MIN_COLUMN_WIDTH`).
const MIN_COLUMN_WIDTH: usize = 3;
/// Two spaces separate neighbouring columns.
const SEPARATOR: usize = 2;

/// Lays out `items` in columns that fit `width`. With `across` the items run along rows;
/// otherwise they run down the columns. Padding uses tabs every `tabsize` columns (spaces when
/// `tabsize` is 0). Returns the text, ending with a newline when there is any item.
pub fn columns(items: &[String], width: usize, tabsize: usize, across: bool) -> String {
    if items.is_empty() {
        return String::new();
    }
    let lengths: Vec<usize> = items.iter().map(|item| item.chars().count()).collect();
    let geometry = choose_geometry(&lengths, width, across);
    let mut out = String::new();
    for row in 0..geometry.rows {
        let mut pos = 0;
        for column in 0..geometry.columns {
            let Some(index) = geometry.index(row, column) else {
                break;
            };
            out.push_str(&items[index]);
            pos += lengths[index];
            let last = column + 1 == geometry.columns || geometry.index(row, column + 1).is_none();
            if !last {
                let target = geometry.start(column + 1);
                indent(&mut out, pos, target, tabsize);
                pos = target;
            }
        }
        out.push('\n');
    }
    out
}

/// Comma separated output for `-m`: items are joined by `", "`, and a line is broken after a
/// comma when the next item would not fit in `width` (`usize::MAX` means no limit).
pub fn commas(items: &[String], width: usize) -> String {
    let limit = width;
    let mut out = String::new();
    let mut pos: usize = 0;
    for (index, item) in items.iter().enumerate() {
        let length = item.chars().count();
        if index > 0 {
            if pos.saturating_add(length).saturating_add(3) > limit {
                out.push_str(",\n");
                pos = 0;
            } else {
                out.push_str(", ");
                pos += 2;
            }
        }
        out.push_str(item);
        pos += length;
    }
    if !items.is_empty() {
        out.push('\n');
    }
    out
}

/// Column count and widths for one layout.
struct Geometry {
    rows: usize,
    columns: usize,
    /// Content width of each column (the separator is added between columns).
    widths: Vec<usize>,
    across: bool,
    count: usize,
}

impl Geometry {
    /// The item shown at (`row`, `column`), if any.
    fn index(&self, row: usize, column: usize) -> Option<usize> {
        let index = if self.across {
            row * self.columns + column
        } else {
            column * self.rows + row
        };
        (index < self.count).then_some(index)
    }

    /// Position of the first character of `column` on a line.
    fn start(&self, column: usize) -> usize {
        self.widths[..column]
            .iter()
            .map(|width| width + SEPARATOR)
            .sum()
    }
}

/// The largest column count whose total width fits, or one column when none does.
fn choose_geometry(lengths: &[usize], width: usize, across: bool) -> Geometry {
    let count = lengths.len();
    let most = (width / MIN_COLUMN_WIDTH).clamp(1, count);
    for columns in (1..=most).rev() {
        let rows = count.div_ceil(columns);
        // Every column must hold at least one item; fewer columns may be needed.
        let columns = if across {
            columns
        } else {
            count.div_ceil(rows)
        };
        let geometry = Geometry {
            rows,
            columns,
            widths: column_widths(lengths, rows, columns, across),
            across,
            count,
        };
        let total: usize =
            geometry.widths.iter().sum::<usize>() + SEPARATOR * columns.saturating_sub(1);
        // GNU requires the total to be strictly narrower than the line.
        if columns == 1 || total < width {
            return geometry;
        }
    }
    Geometry {
        rows: count,
        columns: 1,
        widths: column_widths(lengths, count, 1, across),
        across,
        count,
    }
}

fn column_widths(lengths: &[usize], rows: usize, columns: usize, across: bool) -> Vec<usize> {
    (0..columns)
        .map(|column| {
            (0..rows)
                .filter_map(|row| {
                    let index = if across {
                        row * columns + column
                    } else {
                        column * rows + row
                    };
                    lengths.get(index).copied()
                })
                .max()
                .unwrap_or(0)
        })
        .collect()
}

/// Writes padding from column `from` to column `to`. A tab is written when it reaches a tab stop
/// that is not the position just after `from` (GNU's observed rule: `to / tabsize` must exceed
/// `(from + 1) / tabsize`), and spaces otherwise.
fn indent(out: &mut String, mut from: usize, to: usize, tabsize: usize) {
    while from < to {
        if tabsize > 0 && to / tabsize > (from + 1) / tabsize {
            out.push('\t');
            from += tabsize - from % tabsize;
        } else {
            out.push(' ');
            from += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn single_column_when_items_are_too_wide() {
        let text = columns(&items(&["aaaaaaaaaa", "bbbbbbbbbb"]), 10, 8, false);
        assert_eq!(text, "aaaaaaaaaa\nbbbbbbbbbb\n");
    }

    #[test]
    fn down_columns_fill_top_to_bottom() {
        // Four one-character items fit on one row at width 80; width 6 allows two columns.
        assert_eq!(
            columns(&items(&["a", "b", "c", "d"]), 80, 0, false),
            "a  b  c  d\n"
        );
        assert_eq!(
            columns(&items(&["a", "b", "c", "d"]), 6, 0, false),
            "a  c\nb  d\n"
        );
    }

    #[test]
    fn across_rows_fill_left_to_right() {
        assert_eq!(
            columns(&items(&["a", "b", "c", "d"]), 6, 0, true),
            "a  b\nc  d\n"
        );
    }

    #[test]
    fn padding_uses_tabs_at_tab_stops() {
        // From column 5 to column 20 (GNU ls -x with 18-wide names) the first tab stop is 8.
        let mut out = String::new();
        indent(&mut out, 5, 20, 8);
        assert_eq!(out, "\t\t    ");
        let mut spaced = String::new();
        indent(&mut spaced, 5, 20, 0);
        assert_eq!(spaced, " ".repeat(15));
    }

    #[test]
    fn commas_wrap_before_overflowing_items() {
        let text = commas(&items(&["a.txt", "bak~", "big.bin", "it's"]), 20);
        assert_eq!(text, "a.txt, bak~,\nbig.bin, it's\n");
        assert_eq!(commas(&items(&["x", "y"]), usize::MAX), "x, y\n");
        assert_eq!(commas(&[], usize::MAX), "");
    }
}
