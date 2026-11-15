use crate::common::args::{self, Item, OptSpec};
use std::path::PathBuf;
pub(super) const HELP:&str="Usage: patch [OPTION]... [ORIGINAL [PATCHFILE]]\nApply unified patches, including textual git diffs.\n  -p, --strip=N                strip N leading path components\n  -i, --input=FILE             read a patch from FILE\n  -d, --directory=DIR          operate in DIR\n  -R, --reverse                reverse old and new contents\n  -N, --forward                skip already-applied patches\n      --dry-run                check without writing files\n  -o, --output=FILE            write concatenated results to FILE\n  -b, --backup                 retain originals in .orig files\n  -E, --remove-empty-files     remove empty patched files\n  -s, --silent, --quiet        suppress progress messages\n  -f, --force                  do not prompt\n  -t, --batch                  skip reversed or applied patches\n  -F, --fuzz=N                 ignore at most N context lines (default 2)\n  -l, --ignore-whitespace      compare runs of blanks loosely\n      --binary                 accept binary I/O mode\n  -r, --reject-file=FILE       write rejected hunks to FILE\n      --no-backup-if-mismatch  disable automatic mismatch backups\n  -u                           accept unified format\n      --help                   display help\n      --version                output version\nContext, normal, rename and binary patches are unsupported.\nExit status: 0 success, 1 failed hunks, 2 fatal error.\n";
pub(super) struct Options {
    pub strip: Option<usize>,
    pub input: String,
    pub directory: PathBuf,
    pub reverse: bool,
    pub forward: bool,
    pub force: bool,
    pub dry: bool,
    pub output: Option<PathBuf>,
    pub backup: bool,
    pub remove: bool,
    pub quiet: bool,
    pub batch: bool,
    pub fuzz: usize,
    pub loose: bool,
    pub reject: Option<PathBuf>,
    pub no_mismatch_backup: bool,
    pub original: Option<String>,
}
pub(super) fn parse(arguments: &[String]) -> Result<(Options, Option<String>), String> {
    let specs = [
        OptSpec::value("strip", 'p'),
        OptSpec::value("input", 'i'),
        OptSpec::value("directory", 'd'),
        OptSpec::flag("reverse", 'R'),
        OptSpec::flag("forward", 'N'),
        OptSpec::flag("dry-run", '\0'),
        OptSpec::value("output", 'o'),
        OptSpec::flag("backup", 'b'),
        OptSpec::flag("remove-empty-files", 'E'),
        OptSpec::flag("silent", 's'),
        OptSpec {
            id: "silent",
            long: "quiet",
            short: '\0',
            has_arg: args::HasArg::No,
        },
        OptSpec::flag("force", 'f'),
        OptSpec::flag("batch", 't'),
        OptSpec::value("fuzz", 'F'),
        OptSpec::flag("ignore-whitespace", 'l'),
        OptSpec::flag("binary", '\0'),
        OptSpec::value("reject-file", 'r'),
        OptSpec::flag("no-backup-if-mismatch", '\0'),
        OptSpec {
            id: "unified",
            long: "",
            short: 'u',
            has_arg: args::HasArg::No,
        },
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let mut o = Options {
        strip: None,
        input: "-".into(),
        directory: ".".into(),
        reverse: false,
        forward: false,
        force: false,
        dry: false,
        output: None,
        backup: false,
        remove: false,
        quiet: false,
        batch: false,
        fuzz: 2,
        loose: false,
        reject: None,
        no_mismatch_backup: false,
        original: None,
    };
    let mut operands = vec![];
    for item in args::parse(arguments, &specs, true)? {
        match item {
            Item::Operand(s) => operands.push(s),
            Item::Opt(id, v) => match id {
                "strip" => o.strip = Some(number(&v.unwrap())?),
                "input" => o.input = v.unwrap(),
                "directory" => o.directory = v.unwrap().into(),
                "reverse" => o.reverse = true,
                "forward" => o.forward = true,
                "dry-run" => o.dry = true,
                "output" => o.output = Some(v.unwrap().into()),
                "backup" => o.backup = true,
                "remove-empty-files" => o.remove = true,
                "silent" => o.quiet = true,
                "batch" => o.batch = true,
                "fuzz" => o.fuzz = number(&v.unwrap())?,
                "ignore-whitespace" => o.loose = true,
                "reject-file" => o.reject = Some(v.unwrap().into()),
                "no-backup-if-mismatch" => o.no_mismatch_backup = true,
                "force" => o.force = true,
                "binary" | "unified" => {}
                "help" | "version" => return Ok((o, Some(format!("--{id}")))),
                _ => unreachable!(),
            },
        }
    }
    if operands.len() > 2 {
        return Err("extra operand".into());
    }
    o.original = operands.first().cloned();
    if let Some(input) = operands.get(1) {
        if o.input != "-" {
            return Err("patch input specified twice".into());
        }
        o.input = input.clone();
    }
    Ok((o, None))
}
fn number(s: &str) -> Result<usize, String> {
    s.parse().map_err(|_| format!("invalid number '{s}'"))
}
