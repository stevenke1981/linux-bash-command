use crate::common::{
    args::{parse_command, Item, OptSpec},
    error::usage,
    io::output,
};

fn normalized(name: &str) -> String {
    if cfg!(windows) {
        name.replace('\\', "/")
    } else {
        name.to_owned()
    }
}

fn base(name: &str, suffix: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let name = normalized(name);
    if cfg!(windows) && name.len() >= 2 && name.as_bytes()[1] == b':' {
        if name[2..].bytes().all(|b| b == b'/') {
            return if name.len() > 2 {
                format!("{}/", &name[..2])
            } else {
                name
            };
        }
        if !name[2..].contains('/') {
            return strip(&name[2..], suffix).to_owned();
        }
    }
    let name = name.trim_end_matches('/');
    if name.is_empty() {
        return "/".into();
    }
    strip(name.rsplit('/').next().unwrap_or(""), suffix).to_owned()
}

fn strip<'a>(name: &'a str, suffix: &str) -> &'a str {
    if name != suffix {
        name.strip_suffix(suffix).unwrap_or(name)
    } else {
        name
    }
}

fn dir(name: &str) -> String {
    let name = normalized(name);
    if !name.is_empty() && name.bytes().all(|b| b == b'/') {
        return "/".into();
    }
    if cfg!(windows) && name.len() >= 2 && name.as_bytes()[1] == b':' {
        if name[2..].bytes().all(|b| b == b'/') {
            return if name.len() > 2 {
                format!("{}/", &name[..2])
            } else {
                name
            };
        }
        if !name[2..].contains('/') {
            return name[..2].into();
        }
    }
    let name = name.trim_end_matches('/');
    match name.rfind('/') {
        Some(index) => {
            let parent = name[..index].trim_end_matches('/');
            if parent.is_empty() {
                "/".into()
            } else {
                parent.into()
            }
        }
        None => ".".into(),
    }
}

fn run(args: &[String], basename: bool) -> i32 {
    let command = if basename { "basename" } else { "dirname" };
    let help = if basename {
        "Usage: basename NAME [SUFFIX]\n  or: basename OPTION... NAME...\nPrint NAME with leading directory components removed.\n\n  -a, --multiple       support multiple arguments\n  -s, --suffix=SUFFIX  remove SUFFIX; implies --multiple\n  -z, --zero          end each output with NUL\n  --help             display this help\n  --version          output version information\n"
    } else {
        "Usage: dirname [OPTION] NAME...\nPrint NAME with its last component removed.\n\n  -z, --zero  end each output with NUL\n  --help     display this help\n  --version  output version information\n"
    };
    let specs = if basename {
        vec![
            OptSpec::flag("multiple", 'a'),
            OptSpec::value("suffix", 's'),
            OptSpec::flag("zero", 'z'),
        ]
    } else {
        vec![OptSpec::flag("zero", 'z')]
    };
    let items = match parse_command(command, args, help, &specs, true) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let (mut multiple, mut suffix, mut delimiter) = (false, String::new(), b'\n');
    let mut names = Vec::new();
    for item in items {
        match item {
            Item::Opt("multiple", _) => multiple = true,
            Item::Opt("suffix", Some(value)) => {
                suffix = value;
                multiple = true;
            }
            Item::Opt("zero", _) => delimiter = 0,
            Item::Operand(name) => names.push(name),
            _ => unreachable!(),
        }
    }
    if names.is_empty() {
        return usage(command, "missing operand", 1);
    }
    if basename && !multiple {
        if names.len() > 2 {
            return usage(command, format!("extra operand '{}'", names[2]), 1);
        }
        if names.len() == 2 {
            suffix = names.pop().unwrap();
        }
    }
    output(command, |out| {
        for name in names {
            let text = if basename {
                if name.is_empty() {
                    String::new()
                } else {
                    base(&name, &suffix)
                }
            } else if name.bytes().all(|b| b == b'/') && !name.is_empty() {
                "/".into()
            } else {
                dir(&name)
            };
            out.write_all(text.as_bytes())?;
            out.write_all(&[delimiter])?;
        }
        Ok(())
    })
}

pub fn run_basename(args: &[String]) -> i32 {
    run(args, true)
}
pub fn run_dirname(args: &[String]) -> i32 {
    run(args, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basename_simple() {
        assert_eq!(base("a/b", ""), "b");
    }
    #[test]
    fn basename_suffix() {
        assert_eq!(base("a/b.rs", ".rs"), "b");
    }
    #[test]
    fn basename_equal_suffix() {
        assert_eq!(base("a/b", "b"), "b");
    }
    #[test]
    fn basename_root() {
        assert_eq!(base("///", ""), "/");
    }
    #[test]
    fn basename_trailing() {
        assert_eq!(base("a/b///", ""), "b");
    }
    #[test]
    fn dirname_simple() {
        assert_eq!(dir("a/b"), "a");
    }
    #[test]
    fn dirname_leaf() {
        assert_eq!(dir("leaf"), ".");
    }
    #[test]
    fn dirname_absolute() {
        assert_eq!(dir("/leaf"), "/");
    }
    #[test]
    fn dirname_trailing() {
        assert_eq!(dir("a/b///"), "a");
    }
    #[test]
    fn dirname_empty() {
        assert_eq!(dir(""), ".");
    }
}
