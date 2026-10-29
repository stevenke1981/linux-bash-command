# agentcmd

A collection of standalone Rust command-line tools for coding agents. Each command is a separate executable and implements a documented subset of familiar Linux-style tools. Most inspect local data; `curl` makes HTTP(S) requests and can write a response to a file. These tools are not complete replacements for ripgrep, GNU utilities, or curl.

## Build

Prebuilt x86-64 packages for Windows and Linux are produced under `dist/` by the local packaging process; CI builds binaries for Windows, Linux, and macOS. Extract the package and add its extracted directory to your PATH. The macOS build has not been verified locally; the three-OS GitHub Actions workflow can build it on a macOS runner.

To build from source, install Rust and run:

```sh
cargo build --release --bins
cargo test
```

The executables are in `target/release/`, with `.exe` extensions on Windows: `search`, `diff`, `list`, `read`, `rg`, `ls`, `pwd`, `cd`, `cat`, `less`, `head`, `tail`, `grep`, `find`, `ps`, `df`, and `curl`. Each executable accepts its arguments directly; for example, run `search --help`. The previous combined `agentcmd` binary remains available for compatibility.

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

Use `--` before positional arguments that begin with a dash. Arguments must be valid UTF-8. Commands write result data to standard output and errors to standard error. Search returns **0** for matches, **1** for none and **2** for errors; diff returns **0** for equal files, **1** for differences and **2** for errors. Most other commands return **0** for success and **2** for errors; `curl -f` returns **22** for HTTP error responses. Search can emit partial results before reporting an error, so always check the exit status.

### Search

`search [OPTIONS] PATTERN [PATH...]` uses the Rust `regex` crate syntax, matches each line independently and defaults to the current directory. `-F` searches a literal string; `-i` ignores case. Results are `path:line:text` by default; `--no-line-number` omits the line number. Use `--json` for newline-delimited JSON objects with `path` (string), `line_number` (one-based integer), and `text` (string); JSON is preferable when paths or lines contain colons. `-g GLOB` filters traversed files and can be repeated; explicit file arguments bypass glob filtering.

Directory traversal skips hidden entries and honors ignore rules supplied by the `ignore` crate. `--hidden` includes hidden entries, while `--no-ignore` disables ignore files; use both to include both categories. Explicit files are searched regardless of these traversal filters. Traversal does not follow symlinks; an explicitly named symlink to a file is opened. Files containing a NUL byte are skipped; other invalid UTF-8 is reported as an error. Files are read fully into memory, and traversal result order is not guaranteed. `search` accepts `-` as an explicit standard-input path; `rg` also detects piped standard input when no path is given. Both load each input fully into memory.

### List

`list [OPTIONS] [PATH]` outputs sorted, `/`-separated paths relative to the requested directory. It skips dot-prefixed names and common generated directories (`target`, `node_modules`, `venv`, `__pycache__`, and version-control directories) by default. Unlike search, it does not interpret `.gitignore` files. `--all` includes dot-prefixed entries; `--no-ignore` includes common generated directories; `--dirs` also lists directories. `--max-depth N` limits traversal: direct children have depth 1, and depth 0 returns no entries. Symlinks are listed but not followed during traversal. For paths with newlines or non-UTF-8 names, this line-based output may be ambiguous or lossy.

### Read and diff

`read` accepts UTF-8 text and prints one-based line numbers. `--start` and `--end` are inclusive; `--limit` caps the number of displayed lines. A start beyond the end of a file produces no output. Lines are read incrementally and reading stops at the selected end or limit; a single long line can still consume substantial memory. Invalid UTF-8 beyond the selected range is not read or validated. Line terminators are normalized in output.

`diff` accepts two UTF-8 files and prints a unified line diff. `-U N` or `--context N` controls the number of context lines. Neither read nor diff supports standard input; missing or invalid UTF-8 input is an error. NUL bytes are accepted if the input is otherwise valid UTF-8. Like ordinary file opens, explicitly named symlinks are followed. Very large inputs can consume substantial memory.

### Additional commands

- `rg` shares the `search` engine, detects piped stdin when no path is specified, and supports `--files` with `-g`, `--hidden`, and `--no-ignore` for file discovery. `grep` reads files or stdin; its options include `-i`, `-n`, `-F`, `-v`, and `-c`. Both use Rust regex syntax and return 0 for a match, 1 for no match, and 2 for errors.
- `ls [-al] [PATH...]` lists one directory level; `-l` shows a cross-platform type, read-only flag, byte size, and modification time as Unix seconds, not the full POSIX long listing. `pwd` prints the current directory. `find [PATH...] -name '*.rs' -type f -maxdepth 3` supports `*` and `?` basename patterns and does not follow symlinks.
- `cat` copies files or stdin as bytes. `head -n 10` and `tail -n 10` work with files or stdin; their line counts default to 10. `tail -f` is not implemented.
- `less FILE` provides a basic interactive pager on a terminal; Space, Enter, b, g, G, and q navigate. `-N` adds line numbers. Piped input or redirected output is copied without interactive paging. It loads input into memory and clips long display lines.
- `ps [--json] [-p PID]` lists processes; it defaults to all processes visible to the OS. `df [-h] [PATH...]` lists filesystem sizes and available space. OS permissions and filesystem reporting can affect the results.
- `curl URL` supports HTTP(S), `-I`, `-X`, `-H`, `-d`, `-o`, `-L`, `-f`, `-s`, and `-m`. Redirects are followed only with `-L`; `-f` returns 22 for HTTP errors. It does not support all curl protocols or options. `-d` and custom methods may change server state, and `-o` writes the chosen local file.

Use `--help` on each command for the implemented options. Standard input is selected by `-` for `cat`, `head`, `tail`, `grep`, `search`, and `rg`.

## Development

Automated CLI tests use temporary directories and cover common success and error paths.
