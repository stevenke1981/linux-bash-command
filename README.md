# agentcmd

## Colors and terminal formatting

Each command colors and formats its own output, like GNU coreutils, GNU grep
and ripgrep do. Nothing needs to wrap the command:

| Command | On a terminal |
|---|---|
| `ls` | Names colored by type (`LS_COLORS` honored, GNU defaults otherwise); entries laid out in columns |
| `grep`, `egrep`, `fgrep` | Matches, file names, line numbers and separators colored (`GREP_COLORS` honored) |
| `rg`, `search` | Matches, paths and line numbers colored (`--colors` honored); matches grouped under file headings with line numbers |
| `diff` | Headers bold, hunks cyan, removed lines red, added lines green |
| `df`, `ps` | Bold header row; `df` colors `Use%` green/yellow/red |

Whether a command colors its output is decided in this order:

1. The command's own `--color[=WHEN]` option (`always`, `never`, `auto`), where it has one.
2. `AGENTCMD_COLOR=always|never|auto` in the environment.
3. `auto`, or `never` when `NO_COLOR` is set.

`auto` colors only when stdout is a terminal and `TERM` is not `dumb`. Piped
or captured output, which is what coding agents read, stays plain and
byte-for-byte unchanged, and so does the terminal-only layout (`ls` columns, `rg`
headings). Column width comes from `COLUMNS`, then the terminal size, then 80.
On Windows, console virtual-terminal processing is enabled automatically, and
`auto` stays plain if the console cannot enable it.

```bash
ls                                  # colored columns in a terminal
ls --color=always | less -R         # keep colors through a pipe
grep --color=never -n TODO src/*.rs
AGENTCMD_COLOR=never rg fn src      # turn colors off for every command
```

A collection of standalone Rust command-line tools for coding agents. Each command is a separate executable and implements a documented subset of familiar Linux-style tools. Most inspect local data; `curl` makes HTTP(S) requests and can write a response to a file. These tools are not complete replacements for ripgrep, GNU utilities, or curl.

## Build

Prebuilt x86-64 packages for Windows and Linux are produced under `dist/` by the local packaging process; CI builds binaries for Windows, Linux, and macOS. Extract the package and add its extracted directory to your PATH. The macOS build has not been verified locally; the three-OS GitHub Actions workflow can build it on a macOS runner.

To build from source, install Rust and run:

```sh
cargo build --release --bins
cargo test
```

The executables are in `target/release/`, with `.exe` extensions on Windows: `search`, `diff`, `list`, `read`, `rg`, `ls`, `pwd`, `cd`, `cat`, `less`, `head`, `tail`, `grep`, `find`, `ps`, `df`, and `curl`. Each executable accepts its arguments directly; for example, run `search --help`. The `agentcmd` multi-call binary also runs each command as `agentcmd <command> ...`; `agentcmd --list` lists available commands. Calling `agentcmd` without arguments now reports an error and exits 1. A copy named after a command dispatches to that command.

Add the release directory to your shell's PATH to call the executables by name. For the current PowerShell session:

```powershell
$env:PATH = "$(Join-Path (Get-Location) 'target\release');$env:PATH"
search.exe --help
diff.exe old.txt new.txt
list.exe .
read.exe --limit 20 README.md
```

In PowerShell, names such as `ls`, `cat`, `curl`, `pwd`, and `diff` may resolve to aliases; use `.exe`, for example `curl.exe`, to select these executables. In Bash, `read`, `pwd`, and `cd` can be builtins; use `env read`, `env pwd`, or an explicit executable path where needed. When an executable has the same name as an existing command, PATH order determines which binary is selected.

### Changing directories

An external `cd` process cannot change the parent shell's working directory. The standalone `cd` executable checks a directory and prints its absolute path; activate the included shell function to make `cd` change your current shell. Put the built binaries first on PATH, then:

```bash
source /path/to/agent-commands/shell/cd.bash
cd /some/directory
```

```powershell
. .\shell\cd.ps1
cd .\src
```

For an extracted package, source the `shell/cd.bash` or `shell/cd.ps1` file inside that extracted directory instead. These integrations change `cd` only in the current shell session. The Bash wrapper requires Bash; the PowerShell wrapper replaces its `cd` alias in that session. `cd -` requires `OLDPWD` to be set.

## Commands

```sh
search 'fn\s+main' src
search -F -i 'todo' .
search --json -g '*.rs' 'error' src
list --max-depth 2 src
list --all --no-ignore .
env read --start 20 --end 50 src/main.rs
env read --start 20 --limit 30 src/main.rs
diff --context 3 old.txt new.txt
```

Use `--` before positional arguments that begin with a dash. Arguments must be valid UTF-8. Commands write result data to standard output and errors to standard error. Search returns **0** for matches, **1** for none and **2** for errors; diff returns **0** for equal files, **1** for differences and **2** for errors. `cat`, `cd`, `df`, `find`, `head`, `less`, `ps`, `pwd`, and `tail` return **1** for errors; `ls` returns **1** for minor errors and **2** for serious errors; the new Phase 1 utilities normally return **1** for errors (`sort`, `test`, `[` and grep use **2** for usage errors); `list` and `read` return **2** for errors; `curl -f` returns **22** for HTTP error responses. Search can emit partial results before reporting an error, so always check the exit status.

### Phase 1 text pipelines

The multi-call registry includes 115 names: the original 17, these 29 Phase 1 names, 32 Phase 2 names, 13 Phase 3 names, 16 Phase 4 names, and 8 Phase 5 names:

`wc sort uniq cut tr tee echo printf true false test [ seq tac nl paste comm fold expand unexpand yes basename dirname xargs env printenv sleep egrep fgrep`.

Each has a standalone executable, except `[`, which is a copy or symlink of `test` in packages. `egrep` and `fgrep` select extended and fixed-string grep. `head -z` and `tail -z` process NUL-delimited records; `cat -u` is accepted. Grep additionally supports NUL records and filenames, byte offsets, initial tabs, stdin labels, exclude files, and directory/device actions. Use each command's `--help` for the supported options.

Examples:

```sh
agentcmd seq 1 5
agentcmd cut -d, -f2 data.csv
agentcmd sort -t, -k2,2n data.csv
agentcmd env -i PATH=/usr/bin:/bin printenv PATH
```

Phase 1 data processing uses byte order and ASCII classes in the C locale. `wc -m` explicitly counts UTF-8 characters, including one character per invalid byte. `seq` uses exact decimal steps unless scientific notation or a custom format requires floating point. `printf`, `seq`, and `sort -g` use `f64`. `sort` spills to a stable external merge when its 256 MiB memory budget (or `-S`) is exceeded; `-T` and `--batch-size` control its temporary storage and merge fan-in. Broken output pipes exit silently with status 0. The full specification and documented exceptions are in [Linux commands](docs/spec/linux-commands/README.md) and its [known differences](docs/spec/linux-commands/07-phases.md).

On Windows, `echo`, `tee`, `sort`, and `sleep` can conflict with PowerShell commands, while `expand` and `sort` also conflict with System32 executables. Use `agentcmd <name>` or an explicit executable path to select this implementation.

### Filesystem, checksums and system information

Phase 2 adds `mkdir rm rmdir cp mv touch ln link unlink realpath readlink stat du
mktemp date md5sum sha1sum sha224sum sha256sum sha384sum sha512sum b2sum cksum
sum base64 base32 basenc truncate nproc hostname arch uptime`.

Run `agentcmd <name> --help` for the supported options. Filesystem commands accept
Unix paths on every platform. Windows permissions use 0644 for files and 0755 for
directories or executable extensions, with write bits removed for read-only files.
Creating symlinks on Windows requires Developer Mode or an elevated shell.
`link.exe` is excluded from Windows packages because it conflicts with the MSVC
linker; use `agentcmd link`. `hostname` and `uptime` also overlap Windows commands.

Checksums and base encodings stream input. `date` uses bundled timezone data;
Windows system information omits unavailable load-average and login-user fields.
`hostname HOSTNAME` is deliberately rejected as an unprivileged operation.
See the specification's known differences for platform details and date limits.

### Search

`search [OPTIONS] PATTERN [PATH...]` uses the Rust `regex` crate syntax, matches each line independently and defaults to the current directory. `-F` searches a literal string; `-i` ignores case. Results are `path:line:text` by default; `--no-line-number` omits the line number. Use `--json` for newline-delimited JSON objects with `path` (string), `line_number` (one-based integer), and `text` (string); JSON is preferable when paths or lines contain colons. `-g GLOB` filters traversed files and can be repeated; explicit file arguments bypass glob filtering.

Directory traversal skips hidden entries and honors ignore rules supplied by the `ignore` crate. `--hidden` includes hidden entries, while `--no-ignore` disables ignore files; use both to include both categories. Explicit files are searched regardless of these traversal filters. Traversal does not follow symlinks; an explicitly named symlink to a file is opened. Files containing a NUL byte are skipped; other invalid UTF-8 is reported as an error. Files are read fully into memory, and traversal result order is not guaranteed. `search` accepts `-` as an explicit standard-input path; `rg` also detects piped standard input when no path is given. Both load each input fully into memory.

### List

`list [OPTIONS] [PATH]` outputs sorted, `/`-separated paths relative to the requested directory. It skips dot-prefixed names and common generated directories (`target`, `node_modules`, `venv`, `__pycache__`, and version-control directories) by default. Unlike search, it does not interpret `.gitignore` files. `--all` includes dot-prefixed entries; `--no-ignore` includes common generated directories; `--dirs` also lists directories. `--max-depth N` limits traversal: direct children have depth 1, and depth 0 returns no entries. Symlinks are listed but not followed during traversal. For paths with newlines or non-UTF-8 names, this line-based output may be ambiguous or lossy.

### Read and diff

`read` accepts UTF-8 text and prints one-based line numbers. `--start` and `--end` are inclusive; `--limit` caps the number of displayed lines. A start beyond the end of a file produces no output. Lines are read incrementally and reading stops at the selected end or limit; a single long line can still consume substantial memory. Invalid UTF-8 beyond the selected range is not read or validated. Line terminators are normalized in output.

`diff` compares two files (or `-` for stdin) and prints normal format by default. Use `-u` for unified output, `-c`/`-C N` for context output, or `-y`/`-W N` for side-by-side output. It supports `-u`, `-U N`, `--unified[=N]`, `-q`, `-s`, `-r`, `-N`, `-w`, `-b`, `-B`, `-i`, `--strip-trailing-cr`, `-a`, and `--label`. Two directory operands compare their direct entries; `-r` also descends into common subdirectories. A file and a directory compare the file with the entry of the same name inside the directory. Non-UTF-8 or NUL-containing files are reported as `Binary files A and B differ` unless `-a`/`--text` is used, which compares and prints their original bytes. Case folding and whitespace handling use ASCII rules, as in the C locale. Very large inputs can consume substantial memory.

### Additional commands

These commands accept the GNU, BSD, and ripgrep options that coding agents such as Claude Code typically use: bundled short flags (`-rn`, `-sSL`, `-lah`), attached values (`-A3`, `-m1`), and `--opt=value`. `--color[=WHEN]` works as described in [Colors and terminal formatting](#colors-and-terminal-formatting). On Windows, printed paths use `/` so they can be pasted back into Bash. Git for Windows aliases `ls` and `grep` to `ls -F --color=auto --show-control-chars` and `grep --color=auto`; both forms work. Unsupported options fail with exit status 2 and a message instead of producing different output.

- `grep` defaults to GNU basic regular expressions (`a\|b`, `\(x\)`), with `-E`, `-F`, and `-P` (Rust regex; no lookaround or backreferences). It supports `-r`/`-R` with `--include`, `--exclude`, `--exclude-dir`; `-i -v -w -x -o -n -c -l -L -q -s -H -h -m`; `-A/-B/-C/-NUM` context; repeated `-e` and `-f FILE`; binary file detection with `-a` and `-I`.
- `rg` shares the `search` engine and supports `-l`, `-c`, `--count-matches`, `-o`, `-q`, `-A/-B/-C`, `-t/-T`, `-g` (including `!` negation), `--iglob`, `-U`, `-w -x -v -S -s -i -F`, `-m`, `-M/--max-columns`, `-d`, `--max-filesize`, `-L`, `--sort/--sortr` (including with `--files`), `--column`, `-0`, `--hidden`, `-u/-uu/-uuu`, and `--no-ignore`. `-P` uses Rust regex syntax.
- `ls` supports `-a -A -l -h -F -p -R -t -S -r -d -1 -x -U -v -G -o -g -n`, `--group-directories-first`, and `--time-style`. `-h` means human-readable sizes; use `--help` for help. `-l` prints a GNU-like line with mode, link count, size, and UTC modification time; owner and group use native identifiers and names where available. `pwd` accepts `-L` and `-P`.
- `find` evaluates GNU-style expressions with `!`/`-not`, `-a`, `-o`, and parentheses. Tests include `-name`, `-iname`, `-path`, `-ipath`, `-regex` (Rust syntax), `-type`, `-empty`, `-size`, `-newer`, `-mtime`, and `-mmin`. Actions include `-print`, `-print0`, `-printf`, `-ls`, `-prune`, `-quit`, `-delete`, `-exec`, and `-execdir`. `-maxdepth`, `-mindepth`, `-depth`, and `-L` are also supported. Results are listed in pre-order with entries sorted by name.
- `cat` supports `-n -b -s -E -T -v -A -e -t`. `head` and `tail` support `-n`, `-c`, `-NUM`, `-q`, and `-v`. `head -n -N` prints everything except the last N lines, and `tail -n +N` starts at line N. `tail -f`/`-F` follow files, with `--pid` and `-s`.
- `less FILE` provides a basic interactive pager on a terminal; Space, Enter, b, g, G, and q navigate. `-N` adds line numbers, and common flags such as `-R -S -F -X` are accepted. Piped input or redirected output is copied without interactive paging.
- `ps` lists processes in its compact default format and also accepts `aux`, `-ef`, `-W`, `-o FIELDS`, `-p`, `-u`, `-C`, `--sort`, `--no-headers`, and `--json`. `df` supports `-h -H -k -m -B -T -P -l -t -x` and `--output`. TTY and CPU time are not available and are shown as `?`.
- `curl URL` supports HTTP(S) with bundled flags (`-fsSL`), `-I -i -v -k -X -H -A -e -b -u`, `-d`/`--data-raw`/`--data-binary`/`--data-urlencode`/`--json`/`-G`, `-o`/`-O`/`--create-dirs`/`-D`, `-w` write-out variables, `-L`/`--max-redirs`, `-f`/`--fail-with-body` (exit 22), `-m`/`--connect-timeout`, `--retry`, and `-x` proxies. Multipart `-F` is rejected. Response header names are printed in lowercase, and `--compressed` does not decode responses. `-d` and custom methods may change server state, and `-o` writes the chosen local file.

Use `--help` on each command for the implemented options. Standard input is selected by `-` for `cat`, `head`, `tail`, `grep`, `search`, and `rg`.

## Development

Automated CLI tests use temporary directories and cover common success and error paths.

### Additional data and formatting commands

`join od split csplit fmt pr shuf tsort numfmt factor expr pathchk ptx` are available through the registry and standalone executables. Duplicate join groups and permuted indexes spill to private temporary files. `factor` supports u128 values using certified prime factors; `expr` uses checked i128 arithmetic. See each command's help for options and the known-differences table for required GNU exceptions. Attribution for the MIT-licensed formatting components is in [NOTICE.md](NOTICE.md).

### Additional Linux tools

`sed which tree uname whoami timeout kill pgrep pkill free cmp rev column dos2unix unix2dos chmod` are available as standalone commands and through `agentcmd`.

**Diff format change:** `diff` now defaults to GNU normal format. Use `diff -u` for unified output; `-c`/`-C` select context and `-y`/`-W` select side-by-side output.

On Windows, `tree`, `whoami`, `timeout` and `kill` conflict with system executables or PowerShell commands. Use `agentcmd NAME` or the executable's full path to select this implementation. Unix permission bits map to the Windows read-only attribute for chmod; signal delivery terminates the selected process, while timeout uses a Job Object for its owned process tree.


### Programs, archives and binary inspection

Phase 5 adds `awk tar gzip gunzip zcat patch xxd file`. AWK supports records,
arrays, functions, arithmetic, regular expressions, and file redirection;
command pipes and `system()` are deliberately unsupported. Function-call
nesting is limited to 1024; regex compilation has explicit resource budgets.
Use each command's help and the specification for the supported subset.

Gzip handles concatenated members and validates trailers. Tar supports plain
and gzip archives and rejects extraction through symlinks or parent components.
Patch applies unified and Git text patches with atomic file installation. Xxd
dumps and reverses bytes; file identifies a documented signature/text subset.
Windows also supplies a `tar.exe`; select this implementation with `agentcmd tar`
or its explicit executable path.
