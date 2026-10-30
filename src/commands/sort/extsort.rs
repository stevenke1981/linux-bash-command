//! External sorting for `sort`. Input is cut into sorted runs of about the buffer size.
//! A single run is written straight to the output; several runs are spilled to temporary
//! files and merged with a k-way merge, in batches of the merge fan-in. Runs always cover
//! consecutive parts of the input, and ties go to the earlier run, so `-s` and `-u` keep
//! the original order of equal lines.

use std::cmp::Ordering;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::common::error::io_error_text;
use crate::common::io::read_record;

use super::compare::Comparer;

/// Approximate bookkeeping cost per buffered line, added to its length.
const RECORD_OVERHEAD: u64 = 48;

/// Owns the temporary directory and every file created in it; files left over when the
/// sort ends (including after an error) are removed on drop.
pub struct TempDir {
    dir: PathBuf,
    paths: Vec<PathBuf>,
    counter: u64,
}

impl TempDir {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            paths: Vec::new(),
            counter: 0,
        }
    }

    /// Creates a new empty file and returns its path with a buffered writer.
    fn create(&mut self) -> io::Result<(PathBuf, BufWriter<File>)> {
        loop {
            self.counter += 1;
            let name = format!("agentcmd-sort-{}-{}", std::process::id(), self.counter);
            let path = self.dir.join(name);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    self.paths.push(path.clone());
                    return Ok((path, BufWriter::new(file)));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    let message = format!(
                        "cannot create temporary file in '{}': {}",
                        self.dir.display(),
                        io_error_text(&error)
                    );
                    return Err(io::Error::other(message));
                }
            }
        }
    }

    /// Removes a temporary file that has been merged away.
    fn forget(&mut self, path: &Path) {
        let _ = fs::remove_file(path);
        self.paths.retain(|p| p != path);
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

/// Collects input lines and produces the sorted output.
pub struct Sorter<'a> {
    comparer: &'a Comparer,
    delim: u8,
    limit: u64,
    fan_in: usize,
    chunk: Vec<Vec<u8>>,
    chunk_bytes: u64,
    runs: Vec<PathBuf>,
    temp: TempDir,
}

impl<'a> Sorter<'a> {
    /// `limit` is the buffer size in bytes; `fan_in` the most runs merged at once.
    pub fn new(
        comparer: &'a Comparer,
        delim: u8,
        limit: u64,
        fan_in: usize,
        temp_dir: PathBuf,
    ) -> Self {
        Self {
            comparer,
            delim,
            limit: limit.max(1),
            fan_in: fan_in.max(2),
            chunk: Vec::new(),
            chunk_bytes: 0,
            runs: Vec::new(),
            temp: TempDir::new(temp_dir),
        }
    }

    /// Reads every record of `reader` into the sort.
    pub fn add(&mut self, reader: &mut dyn BufRead) -> io::Result<()> {
        let mut buf = Vec::new();
        while read_record(reader, self.delim, &mut buf)? {
            trim_delim(&mut buf, self.delim);
            self.chunk_bytes += buf.len() as u64 + RECORD_OVERHEAD;
            self.chunk.push(std::mem::take(&mut buf));
            if self.chunk_bytes >= self.limit {
                self.spill()?;
            }
        }
        Ok(())
    }

    fn sort_chunk(&mut self) {
        let comparer = self.comparer;
        self.chunk.sort_by(|a, b| comparer.compare(a, b));
    }

    /// Sorts the buffered lines into a new run file.
    fn spill(&mut self) -> io::Result<()> {
        self.sort_chunk();
        let delim = self.delim;
        let (path, mut writer) = self.temp.create()?;
        for line in self.chunk.drain(..) {
            write_line(&mut writer, &line, delim)?;
        }
        writer.flush()?;
        self.runs.push(path);
        self.chunk_bytes = 0;
        Ok(())
    }

    /// Writes the sorted result of everything added so far.
    pub fn finish(mut self, out: &mut dyn Write) -> io::Result<()> {
        if self.runs.is_empty() {
            self.sort_chunk();
            return write_unique_or_all(out, &self.chunk, self.comparer, self.delim);
        }
        if !self.chunk.is_empty() {
            self.spill()?;
        }
        let runs = self.reduce_runs()?;
        let readers = open_runs(&runs)?;
        merge_sources(readers, self.comparer, self.delim, out)
    }

    /// Merges groups of runs until at most `fan_in` remain, keeping the input order.
    fn reduce_runs(&mut self) -> io::Result<Vec<PathBuf>> {
        let comparer = self.comparer;
        let delim = self.delim;
        while self.runs.len() > self.fan_in {
            let runs = std::mem::take(&mut self.runs);
            let mut next = Vec::new();
            for group in runs.chunks(self.fan_in) {
                if group.len() == 1 {
                    next.push(group[0].clone());
                    continue;
                }
                let (path, mut writer) = self.temp.create()?;
                let readers = open_runs(group)?;
                merge_sources(readers, comparer, delim, &mut writer)?;
                writer.flush()?;
                for old in group {
                    self.temp.forget(old);
                }
                next.push(path);
            }
            self.runs = next;
        }
        Ok(std::mem::take(&mut self.runs))
    }
}

fn trim_delim(buf: &mut Vec<u8>, delim: u8) {
    if buf.last() == Some(&delim) {
        buf.pop();
    }
}

/// Writes one record followed by the line delimiter.
pub fn write_line(out: &mut dyn Write, line: &[u8], delim: u8) -> io::Result<()> {
    out.write_all(line)?;
    out.write_all(&[delim])
}

/// Writes already sorted lines. With `-u`, only the first of each run of equal keys.
fn write_unique_or_all(
    out: &mut dyn Write,
    lines: &[Vec<u8>],
    comparer: &Comparer,
    delim: u8,
) -> io::Result<()> {
    let mut last: Option<&[u8]> = None;
    for line in lines {
        if comparer.unique {
            if let Some(prev) = last {
                if comparer.compare(prev, line) == Ordering::Equal {
                    continue;
                }
            }
            last = Some(line.as_slice());
        }
        write_line(out, line, delim)?;
    }
    Ok(())
}

fn open_runs(paths: &[PathBuf]) -> io::Result<Vec<Box<dyn BufRead>>> {
    let mut readers: Vec<Box<dyn BufRead>> = Vec::with_capacity(paths.len());
    for path in paths {
        readers.push(Box::new(BufReader::new(File::open(path)?)));
    }
    Ok(readers)
}

/// One input of a merge: its reader and the record at its head.
struct Source {
    reader: Box<dyn BufRead>,
    line: Vec<u8>,
}

impl Source {
    /// Loads the next record; `false` at end of input.
    fn fill(&mut self, delim: u8) -> io::Result<bool> {
        if read_record(&mut *self.reader, delim, &mut self.line)? {
            trim_delim(&mut self.line, delim);
            return Ok(true);
        }
        Ok(false)
    }
}

/// Min-heap of input indices ordered by their head records; ties go to the lower index.
struct Merger<'a> {
    comparer: &'a Comparer,
    sources: Vec<Source>,
    heap: Vec<usize>,
}

impl<'a> Merger<'a> {
    fn new(readers: Vec<Box<dyn BufRead>>, comparer: &'a Comparer, delim: u8) -> io::Result<Self> {
        let mut sources = Vec::with_capacity(readers.len());
        let mut heap = Vec::with_capacity(readers.len());
        for reader in readers {
            let mut source = Source {
                reader,
                line: Vec::new(),
            };
            if source.fill(delim)? {
                heap.push(sources.len());
            }
            sources.push(source);
        }
        let mut merger = Self {
            comparer,
            sources,
            heap,
        };
        for pos in (0..merger.heap.len() / 2).rev() {
            merger.sift_down(pos);
        }
        Ok(merger)
    }

    fn less(&self, a: usize, b: usize) -> bool {
        let order = self
            .comparer
            .compare(&self.sources[a].line, &self.sources[b].line);
        order.is_lt() || (order.is_eq() && a < b)
    }

    fn sift_down(&mut self, mut pos: usize) {
        let len = self.heap.len();
        loop {
            let left = 2 * pos + 1;
            if left >= len {
                break;
            }
            let right = left + 1;
            let mut child = left;
            if right < len && self.less(self.heap[right], self.heap[left]) {
                child = right;
            }
            if !self.less(self.heap[child], self.heap[pos]) {
                break;
            }
            self.heap.swap(child, pos);
            pos = child;
        }
    }

    /// The record that comes next, if any.
    fn top_line(&self) -> Option<&[u8]> {
        self.heap
            .first()
            .map(|&index| self.sources[index].line.as_slice())
    }

    /// Replaces the top record with the next one from the same input.
    fn advance(&mut self, delim: u8) -> io::Result<()> {
        let Some(&index) = self.heap.first() else {
            return Ok(());
        };
        if self.sources[index].fill(delim)? {
            self.sift_down(0);
        } else {
            self.heap.swap_remove(0);
            if !self.heap.is_empty() {
                self.sift_down(0);
            }
        }
        Ok(())
    }
}

/// Merges sorted inputs into `out`. With `-u`, a record equal by key to the last one
/// written is dropped.
pub fn merge_sources(
    readers: Vec<Box<dyn BufRead>>,
    comparer: &Comparer,
    delim: u8,
    out: &mut dyn Write,
) -> io::Result<()> {
    let mut merger = Merger::new(readers, comparer, delim)?;
    let mut last: Vec<u8> = Vec::new();
    let mut have_last = false;
    while let Some(line) = merger.top_line() {
        let duplicate =
            comparer.unique && have_last && comparer.compare(&last, line) == Ordering::Equal;
        if !duplicate {
            write_line(out, line, delim)?;
            if comparer.unique {
                last.clear();
                last.extend_from_slice(line);
                have_last = true;
            }
        }
        merger.advance(delim)?;
    }
    Ok(())
}
