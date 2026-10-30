//! Directory traversal and action execution for find.
//!
//! Entries are visited in pre-order (or post-order with `-depth`). Each directory's
//! children are sorted by name, so output is deterministic. Nothing is sorted globally.

use super::exec;
use super::expr::{self, file_name_text, now_ns, Action, Entry, Expr};
use super::parse::{BatchSpec, Follow, Options, Plan};
use super::printf;
use crate::display_path;
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io::{self, BufWriter, StdoutLock, Write};
use std::path::{Path, PathBuf};

/// Queued `{} +` paths are flushed once this many bytes are pending. This keeps command
/// lines well below the Windows 32K limit.
const BATCH_BYTES: usize = 16 * 1024;

/// Pending paths for one `{} +` command.
struct Batch {
    argv: Vec<String>,
    in_dir: bool,
    pending: Vec<String>,
    pending_bytes: usize,
    directory: Option<PathBuf>,
}

impl Batch {
    fn new(spec: &BatchSpec) -> Self {
        Self {
            argv: spec.argv.clone(),
            in_dir: spec.in_dir,
            pending: Vec::new(),
            pending_bytes: 0,
            directory: None,
        }
    }
}

struct Walker<'a> {
    expr: &'a Expr,
    options: Options,
    now_ns: i128,
    out: BufWriter<StdoutLock<'static>>,
    batches: Vec<Batch>,
    /// Canonical paths of directories on the current traversal stack (for loop checks).
    ancestors: Vec<PathBuf>,
    status: i32,
    stop: bool,
    pruned: bool,
}

/// Status for a starting point that does not exist. GNU find uses 1 here, as for other
/// failures; unreadable entries below a root also give 1.
const MISSING_ROOT_STATUS: i32 = 1;

pub fn run(plan: Plan) -> i32 {
    let follow_roots = plan.options.follow != Follow::Never;
    let mut walker = Walker::new(&plan);
    let mut missing_root = false;
    for root in &plan.roots {
        if let Err(error) = metadata(root, follow_roots) {
            walker.report(root, &error);
            missing_root = true;
            continue;
        }
        walker.visit(root, root, 0, follow_roots);
    }
    let status = walker.finish();
    if missing_root {
        MISSING_ROOT_STATUS
    } else {
        status
    }
}

/// GNU-style text for an I/O error (see `common::error::io_error_text`).
pub fn message(error: &io::Error) -> String {
    crate::common::error::io_error_text(error)
}

fn metadata(path: &Path, follow: bool) -> io::Result<Metadata> {
    if follow {
        match fs::metadata(path) {
            Ok(meta) => return Ok(meta),
            // A dangling link is still an entry: fall back to the link itself.
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            Err(_) => {}
        }
    }
    fs::symlink_metadata(path)
}

fn parent_or_dot(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

impl<'a> Walker<'a> {
    fn new(plan: &'a Plan) -> Self {
        Self {
            expr: &plan.expr,
            options: plan.options,
            now_ns: now_ns(),
            out: BufWriter::new(io::stdout().lock()),
            batches: plan.batches.iter().map(Batch::new).collect(),
            ancestors: Vec::new(),
            status: 0,
            stop: false,
            pruned: false,
        }
    }

    fn visit(&mut self, path: &Path, root: &Path, depth: usize, follow: bool) {
        if self.stop {
            return;
        }
        let meta = match metadata(path, follow) {
            Ok(meta) => meta,
            Err(error) => {
                self.report(path, &error);
                return;
            }
        };
        let is_dir = meta.is_dir();
        let canonical = if follow && is_dir {
            fs::canonicalize(path).ok()
        } else {
            None
        };
        if let Some(real) = &canonical {
            if self.ancestors.contains(real) {
                eprintln!("find: '{}': File system loop detected", display_path(path));
                self.status = 1;
                return;
            }
        }
        let pushed = canonical.is_some();
        if let Some(real) = canonical {
            self.ancestors.push(real);
        }

        let depth_first = self.options.depth_first;
        let pruned = !depth_first && self.evaluate(path, root, depth, &meta);
        if is_dir && depth < self.options.max_depth && !pruned {
            self.visit_children(path, root, depth);
        }
        if depth_first {
            self.evaluate(path, root, depth, &meta);
        }
        if pushed {
            self.ancestors.pop();
        }
    }

    fn visit_children(&mut self, path: &Path, root: &Path, depth: usize) {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                self.report(path, &error);
                return;
            }
        };
        let mut children: Vec<(OsString, PathBuf)> = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => children.push((entry.file_name(), entry.path())),
                Err(error) => self.report(path, &error),
            }
        }
        children.sort_by(|left, right| left.0.cmp(&right.0));
        let follow = self.options.follow == Follow::Always;
        for (_, child) in children {
            if self.stop {
                break;
            }
            self.visit(&child, root, depth + 1, follow);
        }
    }

    /// Evaluates the expression for one entry. Returns true when `-prune` was applied.
    fn evaluate(&mut self, path: &Path, root: &Path, depth: usize, meta: &Metadata) -> bool {
        if depth < self.options.min_depth {
            return false;
        }
        self.pruned = false;
        let entry = Entry {
            path,
            root,
            depth,
            meta,
            now_ns: self.now_ns,
        };
        let expr = self.expr;
        self.eval(expr, &entry);
        std::mem::take(&mut self.pruned)
    }

    fn eval(&mut self, expr: &Expr, entry: &Entry) -> bool {
        match expr {
            Expr::Const(value) => *value,
            Expr::Not(inner) => !self.eval(inner, entry),
            Expr::And(left, right) => self.eval(left, entry) && self.eval(right, entry),
            Expr::Or(left, right) => self.eval(left, entry) || self.eval(right, entry),
            Expr::Comma(left, right) => {
                self.eval(left, entry);
                self.eval(right, entry)
            }
            Expr::Test(test) => expr::test_matches(test, entry),
            Expr::Action(action) => self.act(action, entry),
        }
    }

    fn act(&mut self, action: &Action, entry: &Entry) -> bool {
        match action {
            Action::Print => self.write(&format!("{}\n", display_path(entry.path))),
            Action::Print0 => self.write(&format!("{}\0", display_path(entry.path))),
            Action::Printf(pieces) => self.write(&printf::render(pieces, entry)),
            Action::Ls => self.write(&format!("{}\n", printf::long_listing(entry))),
            Action::Prune => {
                self.pruned = true;
                true
            }
            Action::Quit => {
                self.stop = true;
                true
            }
            Action::Delete => self.delete(entry),
            Action::Exec { argv, in_dir } => self.run_now(argv, *in_dir, entry),
            Action::Batch(index) => self.queue(*index, entry),
        }
    }

    fn write(&mut self, text: &str) -> bool {
        match self.out.write_all(text.as_bytes()) {
            Ok(()) => true,
            Err(error) => {
                // A closed pipe (for example `find ... | head`) ends output quietly.
                if error.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("find: write error: {}", message(&error));
                    self.status = 1;
                }
                self.stop = true;
                false
            }
        }
    }

    fn flush_output(&mut self) {
        if let Err(error) = self.out.flush() {
            if error.kind() != io::ErrorKind::BrokenPipe {
                eprintln!("find: write error: {}", message(&error));
                self.status = 1;
            }
            self.stop = true;
        }
    }

    fn delete(&mut self, entry: &Entry) -> bool {
        // GNU find leaves the starting point "." in place; do the same.
        if entry.path == Path::new(".") {
            return true;
        }
        let result = if entry.meta.is_dir() {
            fs::remove_dir(entry.path)
        } else {
            fs::remove_file(entry.path)
        };
        match result {
            Ok(()) => true,
            Err(error) => {
                self.report(entry.path, &error);
                false
            }
        }
    }

    fn run_now(&mut self, argv: &[String], in_dir: bool, entry: &Entry) -> bool {
        let (text, dir) = if in_dir {
            (
                format!("./{}", file_name_text(entry.path)),
                Some(parent_or_dot(entry.path)),
            )
        } else {
            (display_path(entry.path), None)
        };
        let command = exec::substitute(argv, &text);
        self.flush_output();
        match exec::run(&command, dir.as_deref()) {
            Ok(success) => success,
            Err(message) => {
                eprintln!("find: {message}");
                self.status = 1;
                false
            }
        }
    }

    fn queue(&mut self, index: usize, entry: &Entry) -> bool {
        let in_dir = self.batches[index].in_dir;
        let directory = in_dir.then(|| parent_or_dot(entry.path));
        let text = if in_dir {
            format!("./{}", file_name_text(entry.path))
        } else {
            display_path(entry.path)
        };
        let batch = &self.batches[index];
        let must_flush = !batch.pending.is_empty()
            && (batch.directory != directory || batch.pending_bytes + text.len() > BATCH_BYTES);
        if must_flush {
            self.flush_batch(index);
        }
        let batch = &mut self.batches[index];
        batch.directory = directory;
        batch.pending_bytes += text.len() + 1;
        batch.pending.push(text);
        true
    }

    fn flush_batch(&mut self, index: usize) {
        if self.batches[index].pending.is_empty() {
            return;
        }
        self.flush_output();
        let batch = &mut self.batches[index];
        let mut command = batch.argv.clone();
        command.append(&mut batch.pending);
        batch.pending_bytes = 0;
        let dir = batch.directory.take();
        match exec::run(&command, dir.as_deref()) {
            Ok(true) => {}
            Ok(false) => self.status = 1,
            Err(message) => {
                eprintln!("find: {message}");
                self.status = 1;
            }
        }
    }

    fn report(&mut self, path: &Path, error: &io::Error) {
        eprintln!("find: '{}': {}", display_path(path), message(error));
        self.status = 1;
    }

    fn finish(mut self) -> i32 {
        for index in 0..self.batches.len() {
            self.flush_batch(index);
        }
        self.flush_output();
        self.status
    }
}
