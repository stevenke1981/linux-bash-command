//! `find`: a GNU-style expression parser and evaluator for file tree searches.
//!
//! The implementation is split into submodules: `parse` (arguments to expression tree),
//! `expr` (tree types and tests), `walk` (traversal and actions), `printf` (`-printf` and
//! `-ls` output), `exec` (command spawning) and `glob` (wildcards).

mod exec;
mod expr;
pub(crate) mod glob;
mod owner;
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
  -xdev, -mount, -noleaf,
  -ignore_readdir_race       Accepted; mount boundaries are not enforced
  -regextype TYPE            posix-basic (BRE) or posix-extended, emacs, posix-awk
                             (ERE syntax) for later -regex and -iregex

Tests:
  -name PAT, -iname PAT      Basename matches a wildcard (* ? [...] \\ escape)
  -path PAT, -ipath PAT      Whole path matches a wildcard (also -wholename)
  -regex RE, -iregex RE      Whole path matches a regular expression (Rust regex syntax)
  -type f|d|l[,...]          File type; b c p s are accepted on Unix only
  -xtype f|d|l[,...]         Like -type, but a symbolic link is classified by its target
  -empty                     Empty regular file or directory
  -size [+-]N[cwbkMG]        Size; sizes round up to whole units (default 512-byte blocks)
  -newer FILE                Modified later than FILE (also -anewer, -cnewer)
  -newerXY REF               X: a, B, c, m (the entry); Y: a, B, c, m (REF), or t (REF is
                             a date string)
  -mtime/-atime/-ctime [+-]N Age in whole days (-mmin/-amin/-cmin in minutes)
  -perm [-/]MODE             Octal mode (Unix only)
  -user NAME|UID, -group NAME|GID
                             Owner or group by name or number ('-' on Windows)
  -uid N, -gid N             Owner or group by number
  -nouser, -nogroup          Owner or group has no account entry
  -links [+-]N               Hard-link count
  -inum N, -samefile FILE    Same inode number (or the same file); Unix only
  -lname PAT, -ilname PAT    Symbolic link target matches a wildcard
  -readable, -writable, -executable
                             Best-effort access checks
  -true, -false

Actions:
  -print, -print0, -ls       Print the path (default action when none is given)
  -fprint FILE, -fprint0 FILE, -fls FILE
                             Like -print, -print0 and -ls, written to FILE
  -printf FORMAT             Formatted output: %p %P %f %h %s %d %y %m %T@ and escapes
                             \\n \\t \\0 \\\\ (other %T letters are rejected)
  -fprintf FILE FORMAT       Like -printf, written to FILE (FILE comes first)
  -prune                     Do not descend into the directory
  -quit                      Stop after the current entry
  -delete                    Remove entries (implies -depth; '.' is kept)
  -exec CMD {} ;             Run CMD per entry; success means exit status 0
  -exec CMD {} +             Run CMD with batches of paths
  -execdir CMD {} ;          Like -exec, run in the entry's directory with ./NAME
  -ok CMD {} ;               Like -exec, after asking '< CMD > ? ' and reading y or Y
  -okdir CMD {} ;            Like -ok, run in the entry's directory with ./NAME

Operators: ( ) ! -not -a -and -o -or , (precedence as in GNU find).
Exit status: 0 on success, 1 when some entries below a starting point could not be read
or an action failed, 1 on invalid usage or a starting point that does not exist.
";

pub fn find(args: &[String]) -> i32 {
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        print!("{HELP}");
        return 0;
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
