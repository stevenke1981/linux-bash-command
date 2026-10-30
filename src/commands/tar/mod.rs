//! `tar`: GNU tar 1.35 subset on the `tar` crate, with gzip through flate2.
//!
//! Supported: create (`-c`), extract (`-x`) and list (`-t`) of ustar, GNU and PAX archives,
//! with the options listed in `options::HELP`. Appending (`-r`) and the bzip2, xz, zstd and
//! lzma filters are not supported.

mod archive_io;
mod create;
mod extract;
mod list;
mod names;
mod options;
mod safety;

use std::io::Read;

use crate::common::error::{io_error_text, EXIT_TROUBLE};
use options::{Action, Operand, CMD};

/// Entry point for `tar`.
pub fn run(args: &[String]) -> i32 {
    let opts = match options::parse(args) {
        Ok(opts) => opts,
        Err(code) => return code,
    };
    let mut operands = opts.operands.clone();
    for source in &opts.files_from {
        match read_names(source, opts.null_separated) {
            Ok(names) => operands.extend(names.into_iter().map(|name| Operand {
                name,
                chdir: opts.extract_dir.clone(),
            })),
            Err(error) => {
                eprintln!("{CMD}: {source}: Cannot open: {}", io_error_text(&error));
                return EXIT_TROUBLE;
            }
        }
    }
    match opts.action {
        Action::Create => {
            if operands.is_empty() {
                eprintln!("{CMD}: Cowardly refusing to create an empty archive");
                eprintln!("Try '{CMD} --help' for more information.");
                return EXIT_TROUBLE;
            }
            create::run(&opts, &operands)
        }
        Action::Extract => extract::run(&opts, &operands),
        Action::List => list::run(&opts, &operands),
    }
}

/// Reads member names for `-T`: one per line, or NUL-separated with `--null`. `-` is
/// standard input.
fn read_names(source: &str, null_separated: bool) -> std::io::Result<Vec<String>> {
    let mut data = Vec::new();
    if source == "-" {
        std::io::stdin().lock().read_to_end(&mut data)?;
    } else {
        data = std::fs::read(source)?;
    }
    let separator = if null_separated { 0u8 } else { b'\n' };
    Ok(data
        .split(|&byte| byte == separator)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_lists_split_on_newlines_or_nuls() {
        let dir = std::env::temp_dir().join(format!("agentcmd-tar-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lines = dir.join("lines.txt");
        std::fs::write(&lines, b"one\ntwo\n\nthree\n").unwrap();
        let nuls = dir.join("nuls.bin");
        std::fs::write(&nuls, b"a b\0c\0").unwrap();
        let text = lines.to_string_lossy().into_owned();
        assert_eq!(
            read_names(&text, false).unwrap(),
            vec!["one", "two", "three"]
        );
        let bin = nuls.to_string_lossy().into_owned();
        assert_eq!(read_names(&bin, true).unwrap(), vec!["a b", "c"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_name_file_is_an_error() {
        assert!(read_names("definitely-missing-tar-list-file", false).is_err());
    }
}
