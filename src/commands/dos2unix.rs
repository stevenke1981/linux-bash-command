mod stream;
use crate::common::{args, error, external::TemporaryFile, io as common_io};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};
use stream::Settings;

const DOS_HELP: &str = "Usage: dos2unix [OPTION]... [FILE]...\nConvert text files from DOS to Unix format.\n  -n, --newfile IN OUT  write converted input to output\n  -o, --oldfile         convert files in place (default)\n  -k, --keep-date       keep output date\n  -q, --quiet           quiet mode\n  -f, --force           force binary conversion\n  -b, --keep-bom        keep byte-order mark\n  -r, --remove-bom      remove byte-order mark\n  -m, --add-bom         add byte-order mark\n  -c, --convmode=MODE   conversion mode (ascii only)\n  -e, --add-eol         ensure final newline\n  -s, --safe            skip binary files (default)\n";
const UNIX_HELP: &str = "Usage: unix2dos [OPTION]... [FILE]...\nConvert text files from Unix to DOS format.\n  -n, --newfile IN OUT  write converted input to output\n  -o, --oldfile         convert files in place (default)\n  -k, --keep-date       keep output date\n  -q, --quiet           quiet mode\n  -f, --force           force binary conversion\n  -b, --keep-bom        keep byte-order mark\n  -r, --remove-bom      remove byte-order mark\n  -m, --add-bom         add byte-order mark\n  -c, --convmode=MODE   conversion mode (ascii only)\n  -e, --add-eol         ensure final newline\n  -s, --safe            skip binary files (default)\n";

#[derive(Default)]
struct Options {
    settings: Settings,
    quiet: bool,
    keep_date: bool,
    files: Vec<(String, String)>,
}
fn specs() -> [args::OptSpec; 11] {
    [
        args::OptSpec::flag("newfile", 'n'),
        args::OptSpec::flag("oldfile", 'o'),
        args::OptSpec::flag("keep-date", 'k'),
        args::OptSpec::flag("quiet", 'q'),
        args::OptSpec::flag("force", 'f'),
        args::OptSpec::flag("keep-bom", 'b'),
        args::OptSpec::flag("remove-bom", 'r'),
        args::OptSpec::flag("add-bom", 'm'),
        args::OptSpec::value("convmode", 'c'),
        args::OptSpec::flag("add-eol", 'e'),
        args::OptSpec::flag("safe", 's'),
    ]
}
fn parse(command: &str, argv: &[String], to_dos: bool) -> Result<Options, i32> {
    let help = if to_dos { UNIX_HELP } else { DOS_HELP };
    let items = args::parse_command(command, argv, help, &specs(), true)?;
    let mut o = Options {
        settings: Settings {
            to_dos,
            ..Settings::default()
        },
        ..Options::default()
    };
    let mut newfile = false;
    let mut pending = None;
    for item in items {
        match item {
            args::Item::Operand(v) if newfile => {
                if let Some(input) = pending.take() {
                    o.files.push((input, v));
                } else {
                    pending = Some(v);
                }
            }
            args::Item::Operand(v) => o.files.push((v.clone(), v)),
            args::Item::Opt("newfile" | "oldfile", _) => {
                if pending.is_some() {
                    return Err(error::usage(command, "-n requires input/output pairs", 1));
                }
                newfile = matches!(item, args::Item::Opt("newfile", _));
            }
            args::Item::Opt("keep-date", _) => o.keep_date = true,
            args::Item::Opt("quiet", _) => o.quiet = true,
            args::Item::Opt("force", _) => o.settings.force = true,
            args::Item::Opt("safe", _) => o.settings.force = false,
            args::Item::Opt("keep-bom", _) => o.settings.keep_bom = true,
            args::Item::Opt("remove-bom", _) => o.settings.remove_bom = true,
            args::Item::Opt("add-bom", _) => o.settings.add_bom = true,
            args::Item::Opt("add-eol", _) => o.settings.add_eol = true,
            args::Item::Opt("convmode", Some(v)) if v == "ascii" => {}
            args::Item::Opt("convmode", _) => {
                return Err(error::usage(
                    command,
                    "only ascii conversion mode is supported",
                    1,
                ))
            }
            _ => {}
        }
    }
    finish_options(command, o, pending.is_some(), newfile)
}
fn finish_options(command: &str, o: Options, pending: bool, newfile: bool) -> Result<Options, i32> {
    if pending || (newfile && o.files.is_empty()) {
        return Err(error::usage(command, "-n requires input/output pairs", 1));
    }
    let s = o.settings;
    if (s.keep_bom as u8 + s.remove_bom as u8 + s.add_bom as u8) > 1 {
        return Err(error::usage(command, "conflicting BOM options", 1));
    }
    Ok(o)
}
fn convert_file(input: &str, output: &str, o: &Options) -> io::Result<bool> {
    let (source, metadata): (Box<dyn Read>, _) = if input == "-" {
        (Box::new(io::stdin()), None)
    } else {
        let file = File::open(input)?;
        let metadata = file.metadata()?;
        (Box::new(file), Some(metadata))
    };
    let dest = Path::new(output);
    let tempdir = std::env::temp_dir();
    let directory = if output == "-" {
        tempdir.as_path()
    } else {
        dest.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    };
    let mut temp = TemporaryFile::new(directory)?;
    if !stream::convert(source, temp.file(), o.settings)? {
        return Ok(false);
    }
    if output == "-" {
        let stdout = io::stdout();
        let mut out = io::BufWriter::with_capacity(65536, stdout.lock());
        let result = io::copy(&mut temp.reader()?, &mut out).and_then(|_| out.flush());
        return match result {
            Err(e) if e.kind() != io::ErrorKind::BrokenPipe => Err(e),
            _ => Ok(true),
        };
    }
    if let Some(metadata) = metadata {
        temp.file().set_permissions(metadata.permissions())?;
        if o.keep_date {
            filetime::set_file_handle_times(
                temp.file(),
                metadata.accessed().ok().map(Into::into),
                metadata.modified().ok().map(Into::into),
            )?;
        }
    }
    temp.persist(dest)?;
    Ok(true)
}
fn convert_stdin(command: &str, o: &Options) -> i32 {
    let mut temp = match TemporaryFile::new(&std::env::temp_dir()) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("{command}: {}", error::io_text(&e));
            return 1;
        }
    };
    match stream::convert(io::stdin().lock(), temp.file(), o.settings) {
        Ok(true) => common_io::output(command, |out| {
            io::copy(&mut temp.reader()?, out)?;
            Ok(())
        }),
        Ok(false) => {
            if !o.quiet {
                eprintln!("{command}: Skipping binary file -");
            }
            0
        }
        Err(e) => {
            eprintln!("{command}: {}", error::io_text(&e));
            1
        }
    }
}
pub(super) fn run_direction(argv: &[String], to_dos: bool) -> i32 {
    let command = if to_dos { "unix2dos" } else { "dos2unix" };
    let o = match parse(command, argv, to_dos) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if o.files.is_empty() {
        return convert_stdin(command, &o);
    }
    let mut failed = false;
    for (input, output) in &o.files {
        if input == "-" && output == "-" {
            failed |= convert_stdin(command, &o) != 0;
            continue;
        }
        match convert_file(input, output, &o) {
            Ok(true) if !o.quiet => eprintln!(
                "{command}: converting file {input} to {} format...",
                if to_dos { "DOS" } else { "Unix" }
            ),
            Ok(false) if !o.quiet => eprintln!("{command}: Skipping binary file {input}"),
            Ok(_) => {}
            Err(e) => {
                eprintln!("{command}: {input}: {}", error::io_text(&e));
                failed = true;
            }
        }
    }
    i32::from(failed)
}
pub fn run(argv: &[String]) -> i32 {
    run_direction(argv, false)
}
