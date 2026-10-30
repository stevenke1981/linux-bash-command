//! `patch`: applies unified diffs (a GNU patch 2.7 subset).
//!
//! - `parse` reads the patch text into per-file hunk lists.
//! - `apply` places hunks (exact, offset and fuzz search) and builds the new file.
//! - `reject` writes `.rej` files, backups and replaces target files atomically.
//!
//! Target names that are absolute or contain `..` are refused, so a patch cannot write
//! outside the working directory.

mod apply;
mod parse;
mod reject;

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK, EXIT_TROUBLE};
use crate::common::io::{finish_output, stdout_writer};
use crate::common::path::display_path;

use apply::{assemble, join_file, place_hunk, reverse_hunk, split_file, FLine, Options, Placed};
use parse::{FilePatch, Hunk, ParseError};

const SPECS: &[OptSpec] = &[
    OptSpec::req("strip", Some('p'), Some("strip")),
    OptSpec::req("input", Some('i'), Some("input")),
    OptSpec::req("directory", Some('d'), Some("directory")),
    OptSpec::flag("reverse", Some('R'), Some("reverse")),
    OptSpec::flag("forward", Some('N'), Some("forward")),
    OptSpec::flag("dry_run", None, Some("dry-run")),
    OptSpec::req("output", Some('o'), Some("output")),
    OptSpec::flag("backup", Some('b'), Some("backup")),
    OptSpec::req("suffix", Some('z'), Some("suffix")),
    OptSpec::flag("no_backup", None, Some("no-backup-if-mismatch")),
    OptSpec::flag("remove_empty", Some('E'), Some("remove-empty-files")),
    OptSpec::flag("silent", Some('s'), Some("silent")),
    OptSpec::flag("silent", None, Some("quiet")),
    OptSpec::flag("force", Some('f'), Some("force")),
    OptSpec::flag("batch", Some('t'), Some("batch")),
    OptSpec::req("fuzz", Some('F'), Some("fuzz")),
    OptSpec::flag("ignore_ws", Some('l'), Some("ignore-whitespace")),
    OptSpec::flag("binary", None, Some("binary")),
    OptSpec::req("reject", Some('r'), Some("reject-file")),
    OptSpec::flag("unified", Some('u'), Some("unified")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "\
Usage: patch [OPTION]... [ORIGFILE [PATCHFILE]]

  -p NUM, --strip=NUM        strip NUM leading components from file names
  -i FILE, --input=FILE      read the patch from FILE (default: standard input)
  -d DIR, --directory=DIR    change to DIR before doing anything else
  -R, --reverse              assume the patch was created with old and new swapped
  -N, --forward              ignore patches that look reversed or already applied
      --dry-run              print the results without changing any file
  -o FILE, --output=FILE     write the patched result to FILE instead of in place
  -b, --backup               back up each changed file (suffix .orig)
  -z SUFFIX, --suffix=SUFFIX use SUFFIX instead of .orig for backups
      --no-backup-if-mismatch do not back up a file when a hunk needed fuzz or offset
  -E, --remove-empty-files   remove a file that the patch leaves empty
  -s, --silent, --quiet      print only the summary lines and errors
  -f, --force                never ask; do not check for reversed patches
  -t, --batch                never ask; assume a reversed patch is reversed (-R)
  -F NUM, --fuzz=NUM         maximum fuzz factor (default: 2)
  -l, --ignore-whitespace    match lines ignoring white space differences
      --binary               accepted for compatibility; has no effect
  -r FILE, --reject-file=FILE put all rejected hunks into FILE
  -u, --unified              accepted for compatibility; input is always unified
      --help                 display this help and exit
      --version              output version information and exit

Not supported by agentcmd: context diffs (-c), normal diffs (-n, -e), renames, copies
and binary patches. Questions that GNU patch asks on a terminal are answered \"n\".

Exit status is 0 if all hunks applied, 1 if some hunk failed or was ignored, and 2 for
serious trouble.

GNU patch home page: <https://www.gnu.org/software/patch/>.
General help using GNU software: <https://www.gnu.org/gethelp/>.
";

/// Parsed command line.
struct Cli {
    strip: Option<usize>,
    input: Option<String>,
    directory: Option<String>,
    reverse: bool,
    forward: bool,
    dry_run: bool,
    output: Option<String>,
    backup: bool,
    suffix: String,
    no_backup: bool,
    remove_empty: bool,
    silent: bool,
    force: bool,
    batch: bool,
    fuzz: usize,
    ignore_ws: bool,
    reject: Option<String>,
    operands: Vec<String>,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            strip: None,
            input: None,
            directory: None,
            reverse: false,
            forward: false,
            dry_run: false,
            output: None,
            backup: false,
            suffix: ".orig".to_string(),
            no_backup: false,
            remove_empty: false,
            silent: false,
            force: false,
            batch: false,
            fuzz: 2,
            ignore_ws: false,
            reject: None,
            operands: Vec::new(),
        }
    }
}

/// Entry point for the `patch` command.
pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new("patch", SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage("patch", &error, EXIT_TROUBLE),
    };
    let cli = match collect(items) {
        Ok(cli) => cli,
        Err(status) => return status,
    };
    let mut out = stdout_writer();
    let status = execute(&cli, &mut out);
    let flushed = finish_output("patch", &mut out);
    status.max(flushed)
}

/// Turns parsed items into a [`Cli`]. Help and version are handled by the caller
/// through `Cli`-independent early returns (see [`collect`]).
fn collect(items: Vec<Item>) -> Result<Cli, i32> {
    let mut cli = Cli::default();
    for item in items {
        match item {
            Item::Operand(text) => cli.operands.push(text),
            Item::Opt { id, value } => match id {
                "help" => {
                    print!("{HELP}");
                    return Err(EXIT_OK);
                }
                "version" => return Err(print_version("patch")),
                "strip" => cli.strip = Some(number(&value, "strip count")?),
                "input" => cli.input = value,
                "directory" => cli.directory = value,
                "reverse" => cli.reverse = true,
                "forward" => cli.forward = true,
                "dry_run" => cli.dry_run = true,
                "output" => cli.output = value,
                "backup" => cli.backup = true,
                "suffix" => cli.suffix = value.unwrap_or_default(),
                "no_backup" => cli.no_backup = true,
                "remove_empty" => cli.remove_empty = true,
                "silent" => cli.silent = true,
                "force" => cli.force = true,
                "batch" => cli.batch = true,
                "fuzz" => cli.fuzz = number(&value, "fuzz factor")?,
                "ignore_ws" => cli.ignore_ws = true,
                "reject" => cli.reject = value,
                _ => {}
            },
        }
    }
    if cli.operands.len() > 2 {
        eprintln!(
            "patch: extra operand '{}'\nTry 'patch --help' for more information.",
            cli.operands[2]
        );
        return Err(EXIT_TROUBLE);
    }
    Ok(cli)
}

/// Parses a numeric option value, reporting GNU's `patch: **** ` message on failure.
fn number(value: &Option<String>, what: &str) -> Result<usize, i32> {
    let text = value.as_deref().unwrap_or("");
    text.parse().map_err(|_| {
        eprintln!("patch: **** {what} '{text}' is not a number");
        EXIT_TROUBLE
    })
}

/// Reads the patch, applies every file section and returns the exit status.
fn execute(cli: &Cli, out: &mut dyn Write) -> i32 {
    if let Some(dir) = &cli.directory {
        if let Err(error) = std::env::set_current_dir(dir) {
            eprintln!(
                "patch: **** Can't change to directory {dir} : {}",
                io_error_text(&error)
            );
            return EXIT_TROUBLE;
        }
    }
    let source = cli
        .input
        .as_deref()
        .or(cli.operands.get(1).map(String::as_str));
    let input = match read_patch(source) {
        Ok(input) => input,
        Err(error) => {
            let name = source.unwrap_or("-");
            eprintln!(
                "patch: **** Can't open patch file {name} : {}",
                io_error_text(&error)
            );
            return EXIT_TROUBLE;
        }
    };
    let files = match parse::parse(&input) {
        Ok(files) => files,
        Err(error) => return report_parse_error(error),
    };
    let mut status = EXIT_OK;
    let mut rej_files = HashSet::new();
    for file in &files {
        match process_file(cli, file, out, &mut rej_files) {
            Ok(code) => status = status.max(code),
            Err(code) => return code,
        }
    }
    status
}

fn report_parse_error(error: ParseError) -> i32 {
    match error {
        ParseError::Garbage => eprintln!("patch: **** Only garbage was found in the patch input."),
        ParseError::Malformed { line } => {
            eprintln!("patch: **** malformed patch at line {line}")
        }
        ParseError::Unsupported(what) => {
            eprintln!("patch: **** {what} are not supported by agentcmd")
        }
    }
    EXIT_TROUBLE
}

fn read_patch(source: Option<&str>) -> io::Result<Vec<u8>> {
    match source {
        None | Some("-") => {
            let mut data = Vec::new();
            io::stdin().lock().read_to_end(&mut data)?;
            Ok(data)
        }
        Some(path) => fs::read(path),
    }
}

/// Result of one pass over a file's hunks.
struct Attempt {
    placed: Vec<Placed>,
    failed: Vec<usize>,
    ignored: bool,
    messages: Vec<String>,
}

/// What the caller does after a pass: finish, or start again with the hunks reversed.
enum Pass {
    Done(Attempt),
    Restart { reverse: bool, notice: String },
}

/// Applies one file section. `Err` carries a serious exit status that stops the run.
fn process_file(
    cli: &Cli,
    file: &FilePatch,
    out: &mut dyn Write,
    rej_files: &mut HashSet<String>,
) -> Result<i32, i32> {
    let Some(name) = choose_target(file, cli.strip, cli.reverse) else {
        eprintln!(
            "patch: **** can't find file to patch at input line {}",
            file.line_no
        );
        return Ok(EXIT_FAIL);
    };
    // An ORIGFILE operand names the file for every section, as in GNU patch.
    let target = match cli.operands.first() {
        Some(orig) => orig.clone(),
        None if is_unsafe(&name) => {
            eprintln!(
                "patch: **** refusing to patch '{name}': absolute paths and '..' are not allowed"
            );
            return Err(EXIT_TROUBLE);
        }
        None => name,
    };
    let creates = if cli.reverse {
        file.deletes
    } else {
        file.creates
    };
    let deletes = if cli.reverse {
        file.creates
    } else {
        file.deletes
    };
    let content = match read_target(&target, creates) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("patch: **** can't read {target}: {}", io_error_text(&error));
            return Ok(EXIT_FAIL);
        }
    };
    let lines = split_file(&content);
    let shown = display_path(Path::new(&target));
    if !cli.silent {
        let line = match (&cli.output, cli.dry_run) {
            (Some(out_name), _) => format!("patching file {out_name} (read from {shown})"),
            (None, true) => format!("checking file {shown}"),
            (None, false) => format!("patching file {shown}"),
        };
        let _ = writeln!(out, "{line}");
    }
    let (attempt, notices) = run_passes(cli, file, &lines);
    // A reversal the program assumed counts as a mismatch, so it is backed up as well.
    let mismatch = !notices.is_empty()
        || !attempt.failed.is_empty()
        || attempt.placed.iter().any(|p| p.fuzz > 0 || p.offset != 0);
    let total = file.hunks.len();
    for notice in notices.iter().chain(attempt.messages.iter()) {
        let _ = writeln!(out, "{notice}");
    }
    let rej_name = cli.reject.clone().unwrap_or_else(|| format!("{shown}.rej"));
    let summary = summary_line(&attempt, total, cli.dry_run, &rej_name);
    if let Some(line) = &summary {
        let _ = writeln!(out, "{line}");
    }
    if !cli.dry_run && (attempt.ignored || !attempt.failed.is_empty()) {
        let hunks: Vec<&Hunk> = if attempt.ignored {
            file.hunks.iter().collect()
        } else {
            attempt.failed.iter().map(|&k| &file.hunks[k]).collect()
        };
        let text = reject::reject_text(file, &hunks);
        let append = !rej_files.insert(rej_name.clone());
        if let Err(error) = reject::write_rejects(Path::new(&rej_name), &text, append) {
            eprintln!(
                "patch: **** Can't create file {rej_name} : {}",
                io_error_text(&error)
            );
            return Err(EXIT_TROUBLE);
        }
    }
    if !cli.dry_run {
        write_result(cli, &target, &lines, &attempt, deletes, mismatch)?;
    }
    Ok(if attempt.ignored || !attempt.failed.is_empty() {
        EXIT_FAIL
    } else {
        EXIT_OK
    })
}

/// Runs the hunks, switching to reversed hunks when the patch looks already applied.
/// Returns the final pass and the notices printed before it.
fn run_passes(cli: &Cli, file: &FilePatch, lines: &[FLine]) -> (Attempt, Vec<String>) {
    let mut reverse = cli.reverse;
    let mut detect = !cli.force;
    let mut notices = Vec::new();
    loop {
        let hunks: Vec<Hunk> = if reverse {
            file.hunks.iter().map(reverse_hunk).collect()
        } else {
            file.hunks.clone()
        };
        match run_pass(cli, &hunks, lines, reverse, detect) {
            Pass::Done(attempt) => return (attempt, notices),
            Pass::Restart {
                reverse: next,
                notice,
            } => {
                notices.push(notice);
                reverse = next;
                detect = false;
            }
        }
    }
}

/// One pass over `hunks` (already oriented). `detect` enables the check for a patch that
/// is reversed or already applied, run when a hunk fails to apply forwards.
fn run_pass(cli: &Cli, hunks: &[Hunk], lines: &[FLine], reverse: bool, detect: bool) -> Pass {
    let opts = Options {
        max_fuzz: cli.fuzz,
        ignore_ws: cli.ignore_ws,
    };
    let mut attempt = Attempt {
        placed: Vec::new(),
        failed: Vec::new(),
        ignored: false,
        messages: Vec::new(),
    };
    let mut floor = 0;
    let mut last_offset = 0;
    for (index, hunk) in hunks.iter().enumerate() {
        if let Some(placed) = place_hunk(lines, hunk, floor, last_offset, opts) {
            if !cli.silent && (placed.fuzz > 0 || placed.offset != 0) {
                attempt.messages.push(success_message(index + 1, &placed));
            }
            floor = placed.pos + placed.old_len;
            last_offset = placed.offset;
            attempt.placed.push(placed);
            continue;
        }
        if detect && place_hunk(lines, &reverse_hunk(hunk), 0, 0, opts).is_some() {
            return reversed_pass(cli, reverse, attempt.messages);
        }
        if !cli.silent {
            attempt
                .messages
                .push(format!("Hunk #{} FAILED at {}.", index + 1, hunk.old_start));
        }
        attempt.failed.push(index);
    }
    Pass::Done(attempt)
}

/// The decision for a patch that appears reversed or already applied.
fn reversed_pass(cli: &Cli, reverse: bool, mut messages: Vec<String>) -> Pass {
    let (headline, prompt) = if reverse {
        ("Unreversed patch detected!", "Ignore -R? [n]")
    } else {
        (
            "Reversed (or previously applied) patch detected!",
            "Assume -R? [n]",
        )
    };
    if cli.forward {
        messages.push(format!("{headline}  Skipping patch."));
        return Pass::Done(ignored_attempt(messages));
    }
    if cli.batch {
        let notice = if reverse {
            format!("{headline}  Ignoring -R.")
        } else {
            format!("{headline}  Assuming -R.")
        };
        return Pass::Restart {
            reverse: !reverse,
            notice,
        };
    }
    messages.push(format!("{headline}  {prompt} "));
    messages.push("Apply anyway? [n] ".to_string());
    if !cli.silent {
        messages.push("Skipping patch.".to_string());
    }
    Pass::Done(ignored_attempt(messages))
}

/// An attempt in which the whole section is skipped; every hunk counts as ignored.
fn ignored_attempt(messages: Vec<String>) -> Attempt {
    Attempt {
        placed: Vec::new(),
        failed: Vec::new(),
        ignored: true,
        messages,
    }
}

/// The summary line for failed or ignored hunks, if any.
fn summary_line(attempt: &Attempt, total: usize, dry_run: bool, rej_name: &str) -> Option<String> {
    let plural = |n: usize| if n == 1 { "hunk" } else { "hunks" };
    let save = if dry_run {
        String::new()
    } else {
        format!(" -- saving rejects to file {rej_name}")
    };
    if attempt.ignored {
        return Some(format!(
            "{total} out of {total} {} ignored{save}",
            plural(total)
        ));
    }
    if attempt.failed.is_empty() {
        return None;
    }
    let failed = attempt.failed.len();
    Some(format!(
        "{failed} out of {total} {} FAILED{save}",
        plural(total)
    ))
}

fn success_message(number: usize, placed: &Placed) -> String {
    let mut text = format!("Hunk #{number} succeeded at {}", placed.pos + 1);
    if placed.fuzz > 0 {
        text.push_str(&format!(" with fuzz {}", placed.fuzz));
    }
    if placed.offset != 0 {
        let unit = if placed.offset.abs() == 1 {
            "line"
        } else {
            "lines"
        };
        text.push_str(&format!(" (offset {} {unit})", placed.offset));
    }
    text.push('.');
    text
}

/// Writes the patched content, creates or removes the target, and makes a backup when
/// one is due. Output goes to `-o FILE` when given.
fn write_result(
    cli: &Cli,
    target: &str,
    lines: &[FLine],
    attempt: &Attempt,
    deletes: bool,
    mismatch: bool,
) -> Result<(), i32> {
    let changed = !attempt.placed.is_empty();
    let result = assemble(lines, &attempt.placed);
    let path = Path::new(target);
    if let Some(output) = &cli.output {
        return if changed {
            write_or_fail(Path::new(output), &join_file(&result))
        } else {
            Ok(())
        };
    }
    let touched = changed || !attempt.failed.is_empty();
    let backup_due = cli.backup || (!cli.no_backup && mismatch);
    if !attempt.ignored && touched && backup_due && path.exists() {
        if let Err(error) = reject::backup(path, &cli.suffix) {
            eprintln!(
                "patch: **** Can't create backup of {target} : {}",
                io_error_text(&error)
            );
            return Err(EXIT_TROUBLE);
        }
    }
    if !changed {
        return Ok(());
    }
    let emptied = result.is_empty();
    let remove = (deletes && !attempt.ignored && attempt.failed.is_empty() && emptied)
        || (cli.remove_empty && changed && emptied);
    if remove {
        if let Err(error) = fs::remove_file(path) {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!(
                    "patch: **** Can't remove file {target} : {}",
                    io_error_text(&error)
                );
                return Err(EXIT_TROUBLE);
            }
        }
        return Ok(());
    }
    write_or_fail(path, &join_file(&result))
}

fn write_or_fail(path: &Path, data: &[u8]) -> Result<(), i32> {
    reject::replace_file(path, data).map_err(|error| {
        eprintln!(
            "patch: **** Can't write {} : {}",
            display_path(path),
            io_error_text(&error)
        );
        EXIT_TROUBLE
    })
}

fn read_target(path: &str, creates: bool) -> io::Result<Vec<u8>> {
    match fs::read(path) {
        Ok(data) => Ok(data),
        Err(error) if creates && error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// The file a section applies to. With `reverse` the old and new names trade places.
/// An existing new name wins, then an existing old name, then the new name.
fn choose_target(file: &FilePatch, strip: Option<usize>, reverse: bool) -> Option<String> {
    let (old, new) = if reverse {
        (&file.new_name, &file.old_name)
    } else {
        (&file.old_name, &file.new_name)
    };
    let creates = if reverse { file.deletes } else { file.creates };
    let deletes = if reverse { file.creates } else { file.deletes };
    let old = (old != "/dev/null").then(|| strip_name(old, strip));
    let new = (new != "/dev/null").then(|| strip_name(new, strip));
    let chosen = if creates {
        new
    } else if deletes {
        old
    } else {
        match (old, new) {
            (Some(o), Some(n)) => {
                if Path::new(&n).exists() || !Path::new(&o).exists() {
                    Some(n)
                } else {
                    Some(o)
                }
            }
            (Some(o), None) => Some(o),
            (None, n) => n,
        }
    };
    chosen.filter(|name| !name.is_empty())
}

/// Removes `strip` leading components; without a count only the final component is kept.
fn strip_name(name: &str, strip: Option<usize>) -> String {
    match strip {
        None => name.rsplit('/').next().unwrap_or("").to_string(),
        Some(count) => {
            let mut rest = name;
            for _ in 0..count {
                match rest.find('/') {
                    Some(index) => rest = &rest[index + 1..],
                    None => return String::new(),
                }
            }
            rest.to_string()
        }
    }
}

/// True for names that could write outside the working directory.
fn is_unsafe(name: &str) -> bool {
    name.starts_with('/')
        || name.starts_with('\\')
        || Path::new(name).components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(old: &str, new: &str, creates: bool, deletes: bool) -> FilePatch {
        FilePatch {
            old_line: format!("--- {old}"),
            new_line: format!("+++ {new}"),
            old_name: old.to_string(),
            new_name: new.to_string(),
            creates,
            deletes,
            line_no: 1,
            hunks: Vec::new(),
        }
    }

    #[test]
    fn strip_counts_components() {
        assert_eq!(strip_name("a/b/c.txt", Some(0)), "a/b/c.txt");
        assert_eq!(strip_name("a/b/c.txt", Some(1)), "b/c.txt");
        assert_eq!(strip_name("a/b/c.txt", Some(2)), "c.txt");
        assert_eq!(strip_name("a/b", Some(3)), "");
        assert_eq!(strip_name("a/b/c.txt", None), "c.txt");
    }

    #[test]
    fn unsafe_names_are_refused() {
        assert!(is_unsafe("/etc/passwd"));
        assert!(is_unsafe("../x"));
        assert!(is_unsafe("a/../../x"));
        assert!(is_unsafe("\\\\server\\share"));
        assert!(!is_unsafe("a/b.txt"));
        assert!(!is_unsafe("./a.txt"));
    }

    #[test]
    fn creation_and_deletion_pick_the_right_name() {
        assert_eq!(
            choose_target(&file("/dev/null", "b/n", true, false), Some(1), false).as_deref(),
            Some("n")
        );
        assert_eq!(
            choose_target(&file("a/n", "/dev/null", false, true), Some(1), false).as_deref(),
            Some("n")
        );
        assert_eq!(
            choose_target(&file("/dev/null", "n", true, false), Some(1), true),
            None
        );
    }

    #[test]
    fn reverse_swaps_names_for_target_choice() {
        // A creation patch reversed is a deletion of the same file.
        let fp = file("/dev/null", "n", true, false);
        assert_eq!(choose_target(&fp, Some(0), true).as_deref(), Some("n"));
    }

    #[test]
    fn success_message_matches_gnu_wording() {
        let placed = |pos, fuzz, offset| Placed {
            pos,
            old_len: 1,
            fuzz,
            offset,
            new_lines: Vec::new(),
        };
        assert_eq!(
            success_message(2, &placed(39, 0, 3)),
            "Hunk #2 succeeded at 40 (offset 3 lines)."
        );
        assert_eq!(
            success_message(1, &placed(2, 1, 0)),
            "Hunk #1 succeeded at 3 with fuzz 1."
        );
        assert_eq!(
            success_message(1, &placed(4, 2, -1)),
            "Hunk #1 succeeded at 5 with fuzz 2 (offset -1 line)."
        );
    }

    #[test]
    fn summary_lines_for_failures_and_ignored_hunks() {
        let failed = Attempt {
            placed: Vec::new(),
            failed: vec![0],
            ignored: false,
            messages: Vec::new(),
        };
        assert_eq!(
            summary_line(&failed, 1, false, "x.rej").as_deref(),
            Some("1 out of 1 hunk FAILED -- saving rejects to file x.rej")
        );
        assert_eq!(
            summary_line(&failed, 3, true, "x.rej").as_deref(),
            Some("1 out of 3 hunks FAILED")
        );
        let ignored = ignored_attempt(Vec::new());
        assert_eq!(
            summary_line(&ignored, 2, false, "x.rej").as_deref(),
            Some("2 out of 2 hunks ignored -- saving rejects to file x.rej")
        );
        let clean = Attempt {
            placed: Vec::new(),
            failed: Vec::new(),
            ignored: false,
            messages: Vec::new(),
        };
        assert_eq!(summary_line(&clean, 1, false, "x"), None);
    }
}
