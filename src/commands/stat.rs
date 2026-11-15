pub(crate) mod filesystem;
mod format;

use crate::common::{
    args::{self, Item, OptSpec},
    error::{io_text, usage},
    io::output,
};
use std::fs;
use std::path::Path;

const HELP: &str = "Usage: stat [OPTION]... FILE...\nDisplay file or file system status.\n  -L, --dereference       follow symbolic links\n  -f, --file-system       display file system status\n  -c, --format=FORMAT     use FORMAT and append a newline\n      --printf=FORMAT     use FORMAT with escapes; no added newline\n  -t, --terse             use terse output\n      --cached=WHEN       accept always, never, or default\n      --help              display this help\n      --version           output version information\n";

pub fn run(args: &[String]) -> i32 {
    let specs = [
        OptSpec::flag("dereference", 'L'),
        OptSpec::flag("file-system", 'f'),
        OptSpec::value("format", 'c'),
        OptSpec::value("printf", '\0'),
        OptSpec::flag("terse", 't'),
        OptSpec::value("cached", '\0'),
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let items = match args::parse(args, &specs, true) {
        Ok(items) => items,
        Err(error) => return usage("stat", error, 1),
    };
    let (mut dereference, mut filesystem, mut terse, mut printf) = (false, false, false, false);
    let mut template = None;
    let mut paths = Vec::new();
    for item in items {
        match item {
            Item::Opt(id @ ("help" | "version"), _) => {
                return args::help_version("stat", &[format!("--{id}")], HELP, true).unwrap()
            }
            Item::Operand(path) => paths.push(path),
            Item::Opt("dereference", _) => dereference = true,
            Item::Opt("file-system", _) => filesystem = true,
            Item::Opt("terse", _) => terse = true,
            Item::Opt("format", value) => {
                template = value;
                printf = false;
            }
            Item::Opt("printf", value) => {
                template = value;
                printf = true;
            }
            Item::Opt("cached", Some(value))
                if matches!(value.as_str(), "always" | "never" | "default") => {}
            Item::Opt("cached", Some(value)) => {
                return usage(
                    "stat",
                    format!("invalid argument '{value}' for 'cached'"),
                    1,
                )
            }
            _ => unreachable!(),
        }
    }
    if paths.is_empty() {
        return usage("stat", "missing operand", 1);
    }
    let custom = template.is_some();
    let template = template.unwrap_or_else(|| {
        if filesystem {
            filesystem::default_format(terse).into()
        } else {
            format::default_format(terse).into()
        }
    });
    let template = if printf {
        match crate::common::escape::decode(
            template.as_bytes(),
            crate::common::escape::Mode::Printf,
        ) {
            Ok((bytes, _)) => bytes,
            Err(error) => return usage("stat", error, 1),
        }
    } else {
        template.into_bytes()
    };
    let mut failed = false;
    let context_requested = !filesystem && format::context_requested(&template);
    let status = output("stat", |out| {
        for name in &paths {
            let path = Path::new(name);
            let rendered = if filesystem {
                filesystem::render(path, &template)
            } else {
                (if dereference {
                    fs::metadata(path)
                } else {
                    fs::symlink_metadata(path)
                })
                .map(|meta| {
                    if custom {
                        format::render_bytes(path, &meta, &template)
                    } else {
                        format::render_default_bytes(path, &meta, &template)
                    }
                })
            };
            match rendered {
                Ok(text) => {
                    out.write_all(&text)?;
                    if context_requested {
                        eprintln!(
                            "stat: failed to get security context of '{}': No data available",
                            crate::display_path(path)
                        );
                        failed = true;
                    }
                    if !printf && custom {
                        out.write_all(b"\n")?;
                    }
                }
                Err(error) => {
                    eprintln!(
                        "stat: cannot stat{} '{}': {}",
                        if filesystem { " file system of" } else { "x" },
                        crate::display_path(path),
                        io_text(&error)
                    );
                    failed = true;
                }
            }
        }
        Ok(())
    });
    status.max(i32::from(failed))
}
