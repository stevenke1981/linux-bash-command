// Portions derived from uutils/coreutils (MIT), https://github.com/uutils/coreutils
//! Byte-oriented context windows and plain/roff/TeX rendering.
use super::options::{Format, Options};
use std::ops::Range;
fn trim(bytes: &[u8], mut range: Range<usize>) -> Range<usize> {
    while range.start < range.end && bytes[range.end - 1].is_ascii_whitespace() {
        range.end -= 1
    }
    while range.start < range.end && bytes[range.start].is_ascii_whitespace() {
        range.start += 1
    }
    range
}
fn word(byte: u8, o: &Options) -> bool {
    if o.traditional && !o.custom_words {
        !byte.is_ascii_whitespace()
    } else if let Some(breaks) = &o.breaks {
        !breaks.contains(&byte)
    } else {
        byte.is_ascii_alphabetic()
    }
}
fn ending(bytes: &[u8], range: Range<usize>, width: usize, o: &Options) -> Range<usize> {
    let range = trim(bytes, range);
    let mut start = range.end.saturating_sub(width).max(range.start);
    if start > range.start
        && start < range.end
        && word(bytes[start - 1], o)
        && word(bytes[start], o)
    {
        while start < range.end && word(bytes[start], o) {
            start += 1
        }
    }
    trim(bytes, start..range.end)
}
fn starting(bytes: &[u8], range: Range<usize>, width: usize, o: &Options) -> Range<usize> {
    let mut end = (range.start + width).min(range.end);
    if end > range.start && end < range.end && word(bytes[end - 1], o) && word(bytes[end], o) {
        while end > range.start && word(bytes[end - 1], o) {
            end -= 1
        }
    }
    let end = trim(bytes, range.start..end).end;
    range.start..end
}
struct Chunks {
    tail: Vec<u8>,
    before: Vec<u8>,
    keyword: Vec<u8>,
    after: Vec<u8>,
    head: Vec<u8>,
}
fn chunks(before: &[u8], keyword: &[u8], after: &[u8], o: &Options, width: usize) -> Chunks {
    let half = width.saturating_sub(3) / 2;
    let before_range = ending(before, 0..before.len(), half.saturating_sub(1), o);
    let after_range = starting(after, 0..after.len(), half.saturating_sub(keyword.len()), o);
    let tail_width = half.saturating_sub(1 + before_range.len() + o.gap + o.flag.len());
    let tail_start = trim(after, after_range.end..after.len()).start;
    let tail = starting(after, tail_start..after.len(), tail_width, o);
    let head_width = half.saturating_sub(keyword.len() + after_range.len() + o.gap);
    let head = ending(before, 0..before_range.start, head_width, o);
    let mut c = Chunks {
        tail: after[tail.clone()].to_vec(),
        before: before[before_range.clone()].to_vec(),
        keyword: keyword.to_vec(),
        after: after[after_range.clone()].to_vec(),
        head: before[head.clone()].to_vec(),
    };
    if o.format != Format::Tex {
        if after_range.end != after.len() {
            if tail.is_empty() {
                c.after.extend(&o.flag)
            } else if tail.end != after.len() {
                c.tail.extend(&o.flag)
            }
        }
        if before_range.start != 0 {
            if head.is_empty() {
                let mut b = o.flag.clone();
                b.extend(c.before);
                c.before = b;
            } else if head.start != 0 {
                let mut b = o.flag.clone();
                b.extend(c.head);
                c.head = b;
            }
        }
    }
    c
}
fn quoted(bytes: &[u8], tex: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for b in bytes {
        if tex {
            match b {
                b'\\' => out.extend(b"\\backslash{}"),
                b'$' | b'%' | b'#' | b'&' | b'_' => out.extend([b'\\', *b]),
                b'{' | b'}' => out.extend([b'$', b'\\', *b, b'$']),
                _ => out.push(*b),
            }
        } else if *b == b'"' {
            out.extend(b"\"\"")
        } else {
            out.push(*b)
        }
    }
    out
}
pub(super) fn render(
    before: &[u8],
    keyword: &[u8],
    after: &[u8],
    reference: &[u8],
    o: &Options,
    width: usize,
) -> Vec<u8> {
    let before = &before[trim(before, 0..before.len())];
    let end = trim(after, 0..after.len()).end;
    let after = &after[..end];
    let reference_width = o.width.saturating_sub(width);
    let c = chunks(
        before,
        keyword,
        after,
        o,
        width.saturating_sub(if reference_width > 0 { o.gap } else { 0 }),
    );
    let refs = o.automatic || o.references;
    let mut out = Vec::new();
    match o.format {
        Format::Roff => {
            out.extend(format!(".{}", o.macro_name).as_bytes());
            let mut right = c.keyword;
            right.extend(c.after);
            for field in [&c.tail, &c.before, &right, &c.head] {
                out.extend(b" \"");
                out.extend(quoted(field, false));
                out.push(b'"');
            }
            if refs {
                out.extend(b" \"");
                out.extend(quoted(reference, false));
                out.push(b'"');
            }
        }
        Format::Tex => {
            out.extend(format!("\\{} ", o.macro_name).as_bytes());
            for field in [&c.tail, &c.before, &c.keyword, &c.after, &c.head] {
                out.push(b'{');
                out.extend(quoted(field, true));
                out.push(b'}');
            }
            if refs {
                out.push(b'{');
                out.extend(quoted(reference, true));
                out.push(b'}');
            }
        }
        Format::Plain => {
            let keyword_start = if refs && !o.right {
                (o.width + reference_width + o.gap) / 2
            } else if o.right {
                o.width / 2
            } else {
                o.width / 2 + o.gap
            };
            let edge = keyword_start.saturating_sub(o.gap);
            let prefix = if refs && !o.right {
                let mut p = reference.to_vec();
                if o.automatic {
                    p.push(b':')
                }
                p
            } else {
                Vec::new()
            };
            out.extend(&prefix);
            if !c.tail.is_empty() {
                out.resize(o.gap.max(out.len()), b' ');
                out.extend(&c.tail);
            }
            let before_start = edge.saturating_sub(c.before.len());
            out.resize(before_start.max(out.len()), b' ');
            out.extend(&c.before);
            out.resize(keyword_start.max(out.len() + o.gap), b' ');
            out.extend(&c.keyword);
            out.extend(&c.after);
            if !c.head.is_empty() {
                let end = width + if o.right { 0 } else { o.gap };
                out.resize(
                    end.saturating_sub(c.head.len()).max(out.len() + o.gap),
                    b' ',
                );
                out.extend(&c.head);
            }
            if refs && o.right {
                out.resize((o.width + o.gap).max(out.len() + o.gap), b' ');
                out.extend(reference);
            }
        }
    }
    out.push(b'\n');
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tex_escapes() {
        assert_eq!(quoted(b"a_b$", true), b"a\\_b\\$");
    }
    #[test]
    fn roff_quotes() {
        assert_eq!(quoted(b"a\"b", false), b"a\"\"b");
    }
    #[test]
    fn trim_bytes() {
        assert_eq!(trim(b" a \t", 0..4), 1..2);
    }
}
