//! Destination planning shared by `cp`, `mv` and `ln`: which operands are sources, which
//! operand is the destination, and where each source ends up (GNU rules for `-t`, `-T`
//! and a directory destination).

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::common::args::{report_usage, UsageError};
use crate::common::error::EXIT_FAIL;

use super::helpers::{join_name, os_text};

/// Prints a usage error with the `Try` line and returns the usage status.
pub fn usage(cmd: &str, message: String) -> i32 {
    report_usage(cmd, &UsageError { message }, EXIT_FAIL)
}

/// Where each source goes, and whether the destination is a directory.
pub struct Plan {
    pub sources: Vec<String>,
    pub dest: PathBuf,
    pub dest_is_dir: bool,
}

fn is_dir(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Works out sources and destination from the operands, following GNU's rules.
pub fn plan(
    cmd: &str,
    ops: &[String],
    target_dir: Option<&str>,
    no_target_dir: bool,
) -> Result<Plan, i32> {
    if let Some(dir) = target_dir {
        if ops.is_empty() {
            return Err(usage(cmd, "missing file operand".to_string()));
        }
        return match fs::metadata(dir) {
            Ok(meta) if meta.is_dir() => Ok(Plan {
                sources: ops.to_vec(),
                dest: PathBuf::from(dir),
                dest_is_dir: true,
            }),
            Ok(_) => {
                eprintln!("{cmd}: target '{dir}' is not a directory");
                Err(EXIT_FAIL)
            }
            Err(error) => {
                eprintln!("{cmd}: target directory '{dir}': {}", os_text(&error));
                Err(EXIT_FAIL)
            }
        };
    }
    if no_target_dir {
        return match ops {
            [] => Err(usage(cmd, "missing file operand".to_string())),
            [source] => Err(usage(
                cmd,
                format!("missing destination file operand after '{source}'"),
            )),
            [source, dest] => Ok(Plan {
                sources: vec![source.clone()],
                dest: PathBuf::from(dest),
                dest_is_dir: false,
            }),
            [_, _, extra, ..] => Err(usage(cmd, format!("extra operand '{extra}'"))),
        };
    }
    match ops {
        [] => return Err(usage(cmd, "missing file operand".to_string())),
        [source] => {
            return Err(usage(
                cmd,
                format!("missing destination file operand after '{source}'"),
            ))
        }
        _ => {}
    }
    let (sources, last) = ops.split_at(ops.len() - 1);
    let dest = &last[0];
    let dest_is_dir = is_dir(dest);
    if sources.len() > 1 && !dest_is_dir {
        if fs::metadata(dest).is_ok() {
            eprintln!("{cmd}: target '{dest}' is not a directory");
        } else {
            eprintln!("{cmd}: target '{dest}': No such file or directory");
        }
        return Err(EXIT_FAIL);
    }
    Ok(Plan {
        sources: sources.to_vec(),
        dest: PathBuf::from(dest),
        dest_is_dir,
    })
}

/// The path a source is copied to. Under `--parents` the whole source path is kept.
pub fn target_for(source: &str, plan: &Plan, parents: bool) -> PathBuf {
    if !plan.dest_is_dir {
        return plan.dest.clone();
    }
    if parents {
        let kept: PathBuf = Path::new(source)
            .components()
            .filter(|part| matches!(part, Component::Normal(_)))
            .collect();
        plan.dest.join(kept)
    } else {
        join_name(&plan.dest, source)
    }
}
