//! `sed`: a GNU sed 4.9 subset.
//!
//! - `parse` turns script text into a command list with resolved labels and blocks.
//! - `addr` decides which lines a command selects, `pattern` and `subst` do the regex
//!   work, `exec` runs the cycle, `input` reads lines, `inplace` edits files in place.

mod addr;
mod exec;
mod inplace;
mod input;
mod output;
mod parse;
mod pattern;
mod script;
mod subst;

use std::fs;
use std::io::{self, Read};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, unsupported, EXIT_FAIL};

use parse::{Chunk, Dialect, Origin};

const SPECS: &[OptSpec] = &[
    OptSpec::flag("quiet", Some('n'), Some("quiet")),
    OptSpec::flag("quiet", None, Some("silent")),
    OptSpec::req("expression", Some('e'), Some("expression")),
    OptSpec::req("file", Some('f'), Some("file")),
    OptSpec::flag("extended", Some('E'), None),
    OptSpec::flag("extended", Some('r'), Some("regexp-extended")),
    OptSpec::opt("in-place", Some('i'), Some("in-place")),
    OptSpec::flag("separate", Some('s'), Some("separate")),
    OptSpec::flag("null", Some('z'), Some("null-data")),
    OptSpec::flag("null", None, Some("zero-terminated")),
    OptSpec::flag("unbuffered", Some('u'), Some("unbuffered")),
    OptSpec::req("line-length", Some('l'), Some("line-length")),
    OptSpec::flag("posix", None, Some("posix")),
    OptSpec::flag("follow", None, Some("follow-symlinks")),
    OptSpec::flag("debug", None, Some("debug")),
    OptSpec::flag("sandbox", None, Some("sandbox")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "\
Usage: sed [OPTION]... {script-only-if-no-other-script} [input-file]...

  -n, --quiet, --silent
                 suppress automatic printing of pattern space
  -e script, --expression=script
                 add the script to the commands to be executed
  -f script-file, --file=script-file
                 add the contents of script-file to the commands to be executed
  --follow-symlinks
                 follow symlinks when processing in place
  -i[SUFFIX], --in-place[=SUFFIX]
                 edit files in place (makes backup if SUFFIX supplied)
  -l N, --line-length=N
                 specify the desired line-wrap length for the `l' command
  --posix
                 disable all GNU extensions.
  -E, -r, --regexp-extended
                 use extended regular expressions in the script
                 (for portability use POSIX -E).
  -s, --separate
                 consider files as separate rather than as a single
                 continuous long stream.
  --sandbox
                 operate in sandbox mode (disable e/r/w commands).
  -u, --unbuffered
                 load minimal amounts of data from the input files and flush
                 the output buffers more often
  -z, --null-data
                 separate lines by NUL characters
      --help     display this help and exit
      --version  output version information and exit

If no -e, --expression, -f, or --file option is given, then the first
non-option argument is taken as the sed script to interpret.  All
remaining arguments are names of input files; if no input files are
specified, then the standard input is read.

Not supported by agentcmd: the e command and s///e, --debug. Carriage returns
are ordinary characters, so a CRLF file needs an explicit `s/\\r$//`.

GNU sed home page: <https://www.gnu.org/software/sed/>.
General help using GNU software: <https://www.gnu.org/gethelp/>.
";

/// Parsed command line.
#[derive(Default)]
struct Cli {
    chunks: Vec<Chunk>,
    operands: Vec<String>,
    quiet: bool,
    dialect: Dialect,
    separate: bool,
    in_place: Option<String>,
    null_data: bool,
    unbuffered: bool,
    line_len: Option<usize>,
    follow: bool,
    help: bool,
    version: bool,
}

/// Entry point for the `sed` command.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("sed", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("sed", &error, EXIT_FAIL),
    };
    let cli = match collect(items) {
        Ok(cli) => cli,
        Err(status) => return status,
    };
    if cli.help {
        print!("{HELP}");
        return 0;
    }
    if cli.version {
        return print_version("sed");
    }
    let Cli {
        mut chunks,
        mut operands,
        ..
    } = cli;
    if chunks.is_empty() {
        if operands.is_empty() {
            eprint!("{HELP}");
            return EXIT_FAIL;
        }
        chunks.push(Chunk {
            text: operands.remove(0).into_bytes(),
            origin: Origin::Expr(1),
        });
    }
    let script = match parse::parse(&chunks, cli.dialect) {
        Ok(script) => script,
        Err(error) => {
            eprintln!("sed: {}", error.message);
            return error.status;
        }
    };
    if cli.in_place.is_some() && operands.is_empty() {
        eprintln!("sed: no input files");
        return 4;
    }
    if operands.is_empty() {
        operands.push("-".to_string());
    }
    let options = exec::Options {
        quiet: cli.quiet,
        separate: cli.separate,
        in_place: cli.in_place,
        null_data: cli.null_data,
        unbuffered: cli.unbuffered,
        line_len: cli.line_len.unwrap_or(70),
        follow: cli.follow,
        inputs: operands,
    };
    exec::run(&script, &options)
}

fn collect(items: Vec<Item>) -> Result<Cli, i32> {
    let mut cli = Cli::default();
    for item in items {
        match item {
            Item::Operand(operand) => cli.operands.push(operand),
            Item::Opt { id, value } => apply_option(&mut cli, id, value)?,
        }
    }
    Ok(cli)
}

fn apply_option(cli: &mut Cli, id: &str, value: Option<String>) -> Result<(), i32> {
    match id {
        "quiet" => cli.quiet = true,
        "expression" => {
            let number = cli.chunks.len() + 1;
            let text = value.unwrap_or_default().into_bytes();
            cli.chunks.push(Chunk {
                text,
                origin: Origin::Expr(number),
            });
        }
        "file" => {
            let name = value.unwrap_or_default();
            let text = read_script_file(&name)?;
            cli.chunks.push(Chunk {
                text,
                origin: Origin::File(name),
            });
        }
        "extended" => cli.dialect.ere = true,
        "in-place" => {
            cli.in_place = Some(value.unwrap_or_default());
            cli.separate = true;
        }
        "separate" => cli.separate = true,
        "null" => cli.null_data = true,
        "unbuffered" => cli.unbuffered = true,
        "line-length" => {
            // GNU reads the value with atoi: leading digits, anything else is 0.
            let text = value.unwrap_or_default();
            let digits: String = text
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            cli.line_len = Some(digits.parse().unwrap_or(0));
        }
        "posix" => {}
        "follow" => cli.follow = true,
        "sandbox" => cli.dialect.sandbox = true,
        "debug" => return Err(unsupported("sed", "--debug")),
        "help" => cli.help = true,
        "version" => cli.version = true,
        _ => {}
    }
    Ok(())
}

/// Reads a `-f` script; `-` is standard input.
fn read_script_file(name: &str) -> Result<Vec<u8>, i32> {
    let result = if name == "-" {
        let mut buffer = Vec::new();
        io::stdin().read_to_end(&mut buffer).map(|_| buffer)
    } else {
        fs::read(name)
    };
    result.map_err(|error| {
        // GNU sed reports an unreadable script file with status 4.
        eprintln!("sed: couldn't open file {name}: {}", io_error_text(&error));
        4
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(id: &'static str, value: Option<&str>) -> Item {
        Item::Opt {
            id,
            value: value.map(str::to_owned),
        }
    }

    #[test]
    fn expressions_are_numbered_in_order() {
        let cli = collect(vec![
            opt("expression", Some("p")),
            Item::Operand("in.txt".into()),
            opt("expression", Some("d")),
        ])
        .unwrap();
        assert_eq!(cli.chunks.len(), 2);
        assert_eq!(cli.chunks[1].origin, Origin::Expr(2));
        assert_eq!(cli.operands, vec!["in.txt".to_string()]);
    }

    #[test]
    fn in_place_implies_separate_and_keeps_suffix() {
        let cli = collect(vec![opt("in-place", Some(".bak"))]).unwrap();
        assert!(cli.separate);
        assert_eq!(cli.in_place.as_deref(), Some(".bak"));
        let plain = collect(vec![opt("in-place", None)]).unwrap();
        assert_eq!(plain.in_place.as_deref(), Some(""));
    }

    #[test]
    fn line_length_reads_leading_digits_like_atoi() {
        let cli = collect(vec![opt("line-length", Some("5"))]).unwrap();
        assert_eq!(cli.line_len, Some(5));
        let cli = collect(vec![opt("line-length", Some("x"))]).unwrap();
        assert_eq!(cli.line_len, Some(0));
        let cli = collect(vec![opt("line-length", Some("12abc"))]).unwrap();
        assert_eq!(cli.line_len, Some(12));
    }

    #[test]
    fn debug_is_refused_as_unsupported() {
        assert_eq!(collect(vec![opt("debug", None)]).err(), Some(1));
        let mut cli = Cli::default();
        assert_eq!(apply_option(&mut cli, "debug", None), Err(1));
    }

    #[test]
    fn script_file_missing_is_a_failure() {
        assert_eq!(read_script_file("/no/such/sed-script-file").err(), Some(4));
    }

    #[test]
    fn help_text_lists_implemented_options() {
        for name in [
            "--quiet",
            "--expression",
            "--in-place",
            "--sandbox",
            "--null-data",
        ] {
            assert!(HELP.contains(name), "{name} missing from help");
        }
        assert!(!HELP.contains("      --debug"), "unsupported option listed");
    }

    #[test]
    fn extended_and_sandbox_options_set_dialect() {
        let cli = collect(vec![opt("extended", None), opt("sandbox", None)]).unwrap();
        assert!(cli.dialect.ere && cli.dialect.sandbox);
    }
}
