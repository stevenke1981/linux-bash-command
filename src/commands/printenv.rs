use crate::common::{
    args::{self, Item, OptSpec},
    error, io,
};
const HELP: &str = "Usage: printenv [OPTION]... [VARIABLE]...\nPrint environment variables.\n  -0, --null     End each value with NUL\n      --help     Display this help\n      --version  Display version\nExit status: 1 if any VARIABLE is absent, otherwise 0.\n";

pub fn run(arguments: &[String]) -> i32 {
    let items = match args::parse(
        arguments,
        &[
            OptSpec::flag("null", '0'),
            OptSpec::flag("help", '\0'),
            OptSpec::flag("version", '\0'),
        ],
        true,
    ) {
        Ok(items) => items,
        Err(e) => return error::usage("printenv", e, 1),
    };
    let mut zero = false;
    let mut names = Vec::new();
    for item in items {
        match item {
            Item::Opt("null", _) => zero = true,
            Item::Opt(id @ ("help" | "version"), _) => {
                return args::help_version("printenv", &[format!("--{id}")], HELP, true).unwrap()
            }
            Item::Operand(s) => names.push(s),
            _ => unreachable!(),
        }
    }
    let (values, missing) = values(&names);
    let code = io::output("printenv", |out| {
        for value in &values {
            out.write_all(value)?;
            out.write_all(if zero { b"\0" } else { b"\n" })?;
        }
        Ok(())
    });
    if code == 0 {
        i32::from(missing)
    } else {
        code
    }
}

fn values(names: &[String]) -> (Vec<Vec<u8>>, bool) {
    if names.is_empty() {
        return (
            std::env::vars_os()
                .map(|(k, v)| {
                    let mut bytes = k.into_encoded_bytes();
                    bytes.push(b'=');
                    bytes.extend_from_slice(v.as_encoded_bytes());
                    bytes
                })
                .collect(),
            false,
        );
    }
    let mut missing = false;
    let values = names
        .iter()
        .filter_map(|n| {
            if n.contains('=') {
                missing = true;
                return None;
            }
            match std::env::var_os(n) {
                Some(v) => Some(v.as_encoded_bytes().to_vec()),
                None => {
                    missing = true;
                    None
                }
            }
        })
        .collect();
    (values, missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all() {
        assert!(!values(&[]).1);
    }
    #[test]
    fn missing() {
        assert!(values(&["AGENTCMD_P1_MISSING_ENV_XX".into()]).1);
    }
    #[test]
    fn invalid_name() {
        assert!(values(&["A=B".into()]).1);
    }
    #[test]
    fn empty_name() {
        assert!(values(&[String::new()]).1);
    }
    #[test]
    fn duplicated() {
        // Windows may expose drive-state entries such as '=C:'; these are not
        // valid printenv operands.
        let name = std::env::vars()
            .find(|(name, _)| !name.contains('='))
            .unwrap()
            .0;
        let result = values(&[name.clone(), name]);
        assert_eq!(result.0.len(), 2);
        assert_eq!(result.0[0], result.0[1]);
    }
}
