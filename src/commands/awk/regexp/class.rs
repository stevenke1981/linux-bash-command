pub(super) fn normalize(source: &str) -> Result<String, String> {
    let bytes = source.as_bytes();
    let (mut out, mut at) = (Vec::new(), 0);
    while at < bytes.len() {
        let byte = bytes[at];
        at += 1;
        if byte == b'{' && bytes.get(at) == Some(&b'}') {
            return Err("invalid interval expression".into());
        }
        if byte == b'{' && !bytes.get(at).is_some_and(u8::is_ascii_digit) {
            out.extend_from_slice(br"\{");
            continue;
        }
        out.push(byte);
        if byte == b'\\' && at < bytes.len() {
            out.push(bytes[at]);
            at += 1;
        } else if byte == b'[' {
            class(bytes, &mut at, &mut out)?;
        }
    }
    Ok(String::from_utf8(out).unwrap())
}
fn class(bytes: &[u8], at: &mut usize, out: &mut Vec<u8>) -> Result<(), String> {
    if bytes.get(*at) == Some(&b'^') {
        out.push(b'^');
        *at += 1;
    }
    if bytes.get(*at) == Some(&b']') {
        out.extend_from_slice(br"\]");
        *at += 1;
    }
    while let Some(&byte) = bytes.get(*at) {
        *at += 1;
        match byte {
            b']' => {
                out.push(byte);
                return Ok(());
            }
            b'\\' => {
                out.push(byte);
                if let Some(&next) = bytes.get(*at) {
                    out.push(next);
                    *at += 1;
                }
            }
            b'[' if matches!(bytes.get(*at), Some(b':' | b'.' | b'=')) => {
                let mark = bytes[*at];
                *at += 1;
                let start = *at;
                while *at + 1 < bytes.len() && bytes[*at..*at + 2] != [mark, b']'] {
                    *at += 1;
                }
                if *at + 1 >= bytes.len() {
                    return Err("invalid bracket expression".into());
                }
                let content = &bytes[start..*at];
                *at += 2;
                if mark == b':' {
                    out.extend_from_slice(b"[:");
                    out.extend_from_slice(content);
                    out.extend_from_slice(b":]");
                } else if content.len() == 1 {
                    out.extend_from_slice(
                        regex::escape(std::str::from_utf8(content).unwrap()).as_bytes(),
                    );
                } else {
                    return Err("invalid collating element".into());
                }
            }
            b'[' | b'&' | b'~' => {
                out.push(b'\\');
                out.push(byte);
            }
            _ => out.push(byte),
        }
    }
    Err("invalid bracket expression".into())
}
