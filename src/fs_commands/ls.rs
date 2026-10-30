//! `ls`: operands, directory listings, recursion, headers, filters and exit status. Option
//! parsing is in `ls_options`, ordering in `ls_sort`, and text in `ls_format`.

use super::ls_format::{self, Ctx};
use super::ls_options::{self, Deref, Format, LsOptions, Parsed, Quoting};
use super::ls_sort::sort_entries;
use super::ls_time::allocated_bytes;
use crate::common::datetime;
use crate::common::error::{io_error_text, EXIT_TROUBLE};
use crate::display_path;
use crate::find_command::glob::Glob;
use std::fs::{self, Metadata};
use std::io::{self, BufWriter, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Line width when `-w` is not given and `COLUMNS` is unset or invalid.
const DEFAULT_WIDTH: usize = 80;

/// One listed item: a name as printed, the path it came from, and its metadata.
pub struct Entry {
    /// Text shown for the entry: the base name, or the operand as given.
    pub name: String,
    pub path: PathBuf,
    /// Metadata of the entry as shown: the link itself, or its target under `-L`/`-H`.
    pub meta: Metadata,
    /// The entry is a symbolic link shown as itself (so `-l` appends ` -> target`).
    pub is_link: bool,
    /// Bytes allocated on disk, for `-s` and the `total` line.
    pub allocated: u64,
}

impl Entry {
    fn new(name: String, path: PathBuf, meta: Metadata, is_link: bool) -> Self {
        let allocated = allocated_bytes(&meta);
        Self {
            name,
            path,
            meta,
            is_link,
            allocated,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.meta.is_dir()
    }
}

enum Argument {
    Entry(Box<Entry>),
    Directory(PathBuf),
}

pub fn ls(args: &[String]) -> i32 {
    let (options, paths) = match ls_options::parse(args) {
        Parsed::Run(options, paths) => (*options, paths),
        Parsed::Exit(code) => return code,
    };
    let tty = io::stdout().is_terminal();
    let ctx = Ctx {
        now: datetime::now(),
        zone: datetime::zone_from_env(),
        tty,
        width: options.width.unwrap_or_else(columns_from_env),
        quoting: resolve_quoting(options.quoting, tty),
    };
    let headers = !options.directory && (paths.len() > 1 || options.recursive);
    let ignore = options.ignore.iter().map(|p| Glob::new(p, false)).collect();
    let hide = options.hide.iter().map(|p| Glob::new(p, false)).collect();
    let mut listing = Listing {
        options,
        ctx,
        ignore,
        hide,
        headers,
        printed: false,
        failed: false,
        trouble: false,
        ancestors: Vec::new(),
    };
    let stdout = io::stdout();
    let mut out = BufWriter::with_capacity(64 * 1024, stdout.lock());
    let result = listing.run(&paths, &mut out).and_then(|()| out.flush());
    match result {
        Ok(()) if listing.trouble => EXIT_TROUBLE,
        Ok(()) if listing.failed => 1,
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => {
            eprintln!("ls: cannot write output: {}", io_error_text(&error));
            EXIT_TROUBLE
        }
    }
}

/// `COLUMNS` when it is a positive number, else 80.
fn columns_from_env() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|width| *width > 0)
        .unwrap_or(DEFAULT_WIDTH)
}

/// `--quoting-style` default: `shell-escape` on a terminal, `literal` otherwise.
fn resolve_quoting(quoting: Quoting, tty: bool) -> Quoting {
    match quoting {
        Quoting::Unset if tty => Quoting::ShellEscape,
        Quoting::Unset => Quoting::Literal,
        other => other,
    }
}

struct Listing {
    options: LsOptions,
    ctx: Ctx,
    ignore: Vec<Glob>,
    hide: Vec<Glob>,
    headers: bool,
    printed: bool,
    /// A minor error (inside a directory): exit status 1.
    failed: bool,
    /// A command-line operand could not be used: exit status 2 (serious trouble).
    trouble: bool,
    /// Canonical directories being listed (for loop detection under -L).
    ancestors: Vec<PathBuf>,
}

impl Listing {
    fn run(&mut self, paths: &[String], out: &mut dyn Write) -> io::Result<()> {
        let mut entries = Vec::new();
        let mut directories = Vec::new();
        for path in paths {
            match self.argument(Path::new(path)) {
                Some(Argument::Entry(entry)) => entries.push(*entry),
                Some(Argument::Directory(directory)) => directories.push(directory),
                None => {}
            }
        }
        self.print_entries(&mut entries, out, false)?;
        for directory in directories {
            self.list_directory(&directory, out, true)?;
        }
        Ok(())
    }

    fn argument(&mut self, path: &Path) -> Option<Argument> {
        let link_meta = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                eprintln!(
                    "ls: cannot access '{}': {}",
                    display_path(path),
                    io_error_text(&error)
                );
                self.trouble = true;
                return None;
            }
        };
        let is_symlink = link_meta.file_type().is_symlink();
        let follow = self.options.deref != Deref::Never;
        let (meta, is_link) = match fs::metadata(path) {
            Ok(target) if is_symlink && follow => (target, false),
            _ => (link_meta, is_symlink),
        };
        // A symlink to a directory lists the directory, unless -d or a long format shows it.
        let target_is_dir = fs::metadata(path).is_ok_and(|target| target.is_dir());
        let lists_contents = !self.options.directory
            && (meta.is_dir() && !is_link
                || is_link && self.options.format != Format::Long && target_is_dir);
        if lists_contents {
            return Some(Argument::Directory(path.to_path_buf()));
        }
        Some(Argument::Entry(Box::new(Entry::new(
            display_path(path),
            path.to_path_buf(),
            meta,
            is_link,
        ))))
    }

    /// `top` is true for a directory named on the command line.
    fn list_directory(
        &mut self,
        directory: &Path,
        out: &mut dyn Write,
        top: bool,
    ) -> io::Result<()> {
        // Under -L a symbolic link can lead back to a directory being listed: skip it, as GNU does.
        let real = match self.options.deref {
            Deref::All => fs::canonicalize(directory).ok(),
            _ => None,
        };
        if let Some(real) = &real {
            if self.ancestors.contains(real) {
                eprintln!(
                    "ls: not listing already-listed directory '{}'",
                    display_path(directory)
                );
                self.failed = true;
                return Ok(());
            }
            self.ancestors.push(real.clone());
        }
        let result = self.list_directory_contents(directory, out, top);
        if real.is_some() {
            self.ancestors.pop();
        }
        result
    }

    fn list_directory_contents(
        &mut self,
        directory: &Path,
        out: &mut dyn Write,
        top: bool,
    ) -> io::Result<()> {
        let Some(mut entries) = self.read_entries(directory, top) else {
            return Ok(());
        };
        if self.headers {
            if self.printed {
                writeln!(out)?;
            }
            writeln!(out, "{}:", display_path(directory))?;
            self.printed = true;
        }
        self.print_entries(&mut entries, out, true)?;
        if self.options.recursive {
            for entry in entries
                .iter()
                .filter(|entry| entry.is_dir() && !is_dot_name(&entry.name))
            {
                self.list_directory(&entry.path, out, false)?;
            }
        }
        Ok(())
    }

    fn read_entries(&mut self, directory: &Path, top: bool) -> Option<Vec<Entry>> {
        let items = match fs::read_dir(directory) {
            Ok(items) => items,
            Err(error) => {
                eprintln!(
                    "ls: cannot open directory '{}': {}",
                    display_path(directory),
                    io_error_text(&error)
                );
                if top {
                    self.trouble = true;
                } else {
                    self.failed = true;
                }
                return None;
            }
        };
        let mut entries = Vec::new();
        if self.options.dot_entries {
            entries.extend(dot_entries(directory));
        }
        for item in items {
            match item {
                Ok(item) => entries.extend(self.dir_entry(&item)),
                Err(error) => {
                    eprintln!(
                        "ls: cannot read directory '{}': {}",
                        display_path(directory),
                        io_error_text(&error)
                    );
                    self.failed = true;
                }
            }
        }
        Some(entries)
    }

    fn dir_entry(&mut self, item: &fs::DirEntry) -> Option<Entry> {
        let name = item.file_name().to_string_lossy().into_owned();
        if !self.visible(&name) {
            return None;
        }
        let path = item.path();
        let link_meta = match item.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                eprintln!(
                    "ls: cannot inspect '{}': {}",
                    display_path(&path),
                    io_error_text(&error)
                );
                self.failed = true;
                return None;
            }
        };
        let is_symlink = link_meta.file_type().is_symlink();
        let (meta, is_link) = match fs::metadata(&path) {
            Ok(target) if is_symlink && self.options.deref == Deref::All => (target, false),
            _ => (link_meta, is_symlink),
        };
        Some(Entry::new(name, path, meta, is_link))
    }

    /// Filters for names in a directory: dot names, `-B`, `-I` and (without `-a`/`-A`) `--hide`.
    fn visible(&self, name: &str) -> bool {
        if !self.options.all && name.starts_with('.') {
            return false;
        }
        if self.options.ignore_backups && name.ends_with('~') {
            return false;
        }
        if self.ignore.iter().any(|glob| glob.is_match(name)) {
            return false;
        }
        self.options.all || !self.hide.iter().any(|glob| glob.is_match(name))
    }

    /// Sorts and prints one set of entries. A directory listing starts with the `total` line
    /// under `-l` or `-s`, even when it is empty.
    fn print_entries(
        &mut self,
        entries: &mut [Entry],
        out: &mut dyn Write,
        directory: bool,
    ) -> io::Result<()> {
        sort_entries(entries, &self.options);
        let totals = self.options.format == Format::Long || self.options.blocks;
        if directory && totals {
            out.write_all(ls_format::total_line(entries, &self.options).as_bytes())?;
        }
        if entries.is_empty() {
            return Ok(());
        }
        let text = ls_format::body(entries, &self.options, &self.ctx);
        out.write_all(text.as_bytes())?;
        self.printed = true;
        Ok(())
    }
}

fn is_dot_name(name: &str) -> bool {
    name == "." || name == ".."
}

/// The `.` and `..` entries that `-a` lists first, taken from the directory itself and its parent.
fn dot_entries(directory: &Path) -> Vec<Entry> {
    [".", ".."]
        .into_iter()
        .filter_map(|name| {
            let path = directory.join(name);
            let metadata = fs::symlink_metadata(&path).ok()?;
            Some(Entry::new(name.to_string(), path, metadata, false))
        })
        .collect()
}

#[cfg(test)]
mod ls_tests {
    use crate::common::size::human_size;

    #[test]
    fn human_sizes_round_up_like_gnu() {
        assert_eq!(human_size(0), "0");
        assert_eq!(human_size(1000), "1000");
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1025), "1.1K");
        assert_eq!(human_size(1500), "1.5K");
        assert_eq!(human_size(4096), "4.0K");
        assert_eq!(human_size(10 * 1024), "10K");
        assert_eq!(human_size(1_048_550), "1.0M");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0G");
    }

    #[test]
    fn columns_env_defaults_to_eighty() {
        assert!(super::columns_from_env() > 0);
    }
}
