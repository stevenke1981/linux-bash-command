//! File modes for `chmod` and `mkdir -m`: octal (`644`) and symbolic (`u+x,go-w`) forms.
//!
//! Symbolic clauses follow GNU `mode_adjust`: a clause without `who` letters applies to
//! every class except the bits set in the umask, and `X` grants execute only to
//! directories or to files that already have an execute bit.

/// A parsed mode argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModeSpec {
    /// An absolute mode such as `0644`; replaces all twelve permission bits.
    Absolute(u32),
    /// One or more symbolic clauses, applied in order.
    Clauses(Vec<Clause>),
}

/// The operator of a symbolic clause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Remove,
    Set,
}

/// The permission letters or the class copied from (`u=g`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `r w x X s t` as a bit set, with the flags for `X` (execute if directory or already
    /// executable) kept separately.
    Perms { bits: u32, conditional_exec: bool },
    /// Copy the bits of one class: `1` user, `2` group, `4` other.
    Class(u32),
}

/// One symbolic operation, such as `g+rw` or `o=u`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clause {
    /// Class bits: `1` user, `2` group, `4` other. `0` means no `who` letters were given.
    pub classes: u32,
    pub op: Op,
    pub source: Source,
}

const USER: u32 = 1;
const GROUP: u32 = 2;
const OTHER: u32 = 4;

/// Parses a mode argument. The error carries no text; callers add their own wording.
pub fn parse(text: &str) -> Option<ModeSpec> {
    if !text.is_empty() && text.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        let value = u32::from_str_radix(text, 8).ok()?;
        return (value <= 0o7777).then_some(ModeSpec::Absolute(value));
    }
    parse_symbolic(text).ok().map(ModeSpec::Clauses)
}

fn parse_symbolic(text: &str) -> Result<Vec<Clause>, ()> {
    let mut clauses = Vec::new();
    for part in text.split(',') {
        clauses.extend(parse_part(part)?);
    }
    Ok(clauses)
}

/// Parses one comma-separated part: who letters, then one or more `op perms` groups.
fn parse_part(part: &str) -> Result<Vec<Clause>, ()> {
    let chars: Vec<char> = part.chars().collect();
    let mut index = 0;
    let mut classes = 0;
    while index < chars.len() && matches!(chars[index], 'u' | 'g' | 'o' | 'a') {
        classes |= match chars[index] {
            'u' => USER,
            'g' => GROUP,
            'o' => OTHER,
            _ => USER | GROUP | OTHER,
        };
        index += 1;
    }
    let mut out = Vec::new();
    while index < chars.len() && matches!(chars[index], '+' | '-' | '=') {
        let op = match chars[index] {
            '+' => Op::Add,
            '-' => Op::Remove,
            _ => Op::Set,
        };
        index += 1;
        let source = if index < chars.len() && matches!(chars[index], 'u' | 'g' | 'o') {
            let class = match chars[index] {
                'u' => USER,
                'g' => GROUP,
                _ => OTHER,
            };
            index += 1;
            Source::Class(class)
        } else {
            let mut bits = 0;
            let mut conditional_exec = false;
            while index < chars.len() && matches!(chars[index], 'r' | 'w' | 'x' | 'X' | 's' | 't') {
                match chars[index] {
                    'r' => bits |= 0o444,
                    'w' => bits |= 0o222,
                    'x' => bits |= 0o111,
                    'X' => conditional_exec = true,
                    's' => bits |= 0o6000,
                    _ => bits |= 0o1000,
                }
                index += 1;
            }
            Source::Perms {
                bits,
                conditional_exec,
            }
        };
        out.push(Clause {
            classes,
            op,
            source,
        });
    }
    if out.is_empty() || index != chars.len() {
        return Err(());
    }
    Ok(out)
}

/// Applies a parsed mode to `current`. `is_dir` decides `X`; `umask` limits who-less clauses.
pub fn apply(spec: &ModeSpec, current: u32, is_dir: bool, umask: u32) -> u32 {
    match spec {
        ModeSpec::Absolute(value) => *value,
        ModeSpec::Clauses(clauses) => clauses.iter().fold(current, |mode, clause| {
            apply_clause(clause, mode, is_dir, umask)
        }),
    }
}

fn apply_clause(clause: &Clause, mode: u32, is_dir: bool, umask: u32) -> u32 {
    let who = if clause.classes == 0 {
        0o7777 & !umask
    } else {
        class_mask(clause.classes)
    };
    let bits = match clause.source {
        Source::Class(class) => spread(class_bits(mode, class)) & who,
        Source::Perms {
            bits,
            conditional_exec,
        } => {
            let mut bits = bits;
            if conditional_exec && (is_dir || mode & 0o111 != 0) {
                bits |= 0o111;
            }
            bits & who
        }
    };
    match clause.op {
        Op::Add => mode | bits,
        Op::Remove => mode & !bits,
        Op::Set => (mode & !who) | bits,
    }
}

/// Permission and special bits covered by a set of classes.
fn class_mask(classes: u32) -> u32 {
    let mut mask = 0;
    if classes & USER != 0 {
        mask |= 0o4700;
    }
    if classes & GROUP != 0 {
        mask |= 0o2070;
    }
    if classes & OTHER != 0 {
        mask |= 0o1007;
    }
    mask
}

/// The rwx triple of one class, moved to the low three bits.
fn class_bits(mode: u32, class: u32) -> u32 {
    match class {
        USER => (mode >> 6) & 7,
        GROUP => (mode >> 3) & 7,
        _ => mode & 7,
    }
}

/// Copies a three-bit rwx triple into all three classes.
fn spread(triple: u32) -> u32 {
    triple * 0o111
}

/// Renders the `(rwxr-xr-x)` form GNU prints with `-v`.
pub fn symbolic_text(mode: u32) -> String {
    let mut out = String::with_capacity(9);
    let specials = [(0o4000, 's', 'S'), (0o2000, 's', 'S'), (0o1000, 't', 'T')];
    for (index, shift) in [6u32, 3, 0].iter().enumerate() {
        let triple = (mode >> shift) & 7;
        out.push(if triple & 4 != 0 { 'r' } else { '-' });
        out.push(if triple & 2 != 0 { 'w' } else { '-' });
        let (special_bit, lower, upper) = specials[index];
        let exec = triple & 1 != 0;
        out.push(match (mode & special_bit != 0, exec) {
            (true, true) => lower,
            (true, false) => upper,
            (false, true) => 'x',
            (false, false) => '-',
        });
    }
    format!("({out})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_str(text: &str, current: u32, is_dir: bool, umask: u32) -> u32 {
        apply(&parse(text).expect("valid mode"), current, is_dir, umask)
    }

    #[test]
    fn octal_modes_are_absolute() {
        assert_eq!(parse("644"), Some(ModeSpec::Absolute(0o644)));
        assert_eq!(parse("7777"), Some(ModeSpec::Absolute(0o7777)));
        assert_eq!(apply_str("0700", 0o777, false, 0o022), 0o700);
    }

    #[test]
    fn invalid_octal_and_symbolic_modes_are_rejected() {
        for text in ["", "9", "10000", "u=q", "u", "u+x,", "abc", "ug"] {
            assert_eq!(parse(text), None, "{text}");
        }
    }

    #[test]
    fn who_less_clauses_respect_umask() {
        assert_eq!(apply_str("+x", 0o644, false, 0o022), 0o755);
        assert_eq!(apply_str("+w", 0o644, false, 0o022), 0o644);
        assert_eq!(apply_str("=x", 0o644, false, 0o022), 0o111);
    }

    #[test]
    fn explicit_classes_ignore_umask() {
        assert_eq!(apply_str("a+w", 0o644, false, 0o022), 0o666);
        assert_eq!(apply_str("u+x", 0o644, false, 0o022), 0o744);
        assert_eq!(apply_str("go-rwx", 0o777, false, 0o022), 0o700);
    }

    #[test]
    fn copy_form_takes_bits_of_another_class() {
        assert_eq!(apply_str("u=g", 0o640, false, 0o022), 0o440);
        assert_eq!(apply_str("o=u", 0o700, false, 0o022), 0o707);
    }

    #[test]
    fn conditional_execute_applies_to_dirs_or_executables() {
        assert_eq!(apply_str("a+X", 0o600, true, 0o022), 0o711);
        assert_eq!(apply_str("a+X", 0o600, false, 0o022), 0o600);
        assert_eq!(apply_str("a+X", 0o700, false, 0o022), 0o711);
    }

    #[test]
    fn special_bits_follow_their_classes() {
        assert_eq!(apply_str("g+s", 0o644, false, 0o022), 0o2644);
        assert_eq!(apply_str("u+s", 0o644, false, 0o022), 0o4644);
        assert_eq!(apply_str("+t", 0o755, false, 0o022), 0o1755);
    }

    #[test]
    fn multiple_clauses_apply_in_order() {
        assert_eq!(apply_str("u+x,g=r,o-w", 0o622, false, 0o022), 0o740);
        assert_eq!(apply_str("u=rw+x", 0o000, false, 0o022), 0o700);
    }

    #[test]
    fn symbolic_text_matches_ls_style() {
        assert_eq!(symbolic_text(0o644), "(rw-r--r--)");
        assert_eq!(symbolic_text(0o4755), "(rwsr-xr-x)");
        assert_eq!(symbolic_text(0o1600), "(rw------T)");
    }
}
