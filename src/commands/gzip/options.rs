use crate::common::args::{self, Item, OptSpec};

pub const HELP: &str = "Usage: gzip [OPTION]... [FILE]...\nCompress or decompress files.\n  -c, --stdout           write to standard output\n  -d, --decompress       decompress\n  -f, --force            overwrite existing output\n  -k, --keep             keep input files\n  -l, --list             list compressed file sizes\n  -n, --no-name          omit or ignore original name and timestamp\n  -N, --name             store or restore original name and timestamp\n  -q, --quiet            suppress warnings\n  -r, --recursive        process directories recursively\n  -S, --suffix=SUF       use SUF as the compressed suffix\n  -t, --test             test compressed data\n  -v, --verbose          display processing details\n  -1..-9                 compression level\n      --fast             fastest compression\n      --best             best compression\n";
#[derive(Clone)]
pub struct Options {
    pub command: &'static str,
    pub decompress: bool,
    pub stdout: bool,
    pub force: bool,
    pub keep: bool,
    pub list: bool,
    pub no_name: bool,
    pub restore_name: bool,
    pub quiet: bool,
    pub recursive: bool,
    pub suffix: String,
    pub test: bool,
    pub verbose: bool,
    pub level: u32,
    pub files: Vec<String>,
}
fn specs() -> Vec<OptSpec> {
    let mut options = vec![
        OptSpec::flag("stdout", 'c'),
        OptSpec::flag("decompress", 'd'),
        OptSpec::flag("force", 'f'),
        OptSpec::flag("keep", 'k'),
        OptSpec::flag("list", 'l'),
        OptSpec::flag("no-name", 'n'),
        OptSpec::flag("name", 'N'),
        OptSpec::flag("quiet", 'q'),
        OptSpec::flag("recursive", 'r'),
        OptSpec::value("suffix", 'S'),
        OptSpec::flag("test", 't'),
        OptSpec::flag("verbose", 'v'),
        OptSpec::flag("fast", '\0'),
        OptSpec::flag("best", '\0'),
    ];
    for (name, digit) in [
        ("level-1", '1'),
        ("level-2", '2'),
        ("level-3", '3'),
        ("level-4", '4'),
        ("level-5", '5'),
        ("level-6", '6'),
        ("level-7", '7'),
        ("level-8", '8'),
        ("level-9", '9'),
    ] {
        options.push(OptSpec {
            id: name,
            short: digit,
            long: "",
            has_arg: args::HasArg::No,
        });
    }
    options
}
pub fn parse(command: &'static str, argv: &[String]) -> Result<Options, i32> {
    let help = HELP.replacen("Usage: gzip", &format!("Usage: {command}"), 1);
    let items = args::parse_command(command, argv, &help, &specs(), true)?;
    let mut o = Options {
        command,
        decompress: command != "gzip",
        stdout: command == "zcat",
        force: false,
        keep: false,
        list: false,
        no_name: false,
        restore_name: false,
        quiet: false,
        recursive: false,
        suffix: ".gz".into(),
        test: false,
        verbose: false,
        level: 6,
        files: Vec::new(),
    };
    for item in items {
        apply(&mut o, item)?;
    }
    if o.suffix.is_empty() || o.suffix.len() > 30 || o.suffix.contains(['/', '\\']) {
        return Err(crate::common::error::usage(command, "invalid suffix", 1));
    }
    Ok(o)
}
fn apply(o: &mut Options, item: Item) -> Result<(), i32> {
    match item {
        Item::Operand(v) => o.files.push(v),
        Item::Opt("stdout", _) => o.stdout = true,
        Item::Opt("decompress", _) => o.decompress = true,
        Item::Opt("force", _) => o.force = true,
        Item::Opt("keep", _) => o.keep = true,
        Item::Opt("list", _) => {
            o.list = true;
            o.decompress = true;
        }
        Item::Opt("no-name", _) => {
            o.no_name = true;
            o.restore_name = false;
        }
        Item::Opt("name", _) => {
            o.no_name = false;
            o.restore_name = true;
        }
        Item::Opt("quiet", _) => {
            o.quiet = true;
            o.verbose = false;
        }
        Item::Opt("recursive", _) => o.recursive = true,
        Item::Opt("suffix", Some(v)) => o.suffix = v,
        Item::Opt("test", _) => {
            o.test = true;
            o.decompress = true;
        }
        Item::Opt("verbose", _) => {
            o.verbose = true;
            o.quiet = false;
        }
        Item::Opt("fast", _) => o.level = 1,
        Item::Opt("best", _) => o.level = 9,
        Item::Opt(id, _) if id.starts_with("level-") => o.level = id[6..].parse().unwrap(),
        _ => return Err(crate::common::error::usage(o.command, "invalid option", 1)),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_aliases() {
        assert!(!parse("gzip", &[]).unwrap().decompress);
        assert!(parse("gunzip", &[]).unwrap().decompress);
        assert!(parse("zcat", &[]).unwrap().stdout);
    }
    #[test]
    fn options_and_order() {
        let o = parse(
            "gzip",
            &["-9ck".into(), "-n".into(), "-N".into(), "a".into()],
        )
        .unwrap();
        assert_eq!(o.level, 9);
        assert!(o.stdout && o.keep && o.restore_name && !o.no_name);
    }
    #[test]
    fn bad_suffix() {
        assert!(parse("gzip", &["-S".into(), "a/b".into()]).is_err());
    }
}
