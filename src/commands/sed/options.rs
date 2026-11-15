use crate::common::{
    args::{self, Item, OptSpec},
    error,
};
use std::fs;

pub const HELP: &str = "Usage: sed [OPTION]... {script-only-if-no-other-script} [input-file]...\nEdit a stream of bytes according to SCRIPT.\n  -n, --quiet, --silent          suppress automatic printing\n  -e, --expression=SCRIPT        add SCRIPT to the program\n  -f, --file=FILE                read a program from FILE\n  -E, -r, --regexp-extended      use extended regular expressions\n  -i[SUFFIX], --in-place[=SUFFIX] edit each file atomically; optionally back up\n  -s, --separate                 treat input files separately\n  -z, --null-data                separate records with NUL\n  -u, --unbuffered               flush output after each command\n  -l N, --line-length=N          wrap the l command at N columns\n      --posix                   accept POSIX compatibility mode\n      --follow-symlinks         edit the target with --in-place\n      --sandbox                 prohibit execution (always prohibited here)\n      --help                    display this help\n      --version                 display version\nCR bytes are ordinary data: s/x$/y/ does not match x followed by CRLF.\nPattern backreferences and command execution are unsupported.\nExit status: 0 success, 1 syntax/usage, 2 missing input, 4 I/O error.\n";

const SPECS: &[OptSpec] = &[
    OptSpec::flag("quiet", 'n'),
    OptSpec::flag("silent", '\0'),
    OptSpec::value("expression", 'e'),
    OptSpec::value("file", 'f'),
    OptSpec::flag("regexp-extended", 'E'),
    OptSpec {
        id: "regexp-extended",
        short: 'r',
        long: "",
        has_arg: args::HasArg::No,
    },
    OptSpec::optional("in-place", 'i'),
    OptSpec::flag("separate", 's'),
    OptSpec::flag("null-data", 'z'),
    OptSpec::flag("unbuffered", 'u'),
    OptSpec::value("line-length", 'l'),
    OptSpec::flag("posix", '\0'),
    OptSpec::flag("follow-symlinks", '\0'),
    OptSpec::flag("sandbox", '\0'),
    OptSpec::flag("debug", '\0'),
];

#[derive(Clone, Debug)]
pub struct Source {
    pub name: String,
    pub bytes: Vec<u8>,
}
#[derive(Default)]
pub struct Options {
    pub quiet: bool,
    pub extended: bool,
    pub separate: bool,
    pub null: bool,
    pub unbuffered: bool,
    pub line_length: usize,
    pub in_place: Option<String>,
    pub follow: bool,
    pub sources: Vec<Source>,
    pub files: Vec<String>,
}

pub fn parse(argv: &[String]) -> Result<Options, i32> {
    let items = args::parse_command("sed", argv, HELP, SPECS, true)?;
    let mut opt = Options {
        line_length: 70,
        ..Options::default()
    };
    let mut operands = Vec::new();
    let mut expression = 0;
    for item in items {
        match item {
            Item::Operand(path) => operands.push(path),
            Item::Opt(id, value) => match id {
                "quiet" | "silent" => opt.quiet = true,
                "regexp-extended" => opt.extended = true,
                "separate" => opt.separate = true,
                "null-data" => opt.null = true,
                "unbuffered" => opt.unbuffered = true,
                "follow-symlinks" => opt.follow = true,
                "in-place" => {
                    opt.in_place = Some(value.unwrap_or_default());
                    opt.separate = true;
                }
                "line-length" => {
                    opt.line_length = value
                        .as_deref()
                        .unwrap_or("")
                        .parse()
                        .map_err(|_| error::usage("sed", "invalid line length", 1))?
                }
                "expression" => {
                    expression += 1;
                    opt.sources.push(Source {
                        name: format!("-e expression #{expression}"),
                        bytes: value.unwrap_or_default().into_bytes(),
                    });
                }
                "file" => {
                    let name = value.unwrap_or_default();
                    let bytes = read_script(&name)?;
                    opt.sources.push(Source {
                        name: format!("file {name}"),
                        bytes,
                    });
                }
                "debug" => {
                    return Err(error::usage(
                        "sed",
                        "option '--debug' is not supported by agentcmd",
                        1,
                    ))
                }
                _ => {}
            },
        }
    }
    if opt.sources.is_empty() {
        if operands.is_empty() {
            return Err(error::usage("sed", "no script specified", 1));
        }
        opt.sources.push(Source {
            name: "-e expression #1".into(),
            bytes: operands.remove(0).into_bytes(),
        });
    }
    opt.files = operands;
    Ok(opt)
}

fn read_script(name: &str) -> Result<Vec<u8>, i32> {
    use std::io::Read;
    let result = if matches!(name, "-" | "/dev/stdin") {
        let mut data = Vec::new();
        std::io::stdin().read_to_end(&mut data).map(|_| data)
    } else {
        fs::read(name)
    };
    result.map_err(|e| {
        eprintln!("sed: couldn't open file {name}: {}", error::io_text(&e));
        4
    })
}
