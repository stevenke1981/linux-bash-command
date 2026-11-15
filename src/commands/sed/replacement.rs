#[derive(Clone, Debug)]
pub enum Part {
    Literal(Vec<u8>),
    Group(usize),
    Lower,
    Upper,
    LowerOne,
    UpperOne,
    End,
}

pub fn parse(bytes: &[u8]) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut literal = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        i += 1;
        let part = match byte {
            b'&' => Some(Part::Group(0)),
            b'\\' if i < bytes.len() => {
                let next = bytes[i];
                i += 1;
                match next {
                    b'1'..=b'9' => Some(Part::Group(usize::from(next - b'0'))),
                    b'L' => Some(Part::Lower),
                    b'U' => Some(Part::Upper),
                    b'l' => Some(Part::LowerOne),
                    b'u' => Some(Part::UpperOne),
                    b'E' => Some(Part::End),
                    _ => {
                        literal.push(escape(next));
                        None
                    }
                }
            }
            _ => {
                literal.push(byte);
                None
            }
        };
        if let Some(part) = part {
            if !literal.is_empty() {
                parts.push(Part::Literal(std::mem::take(&mut literal)));
            }
            parts.push(part);
        }
    }
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }
    parts
}

pub fn escape(byte: u8) -> u8 {
    match byte {
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'a' => 7,
        b'b' => 8,
        b'f' => 12,
        b'v' => 11,
        _ => byte,
    }
}

pub fn expand(
    parts: &[Part],
    subject: &[u8],
    groups: &[Option<(usize, usize)>],
    out: &mut Vec<u8>,
) {
    let (mut mode, mut once) = (0, 0);
    for part in parts {
        let bytes: &[u8] = match part {
            Part::Literal(bytes) => bytes,
            Part::Group(index) => groups
                .get(*index)
                .and_then(|g| *g)
                .map_or(&[], |(a, b)| &subject[a..b]),
            Part::Lower => {
                mode = -1;
                once = 0;
                continue;
            }
            Part::Upper => {
                mode = 1;
                once = 0;
                continue;
            }
            Part::LowerOne => {
                once = -1;
                continue;
            }
            Part::UpperOne => {
                once = 1;
                continue;
            }
            Part::End => {
                mode = 0;
                once = 0;
                continue;
            }
        };
        for &byte in bytes {
            out.push(
                match if once != 0 {
                    std::mem::take(&mut once)
                } else {
                    mode
                } {
                    -1 => byte.to_ascii_lowercase(),
                    1 => byte.to_ascii_uppercase(),
                    _ => byte,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(replacement: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        expand(
            &parse(replacement),
            b"aB",
            &[Some((0, 2)), Some((0, 1)), None],
            &mut out,
        );
        out
    }
    #[test]
    fn whole_match() {
        assert_eq!(render(b"<&>"), b"<aB>");
    }
    #[test]
    fn capture() {
        assert_eq!(render(br"\1"), b"a");
    }
    #[test]
    fn unmatched_capture() {
        assert_eq!(render(br"x\2y"), b"xy");
    }
    #[test]
    fn upper() {
        assert_eq!(render(br"\U&\E!"), b"AB!");
    }
    #[test]
    fn lower() {
        assert_eq!(render(br"\L&"), b"ab");
    }
    #[test]
    fn once() {
        assert_eq!(render(br"\u&"), b"AB");
    }
    #[test]
    fn pending_once() {
        assert_eq!(render(br"\u\2&"), b"AB");
    }
    #[test]
    fn escaped_literal() {
        assert_eq!(render(br"\&\n\t\\"), b"&\n\t\\");
    }
}
