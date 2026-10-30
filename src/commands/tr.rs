use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
use std::io::Read;
const HELP: &str = "Usage: tr [OPTION]... STRING1 [STRING2]\nTranslate, squeeze, or delete bytes from standard input.\n  -c, -C, --complement     complement STRING1\n  -d, --delete             delete STRING1\n  -s, --squeeze-repeats    squeeze last set\n  -t, --truncate-set1      truncate STRING1 to STRING2 length\n      --help               display help\n      --version            display version\n";
const SPECS: &[OptSpec] = &[
    OptSpec::flag("complement", 'c'),
    OptSpec {
        id: "complement",
        short: 'C',
        long: "",
        has_arg: args::HasArg::No,
    },
    OptSpec::flag("delete", 'd'),
    OptSpec::flag("squeeze-repeats", 's'),
    OptSpec::flag("truncate-set1", 't'),
    OptSpec::flag("help", '\0'),
    OptSpec::flag("version", '\0'),
];

#[derive(Debug)]
enum Atom {
    Byte(u8),
    Class(String, Vec<u8>),
    Repeat(u8, Option<usize>),
}

fn escaped(bytes: &[u8], i: &mut usize) -> Result<u8, String> {
    let first = bytes[*i];
    *i += 1;
    if first != b'\\' || *i == bytes.len() {
        return Ok(first);
    }
    let c = bytes[*i];
    *i += 1;
    if (b'0'..=b'7').contains(&c) {
        let mut value = (c - b'0') as u16;
        for _ in 0..2 {
            if let Some(&next) = bytes.get(*i).filter(|&&b| (b'0'..=b'7').contains(&b)) {
                value = value * 8 + u16::from(next - b'0');
                *i += 1;
            } else {
                break;
            }
        }
        return Ok(value as u8);
    }
    Ok(match c {
        b'a' => 7,
        b'b' => 8,
        b'f' => 12,
        b'n' => 10,
        b'r' => 13,
        b't' => 9,
        b'v' => 11,
        _ => c,
    })
}

fn class(name: &str) -> Result<Vec<u8>, String> {
    let test: fn(u8) -> bool = match name {
        "alnum" => |b| b.is_ascii_alphanumeric(),
        "alpha" => |b| b.is_ascii_alphabetic(),
        "blank" => |b| b == b' ' || b == b'\t',
        "cntrl" => |b| b < 32 || b == 127,
        "digit" => |b| b.is_ascii_digit(),
        "graph" => |b| (33..=126).contains(&b),
        "lower" => |b| b.is_ascii_lowercase(),
        "print" => |b| (32..=126).contains(&b),
        "punct" => |b| b.is_ascii_punctuation(),
        "space" => |b| matches!(b, 9..=13 | 32),
        "upper" => |b| b.is_ascii_uppercase(),
        "xdigit" => |b| b.is_ascii_hexdigit(),
        _ => return Err(format!("invalid character class '{name}'")),
    };
    Ok((0..=255).filter(|b| test(*b)).collect())
}

fn atoms(set: &str, second: bool) -> Result<Vec<Atom>, String> {
    let bytes = set.as_bytes();
    let (mut i, mut result) = (0, Vec::new());
    while i < bytes.len() {
        if bytes[i..].starts_with(b"[:") {
            if let Some(end) = bytes[i + 2..].windows(2).position(|b| b == b":]") {
                let name = &set[i + 2..i + 2 + end];
                result.push(Atom::Class(name.to_owned(), class(name)?));
                i += end + 4;
                continue;
            }
        }
        if bytes[i..].starts_with(b"[=") {
            let mut j = i + 2;
            if j < bytes.len() {
                let c = escaped(bytes, &mut j)?;
                if bytes[j..].starts_with(b"=]") {
                    result.push(Atom::Byte(c));
                    i = j + 2;
                    continue;
                }
            }
        }
        if bytes[i] == b'[' && i + 1 < bytes.len() {
            let mut j = i + 1;
            let c = escaped(bytes, &mut j)?;
            if bytes.get(j) == Some(&b'*') {
                let start = j + 1;
                if let Some(end) = bytes[start..].iter().position(|&b| b == b']') {
                    let digits = &set[start..start + end];
                    let count = if digits.is_empty() {
                        None
                    } else {
                        Some(
                            usize::from_str_radix(
                                digits,
                                if digits.starts_with('0') { 8 } else { 10 },
                            )
                            .map_err(|_| format!("invalid repeat count '{digits}'"))?,
                        )
                    };
                    if !second && count.unwrap_or(0) == 0 {
                        return Err("the [c*] repeat construct may not appear in string1".into());
                    }
                    result.push(Atom::Repeat(c, count.filter(|n| *n != 0)));
                    i = start + end + 1;
                    continue;
                }
            }
        }
        let first = escaped(bytes, &mut i)?;
        if bytes.get(i) == Some(&b'-') && i + 1 < bytes.len() {
            i += 1;
            let last = escaped(bytes, &mut i)?;
            if first > last {
                return Err(format!(
                    "range-endpoints of '{}-{}' are in reverse collating sequence order",
                    first as char, last as char
                ));
            }
            result.extend((first..=last).map(Atom::Byte));
        } else {
            result.push(Atom::Byte(first));
        }
    }
    Ok(result)
}

fn expand(atoms: &[Atom], target: usize) -> Result<Vec<u8>, String> {
    let fixed: usize = atoms
        .iter()
        .map(|a| match a {
            Atom::Byte(_) => 1,
            Atom::Class(_, b) => b.len(),
            Atom::Repeat(_, Some(n)) => *n,
            _ => 0,
        })
        .sum();
    if atoms
        .iter()
        .filter(|a| matches!(a, Atom::Repeat(_, None)))
        .count()
        > 1
    {
        return Err("only one [c*] repeat construct may appear in string2".into());
    }
    let mut result = Vec::new();
    for a in atoms {
        match a {
            Atom::Byte(b) => result.push(*b),
            Atom::Class(_, b) => result.extend_from_slice(b),
            Atom::Repeat(b, n) => result.extend(std::iter::repeat_n(
                *b,
                n.unwrap_or_else(|| target.saturating_sub(fixed)),
            )),
        }
    }
    Ok(result)
}

struct Translation {
    map: [u8; 256],
    delete: [bool; 256],
    squeeze: [bool; 256],
}
fn translation(
    sets: &[String],
    complement: bool,
    delete: bool,
    squeeze: bool,
    truncate: bool,
) -> Result<Translation, String> {
    let min = if delete {
        1 + usize::from(squeeze)
    } else if squeeze && sets.len() == 1 {
        1
    } else {
        2
    };
    if sets.len() < min {
        return Err("missing operand".into());
    }
    if sets.len() > min {
        return Err(format!("extra operand '{}'", sets[min]));
    }
    let a = atoms(&sets[0], false)?;
    let mut first = expand(&a, 0)?;
    if complement {
        first = (0..=255).filter(|b| !first.contains(b)).collect();
    }
    let b = if sets.len() > 1 {
        atoms(&sets[1], true)?
    } else {
        Vec::new()
    };
    if !delete && !b.is_empty() {
        let mut first_offset = 0;
        let mut first_case = Vec::new();
        for atom in &a {
            match atom {
                Atom::Class(name, bytes) => {
                    if name == "lower" || name == "upper" {
                        first_case.push((first_offset, name.as_str()));
                    }
                    first_offset += bytes.len();
                }
                _ => first_offset += expand(std::slice::from_ref(atom), 0)?.len(),
            }
        }
        let mut offset = 0;
        for atom in &b {
            if let Atom::Class(name, bytes) = atom {
                if name != "lower" && name != "upper" {
                    return Err("when translating, the only character classes that may appear in string2 are 'upper' and 'lower'".into());
                }
                if !first_case.iter().any(|&(at, _)| at == offset) {
                    return Err("misaligned [:upper:] and/or [:lower:] construct".into());
                }
                offset += bytes.len();
            } else {
                offset += expand(std::slice::from_ref(atom), first.len())?.len();
            }
        }
    }
    let mut second = expand(&b, first.len())?;
    if !delete
        && !truncate
        && second.len() < first.len()
        && matches!(b.last(), Some(Atom::Class(_, _)))
    {
        return Err("when translating with string1 longer than string2, the latter string must not end with a character class".into());
    }
    let mut t = Translation {
        map: std::array::from_fn(|i| i as u8),
        delete: [false; 256],
        squeeze: [false; 256],
    };
    if delete {
        for c in &first {
            t.delete[*c as usize] = true;
        }
    }
    if !delete && sets.len() == 2 {
        if second.is_empty() && !first.is_empty() && !truncate {
            return Err("when not truncating set1, string2 must be non-empty".into());
        }
        if !truncate && second.len() < first.len() {
            second.resize(first.len(), *second.last().unwrap());
        }
        for (a, b) in first.iter().zip(&second) {
            t.map[*a as usize] = *b;
        }
    }
    if squeeze {
        for c in if sets.len() == 2 { &second } else { &first } {
            t.squeeze[*c as usize] = true;
        }
    }
    Ok(t)
}

pub fn run(argv: &[String]) -> i32 {
    let items = match args::parse(argv, SPECS, true) {
        Ok(v) => v,
        Err(e) => return error::usage("tr", &e, 1),
    };
    if let Some(id) = items.iter().find_map(|item| match item {
        Item::Opt(id @ ("help" | "version"), _) => Some(*id),
        _ => None,
    }) {
        return args::help_version("tr", &[format!("--{id}")], HELP, true).unwrap();
    }
    let (mut sets, mut complement, mut delete, mut squeeze, mut truncate) =
        (Vec::new(), false, false, false, false);
    for item in items {
        match item {
            Item::Operand(s) => sets.push(s),
            Item::Opt("complement", _) => complement = true,
            Item::Opt("delete", _) => delete = true,
            Item::Opt("squeeze-repeats", _) => squeeze = true,
            Item::Opt("truncate-set1", _) => truncate = true,
            _ => {}
        }
    }
    let t = match translation(&sets, complement, delete, squeeze, truncate) {
        Ok(t) => t,
        Err(e) => return error::usage("tr", &e, 1),
    };
    io::output("tr", |out| {
        let mut input = std::io::stdin().lock();
        let (mut block, mut result, mut previous) = (
            [0; io::BUFFER_SIZE],
            Vec::with_capacity(io::BUFFER_SIZE),
            None,
        );
        loop {
            let n = input.read(&mut block)?;
            if n == 0 {
                break;
            }
            result.clear();
            for &c in &block[..n] {
                if t.delete[c as usize] {
                    continue;
                }
                let c = t.map[c as usize];
                if previous == Some(c) && t.squeeze[c as usize] {
                    continue;
                }
                result.push(c);
                previous = Some(c);
            }
            out.write_all(&result)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(s: &str) -> Vec<u8> {
        expand(&atoms(s, true).unwrap(), 5).unwrap()
    }
    #[test]
    fn ranges() {
        assert_eq!(set("a-c"), b"abc");
        assert!(atoms("z-a", false).is_err());
    }
    #[test]
    fn classes() {
        assert_eq!(set("[:digit:]"), b"0123456789");
        assert_eq!(set("[:space:]"), b"\t\n\x0b\x0c\r ");
    }
    #[test]
    fn escaped_octal() {
        assert_eq!(set("\\141\\n\\377"), b"a\n\xff");
    }
    #[test]
    fn repeats() {
        assert_eq!(set("[x*03]y[z*]"), b"xxxyz");
    }
    #[test]
    fn equivalence() {
        assert_eq!(set("[=a=]"), b"a");
    }
    #[test]
    fn padding() {
        let t = translation(&["abc".into(), "xy".into()], false, false, false, false).unwrap();
        assert_eq!(t.map[b'c' as usize], b'y');
    }
    #[test]
    fn complement() {
        let t = translation(&["a".into()], true, true, false, false).unwrap();
        assert!(!t.delete[97]);
        assert!(t.delete[0]);
    }
    #[test]
    fn paired_classes() {
        assert!(translation(
            &["[:upper:]".into(), "[:lower:]".into()],
            false,
            false,
            false,
            false
        )
        .is_ok());
        assert!(translation(
            &["x[:upper:]".into(), "[:lower:]".into()],
            false,
            false,
            false,
            false
        )
        .is_err());
    }
}
