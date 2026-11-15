use super::options::Options;
pub(super) fn display(bytes: &[u8], o: &Options) -> Vec<u8> {
    let mut out = Vec::new();
    let mut column = 0;
    for &byte in bytes {
        if let Some((tab, width)) = o.expand {
            if byte == tab {
                let spaces = width - column % width;
                out.extend(std::iter::repeat_n(b' ', spaces));
                column += spaces;
                continue;
            }
        }
        if (o.controls || o.octal) && byte != b'\t' && !(32..127).contains(&byte) {
            if o.controls && byte < 128 {
                out.extend([b'^', if byte == 127 { b'?' } else { byte + 64 }]);
                column += 2;
            } else {
                out.extend(format!("\\{byte:03o}").as_bytes());
                column += 4;
            }
        } else {
            out.push(byte);
            column = advance(column, byte);
        }
    }
    out
}
fn advance(column: usize, byte: u8) -> usize {
    match byte {
        b'\t' => (column / 8 + 1) * 8,
        b'\r' => 0,
        b'\x08' => column.saturating_sub(1),
        _ => column + 1,
    }
}
fn clip(bytes: &[u8], limit: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut column = 0;
    for &b in bytes {
        let next = advance(column, b);
        if next > limit {
            if b == b'\t' {
                out.extend(std::iter::repeat_n(b' ', limit.saturating_sub(column)));
            }
            break;
        }
        out.push(b);
        column = next;
    }
    out
}
fn columns(bytes: &[u8]) -> usize {
    bytes.iter().fold(0, |column, byte| advance(column, *byte))
}
fn compress(bytes: &[u8], tab: u8, width: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut column = 0;
    while i < bytes.len() {
        if bytes[i] != b' ' {
            out.push(bytes[i]);
            column = advance(column, bytes[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1
        }
        let mut spaces = i - start;
        while spaces > 0 {
            let next = width - column % width;
            if next <= spaces && (next > 1 || spaces > 1) {
                out.push(tab);
                column += next;
                spaces -= next
            } else {
                out.push(b' ');
                column += 1;
                spaces -= 1
            }
        }
    }
    out
}
pub(super) fn row(cells: &[Option<(Vec<u8>, i64)>], o: &Options) -> Result<Vec<u8>, String> {
    let count = cells.len();
    let unaligned = o.join || (o.unaligned && !o.width_set);
    let default = if o.join {
        b"\t".as_slice()
    } else {
        b" ".as_slice()
    };
    let separator = o.separator.as_deref().unwrap_or(default);
    let number_width = o.numbers.map_or(0, |(sep, digits)| {
        if sep == b'\t' {
            (digits / 8 + 1) * 8
        } else {
            digits + 1
        }
    });
    let width = o
        .width
        .saturating_sub(if o.merge { number_width } else { 0 })
        .saturating_sub(separator.len() * (count - 1))
        / count;
    if !unaligned && number_width >= width && count > 1 {
        return Err("page width too narrow".into());
    }
    let mut out = vec![b' '; o.margin];
    if o.merge {
        if let Some((separator, digits)) = o.numbers {
            let number = cells
                .iter()
                .flatten()
                .next()
                .map_or(0, |(_, number)| *number);
            out.extend(format!("{number:>digits$}").as_bytes());
            out.push(separator);
        }
    }

    let last = if o.merge {
        cells.len() - 1
    } else {
        cells.iter().rposition(Option::is_some).unwrap_or(0)
    };
    for (index, cell) in cells.iter().enumerate().take(last + 1) {
        let mut text = Vec::new();
        if let Some((bytes, number)) = cell {
            if let Some((sep, digits)) = o.numbers {
                if !o.merge {
                    text.extend(format!("{number:>digits$}").as_bytes());
                    text.push(sep);
                }
            }
            text.extend(display(bytes, o));
        }
        if !unaligned && (count > 1 || o.wide) {
            text = clip(&text, width)
        }
        let len = columns(&text);
        out.extend(text);
        if index < last {
            if !unaligned {
                out.extend(std::iter::repeat_n(b' ', width.saturating_sub(len)))
            }
            out.extend(separator);
        }
    }
    if o.tabs.is_some() || count > 1 || o.margin > 0 {
        let (tab, width) = o.tabs.unwrap_or((b'\t', 8));
        out = compress(&out, tab, width);
    }
    out.push(b'\n');
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tabs() {
        assert_eq!(compress(b"a       b", b'\t', 8), b"a\tb");
    }
    #[test]
    fn control_bytes() {
        let o = Options {
            controls: true,
            ..Default::default()
        };
        assert_eq!(display(b"\x01\xff", &o), b"^A\\377");
    }
    #[test]
    fn clipped_tab() {
        assert_eq!(clip(b"a\tb", 4), b"a   ");
    }
}
