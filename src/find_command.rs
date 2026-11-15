//! `find`: a GNU-style expression parser and evaluator for file tree searches.
//!
//! The implementation is split into submodules: `parse` (arguments to expression tree),
//! `expr` (tree types and tests), `walk` (traversal and actions), `printf` (`-printf` and
//! `-ls` output), `exec` (command spawning) and `glob` (wildcards).

mod exec;
mod expr;
pub(crate) mod glob;
mod output;
mod parse;
mod printf;
mod walk;

const HELP: &str = "Usage: find [PATH...] [EXPRESSION]

Search for files in a directory hierarchy. Default starting point: '.'.
Output is in directory order (entries sorted by name within each directory).

Options (anywhere in the expression):
  -maxdepth N, -mindepth N   Limit or start the depth of tests and actions
  -depth                     Process directory contents before the directory
  -L, -follow                Follow symbolic links (loops are detected)
  -H                         Follow symbolic links only for starting points
  -P                         Never follow symbolic links (default)
  -xdev, -mount, -noleaf     Stay on the starting file system (-noleaf is accepted)

Tests:
  -name PAT, -iname PAT      Basename matches a wildcard (* ? [...] \\ escape)
  -path PAT, -ipath PAT      Whole path matches a wildcard (also -wholename)
  -regex RE, -iregex RE      Whole path matches a regular expression (Rust regex syntax)
  -type f|d|l[,...]          File type; b c p s are accepted on Unix only
  -empty                     Empty regular file or directory
  -size [+-]N[cwbkMG]        Size; sizes round up to whole units (default 512-byte blocks)
  -newer FILE                Modified later than FILE (also -anewer, -cnewer)
  -mtime/-atime/-ctime [+-]N Age in whole days (-mmin/-amin/-cmin in minutes)
  -perm [-/]MODE             Octal mode (Unix only)
  -readable, -writable, -executable
                             Best-effort access checks
  -true, -false
  -newerXY FILE/TIME          Compare a/c/m/B clocks; t references a date string
  -user NAME, -group NAME     Match an owner name or numeric ID
  -uid N, -gid N              Match numeric IDs with optional + or -
  -nouser, -nogroup           Owner or group has no name
  -links N, -inum N           Match link count or inode
  -samefile FILE              Match the same device and inode
  -lname PAT, -ilname PAT     Match symbolic-link targets
  -xtype TYPE                Match the type with symbolic-link following reversed
  -regextype TYPE             posix-basic, posix-extended, or emacs (ERE)
  -ignore_readdir_race        Ignore files disappearing during traversal

Actions:
  -print, -print0, -ls       Print the path (default action when none is given)
  -printf FORMAT             Formatted output: %p %P %f %h %s %d %y %m %T@ and escapes
                             \\n \\t \\0 \\\\ (other %T letters are rejected)
  -prune                     Do not descend into the directory
  -quit                      Stop after the current entry
  -delete                    Remove entries (implies -depth; '.' is kept)
  -exec CMD {} ;             Run CMD per entry; success means exit status 0
  -exec CMD {} +             Run CMD with batches of paths
  -execdir CMD {} ;          Like -exec, run in the entry's directory with ./NAME
  -ok, -okdir CMD {} ;        Ask on stderr and execute after y/Y on stdin
  -fprint FILE, -fprint0 FILE  Write paths to FILE
  -fprintf FILE FORMAT        Write formatted output to FILE
  -fls FILE                   Write long listings to FILE
  --help, --version          Show help or version information

Operators: ( ) ! -not -a -and -o -or , (precedence as in GNU find).
Exit status: 0 on success, 1 when some entries below a starting point could not be read
or an action failed, invalid usage, or a starting point that does not exist.
";

pub fn find(args: &[String]) -> i32 {
    let expanded;
    let first = if let Some(arg) = args
        .first()
        .filter(|arg| arg.starts_with("--") && *arg != "--")
    {
        expanded = match crate::common::args::expand_long(arg, &["help", "version"]) {
            Ok(arg) => arg,
            Err(error) => return crate::common::error::usage("find", error, 1),
        };
        Some(expanded.as_str())
    } else {
        args.first().map(String::as_str)
    };
    if first == Some("--version") {
        return crate::common::args::help_version("find", &["--version".into()], HELP, true)
            .unwrap();
    }
    if matches!(first, Some("--help" | "-h")) {
        return crate::common::args::help_version("find", &["--help".into()], HELP, true).unwrap();
    }
    if let Some(arg) = first.filter(|arg| arg.starts_with("--") && arg.contains('=')) {
        return crate::common::error::usage(
            "find",
            format!(
                "option '{}' doesn't allow an argument",
                arg.split('=').next().unwrap()
            ),
            1,
        );
    }
    match parse::parse(args) {
        Ok(plan) => walk::run(plan),
        Err(error) => {
            eprintln!("find: {error}\nTry 'find --help' for more information.");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::find;

    #[test]
    fn rejects_unknown_predicates_with_usage_status() {
        let args = ["-bogus".to_string()];
        assert_eq!(find(&args), 1);
    }

    #[test]
    fn rejects_unbalanced_parentheses() {
        let args = ["(".to_string(), "-name".to_string(), "x".to_string()];
        assert_eq!(find(&args), 1);
    }
}
