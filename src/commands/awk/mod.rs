//! `awk`: pattern scanning and text processing (POSIX awk with common gawk extensions).
//!
//! Stages: `lexer` → `parser` (builds `ast`) → `interp` (statements and expressions) with
//! `builtins`, `fields` (record and field splitting), `io` (input records, output streams,
//! getline) and `value`/`format`/`regex` as support.

#![allow(dead_code)]

mod ast;
mod format;
mod lexer;
mod regex;
mod value;

pub fn run(args: &[String]) -> i32 {
    super::not_implemented("awk", args)
}
