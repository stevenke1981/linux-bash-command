mod compare;
mod keys;
mod storage;
use crate::common::{
    args::{self, HasArg, Item, OptSpec},
    error,
    external::{Config, Sorter},
    io,
};
use compare::{hash, Modifiers};
use keys::Key;
use std::cmp::Ordering;
use std::io::{Read, Write};

const HELP: &str = "Usage: sort [OPTION]... [FILE]...\nSort lines using byte order in the C locale.\n  -b, --ignore-leading-blanks   ignore leading blanks\n  -d, --dictionary-order       compare letters, digits and blanks\n  -f, --ignore-case            fold ASCII case\n  -g, --general-numeric-sort   compare floating point numbers\n  -i, --ignore-nonprinting     ignore nonprinting bytes\n  -M, --month-sort             compare month abbreviations\n  -h, --human-numeric-sort     compare human sizes\n  -n, --numeric-sort           compare exact decimal numbers\n  -R, --random-sort            group equal keys randomly\n      --random-source=FILE     obtain random seed from FILE\n  -r, --reverse                reverse comparisons\n  -V, --version-sort           compare version numbers\n  -k, --key=KEYDEF             use F[.C][OPTS][,F[.C][OPTS]]\n  -t, --field-separator=SEP    separate fields with SEP\n  -u, --unique                 output first of equal keys\n  -s, --stable                 disable whole-line fallback\n  -c, --check[=diagnose-first|quiet|silent] check order\n  -C                          check order silently\n  -m, --merge                  merge sorted files\n  -o, --output=FILE            write output to FILE\n  -z, --zero-terminated        use NUL records\n  -S, --buffer-size=SIZE       memory budget before external spill (default 256 MiB)\n  -T, --temporary-directory=DIR temporary directory\n      --parallel=N            accepted worker count\n      --sort=WORD             general-numeric,human-numeric,month,numeric,random,version\n      --files0-from=FILE       read NUL-separated input names\n      --batch-size=N          merge at most N files per batch\n      --help                  display this help\n      --version               output version information\nExit status: 0 success, 1 disorder, 2 error.\n";

fn usage(message: &str) -> i32 {
    error::usage("sort", message, 2)
}

pub fn run(arguments: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
        OptSpec::flag("ignore-leading-blanks", 'b'),
        OptSpec::flag("dictionary-order", 'd'),
        OptSpec::flag("ignore-case", 'f'),
        OptSpec::flag("general-numeric-sort", 'g'),
        OptSpec::flag("ignore-nonprinting", 'i'),
        OptSpec::flag("month-sort", 'M'),
        OptSpec::flag("human-numeric-sort", 'h'),
        OptSpec::flag("numeric-sort", 'n'),
        OptSpec::flag("random-sort", 'R'),
        OptSpec::flag("reverse", 'r'),
        OptSpec::flag("version-sort", 'V'),
        OptSpec::value("key", 'k'),
        OptSpec::value("field-separator", 't'),
        OptSpec::flag("unique", 'u'),
        OptSpec::flag("stable", 's'),
        OptSpec::optional("check", '\0'),
        OptSpec {
            id: "check",
            long: "",
            short: 'c',
            has_arg: HasArg::No,
        },
        OptSpec {
            id: "quiet-check",
            long: "",
            short: 'C',
            has_arg: HasArg::No,
        },
        OptSpec::flag("merge", 'm'),
        OptSpec::value("output", 'o'),
        OptSpec::flag("zero-terminated", 'z'),
        OptSpec::value("buffer-size", 'S'),
        OptSpec::value("temporary-directory", 'T'),
        OptSpec::value("parallel", '\0'),
        OptSpec::value("random-source", '\0'),
        OptSpec::value("sort", '\0'),
        OptSpec::value("files0-from", '\0'),
        OptSpec::value("batch-size", '\0'),
    ];
    let parsed = match args::parse(arguments, &specs, true) {
        Ok(v) => v,
        Err(e) => return usage(&e),
    };
    let mut global = Modifiers::default();
    let (mut key_text, mut files, mut separator, mut output, mut files0, mut random) =
        (Vec::new(), Vec::new(), None, None, None, None);
    let (mut stable, mut unique, mut check, mut quiet, mut merge, mut delimiter) =
        (false, false, false, false, false, b'\n');
    let mut storage = Config::default();
    let mut temporary_set = false;
    for item in parsed {
        let (id, value) = match item {
            Item::Operand(s) => {
                files.push(s);
                continue;
            }
            Item::Opt(i, v) => (i, v),
        };
        if matches!(id, "help" | "version") {
            return args::help_version("sort", &[format!("--{id}")], HELP, true).unwrap();
        }
        let flag = match id {
            "ignore-leading-blanks" => b'b',
            "dictionary-order" => b'd',
            "ignore-case" => b'f',
            "general-numeric-sort" => b'g',
            "ignore-nonprinting" => b'i',
            "month-sort" => b'M',
            "human-numeric-sort" => b'h',
            "numeric-sort" => b'n',
            "random-sort" => b'R',
            "reverse" => b'r',
            "version-sort" => b'V',
            _ => 0,
        };
        if flag != 0 {
            if b"gMhnRV".contains(&flag) && global.mode != 0 && global.mode != flag {
                return usage("options are incompatible");
            }
            global.set(flag);
            continue;
        }
        match id {
            "key" => key_text.push(value.unwrap()),
            "field-separator" => {
                let s = value.unwrap();
                let bytes = if s == "\\0" {
                    b"\0".as_slice()
                } else {
                    s.as_bytes()
                };
                if bytes.len() != 1 {
                    return usage("multi-character tab");
                }
                separator = Some(bytes[0]);
            }
            "output" => output = value,
            "files0-from" => files0 = value,
            "random-source" => random = value,
            "stable" => stable = true,
            "unique" => unique = true,
            "merge" => merge = true,
            "zero-terminated" => delimiter = 0,
            "check" => {
                check = true;
                match value.as_deref() {
                    None | Some("diagnose-first") => quiet = false,
                    Some("quiet" | "silent") => quiet = true,
                    _ => return usage("invalid argument for '--check'"),
                }
            }
            "quiet-check" => {
                check = true;
                quiet = true;
            }
            "sort" => {
                let mode = match value.as_deref() {
                    Some("general-numeric") => b'g',
                    Some("human-numeric") => b'h',
                    Some("month") => b'M',
                    Some("numeric") => b'n',
                    Some("random") => b'R',
                    Some("version") => b'V',
                    _ => return usage("invalid argument for '--sort'"),
                };
                if global.mode != 0 && global.mode != mode {
                    return usage("options are incompatible");
                }
                global.mode = mode;
            }
            "parallel" | "batch-size" => {
                let minimum = if id == "batch-size" { 2 } else { 1 };
                let count = match value.as_ref().and_then(|v| v.parse::<usize>().ok()) {
                    Some(n) if n >= minimum => n,
                    _ => return usage("invalid count"),
                };
                if id == "batch-size" {
                    storage.batch_size = count;
                }
            }
            "buffer-size" => match storage::buffer_size(&value.unwrap()) {
                Ok(size) => storage.memory_limit = size,
                Err(error) => return usage(error),
            },
            "temporary-directory" => {
                if !temporary_set {
                    storage.temporary_dirs.clear();
                    temporary_set = true;
                }
                storage
                    .temporary_dirs
                    .push(std::path::PathBuf::from(value.unwrap()));
            }
            _ => unreachable!(),
        }
    }
    if check && (merge || output.is_some()) {
        return usage("options are incompatible");
    }
    let mut keys = Vec::new();
    for text in key_text {
        match Key::parse(&text, &global) {
            Ok(k) => keys.push(k),
            Err(e) => return usage(&e),
        }
    }
    let seed = if let Some(name) = random {
        let mut bytes = [0; 32];
        match io::input(&name).and_then(|mut r| r.read_exact(&mut bytes)) {
            Ok(()) => hash(&bytes, 0),
            Err(e) => {
                eprintln!("sort: {name}: {}", error::io_text(&e));
                return 2;
            }
        }
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    };
    if let Some(name) = files0 {
        if !files.is_empty() {
            return usage("extra operand with --files0-from");
        }
        let mut data = Vec::new();
        if let Err(e) = io::input(&name).and_then(|mut r| r.read_to_end(&mut data)) {
            eprintln!("sort: {name}: {}", error::io_text(&e));
            return 2;
        }
        if data.is_empty() {
            return usage(&format!("no input from '{}'", name));
        }
        for path in data.strip_suffix(&[0]).unwrap_or(&data).split(|b| *b == 0) {
            if path.is_empty() {
                return usage("invalid zero-length file name");
            }
            files.push(String::from_utf8_lossy(path).to_string());
        }
        if files.is_empty() {
            return usage(&format!("no input from '{}'", name));
        }
    }
    if files.is_empty() {
        files.push("-".into());
    }
    if check && files.len() > 1 {
        return usage("extra operand");
    }
    let compare_keys = |a: &[u8], b: &[u8]| {
        if keys.is_empty() {
            return global.compare(a, b, seed);
        }
        for k in &keys {
            let o = k
                .modifiers
                .compare(k.slice(a, separator), k.slice(b, separator), seed);
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    };
    let compare = |a: &[u8], b: &[u8]| {
        compare_keys(a, b).then_with(|| {
            if stable || unique {
                Ordering::Equal
            } else if global.reverse {
                b.cmp(a)
            } else {
                a.cmp(b)
            }
        })
    };
    let mut sorter = Sorter::new(storage, &compare);
    sorter.set_unique(unique);
    for name in &files {
        let mut input = match io::input(name) {
            Ok(input) => input,
            Err(error) => {
                eprintln!("sort: {name}: {}", error::io_text(&error));
                return 2;
            }
        };
        if check {
            return match storage::check(&mut input, delimiter, unique, &compare, &compare_keys) {
                Ok(Some((line, row))) => {
                    if !quiet {
                        eprintln!(
                            "sort: {name}:{line}: disorder: {}",
                            String::from_utf8_lossy(&row)
                        );
                    }
                    1
                }
                Ok(None) => 0,
                Err(error) => {
                    eprintln!("sort: {name}: {}", error::io_text(&error));
                    2
                }
            };
        }
        let result = if merge {
            sorter.add_sorted_input(&mut input, delimiter)
        } else {
            io::records(&mut input, delimiter, |row| sorter.push(row.to_vec()))
        };
        if let Err(error) = result {
            eprintln!("sort: {name}: {}", error::io_text(&error));
            return 2;
        }
    }
    let write = |out: &mut dyn Write| {
        sorter.finish(&mut |row| {
            out.write_all(row)?;
            out.write_all(&[delimiter])
        })
    };
    if let Some(name) = output {
        let result = storage::output(&name, write);
        if let Err(error) = result {
            eprintln!("sort: {name}: {}", error::io_text(&error));
            2
        } else {
            0
        }
    } else {
        let code = io::output("sort", write);
        if code == 1 {
            2
        } else {
            code
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn external_merge_memory_limit() {
        let root = std::env::temp_dir().join(format!(
            "agentcmd-sort-spill-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&root).unwrap();
        let input = root.join("input");
        let output = root.join("output");
        let lines: Vec<_> = (0..200).map(|n| format!("{n:04} value\n")).collect();
        std::fs::write(&input, lines.iter().rev().cloned().collect::<String>()).unwrap();
        let args = vec![
            "-S1K".into(),
            "--batch-size=2".into(),
            "-T".into(),
            root.to_string_lossy().into_owned(),
            "-o".into(),
            output.to_string_lossy().into_owned(),
            input.to_string_lossy().into_owned(),
        ];
        assert_eq!(super::run(&args), 0);
        assert_eq!(std::fs::read(&output).unwrap(), lines.concat().as_bytes());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
}
