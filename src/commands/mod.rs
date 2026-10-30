//! Commands added by the coreutils alignment work.

pub mod awk;
pub mod basenc;
pub mod cksum;
pub mod cmp;
pub mod column;
pub mod comm;
pub mod csplit;
pub mod cut;
pub mod date;
pub mod dos2unix;
pub mod du;
pub mod echo;
pub mod env;
pub mod expand;
pub mod expr;
pub mod factor;
pub mod file;
pub mod fmt;
pub mod fold;
pub mod fsops;
pub mod grep_alias;
pub mod gzip;
pub mod hashsum;
pub mod join;
pub mod mktemp;
pub mod nl;
pub mod numfmt;
pub mod od;
pub mod paste;
pub mod patch;
pub mod pathchk;
pub mod pathname;
pub mod pgrep;
pub mod pr;
pub mod printf;
pub mod ptx;
pub mod realpath;
pub mod rev;
pub mod sed;
pub mod seq;
pub mod shuf;
pub mod signal;
pub mod sleep;
pub mod sort;
pub mod split;
pub mod stat;
pub mod sysinfo_cmds;
pub mod tac;
pub mod tar;
pub mod tee;
pub mod test;
pub mod timeout;
pub mod tr;
pub mod tree;
pub mod truefalse;
pub mod truncate;
pub mod tsort;
pub mod uniq;
pub mod wc;
pub mod which;
pub mod xargs;
pub mod xxd;
pub mod yes;

/// Placeholder used while a command is being implemented.
pub fn not_implemented(name: &str, _args: &[String]) -> i32 {
    eprintln!("{name}: not implemented yet");
    1
}
