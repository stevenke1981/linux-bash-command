use super::{
    codec::{self, Header},
    options::Options,
};
use crate::common::{error, external::TemporaryFile};
use std::{
    cell::RefCell,
    fs::{self, File, Metadata},
    io::{self, BufRead, BufReader, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    time::UNIX_EPOCH,
};
#[derive(Default)]
struct Count {
    bytes: u64,
    tail: [u8; 8],
    tail_len: usize,
}
#[derive(Default)]
struct Totals {
    compressed: u64,
    uncompressed: u64,
    files: usize,
}
struct Counting<R> {
    inner: R,
    count: Rc<RefCell<Count>>,
}
impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        let mut count = self.count.borrow_mut();
        count.bytes = count.bytes.saturating_add(n as u64);
        if n >= 8 {
            count.tail.copy_from_slice(&buf[n - 8..n]);
        } else if n != 0 {
            count.tail.copy_within(n..8, 0);
            count.tail[8 - n..].copy_from_slice(&buf[..n]);
        }
        count.tail_len = (count.tail_len + n).min(8);
        Ok(n)
    }
}

fn original_name(name: &[u8]) -> Option<std::ffi::OsString> {
    if name.is_empty()
        || name == b"."
        || name == b".."
        || name.iter().any(|b| *b == b'/' || *b == b'\\' || *b == 0)
    {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(std::ffi::OsString::from_vec(name.to_vec()))
    }
    #[cfg(not(unix))]
    {
        #[cfg(windows)]
        if name.contains(&b':') {
            return None;
        }
        Some(std::ffi::OsString::from(
            String::from_utf8(name.to_vec()).ok()?,
        ))
    }
}
fn stripped_name(name: &std::ffi::OsStr, suffix: &str) -> Option<std::ffi::OsString> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        Some(std::ffi::OsString::from_vec(
            name.as_bytes().strip_suffix(suffix.as_bytes())?.to_vec(),
        ))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let name: Vec<_> = name.encode_wide().collect();
        let suffix: Vec<_> = suffix.encode_utf16().collect();
        Some(std::ffi::OsString::from_wide(
            name.strip_suffix(&suffix[..])?,
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Some(std::ffi::OsString::from(
            name.to_str()?.strip_suffix(suffix)?,
        ))
    }
}
fn output_path(input: &Path, o: &Options, header: &Header) -> Option<PathBuf> {
    if !o.decompress {
        let mut n = input.as_os_str().to_os_string();
        n.push(&o.suffix);
        return Some(n.into());
    }
    if o.restore_name {
        if let Some(name) = header.name.as_deref().and_then(original_name) {
            return Some(input.parent().unwrap_or(Path::new("")).join(name));
        }
    }
    let name = input.file_name()?;
    let parent = input.parent().unwrap_or(Path::new(""));
    if let Some(stripped) = stripped_name(name, &o.suffix) {
        if !stripped.is_empty() {
            return Some(parent.join(stripped));
        }
    }
    if o.suffix == ".gz" {
        if let Some(mut stripped) = stripped_name(name, ".tgz") {
            stripped.push(".tar");
            return Some(parent.join(stripped));
        }
        if let Some(stripped) = stripped_name(name, ".Z") {
            return Some(parent.join(stripped));
        }
    }
    None
}
fn metadata_header(input: &Path, meta: &Metadata, o: &Options) -> Header {
    if o.no_name {
        return Header::default();
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| u32::try_from(d.as_secs()).ok())
        .unwrap_or(0);
    let name = input.file_name().map(|n| {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            n.as_bytes().to_vec()
        }
        #[cfg(not(unix))]
        {
            n.to_string_lossy().as_bytes().to_vec()
        }
    });
    Header { name, mtime }
}
fn overwrite_allowed(path: &Path, o: &Options) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    if o.force {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        return Err(io::Error::other(format!(
            "{} already exists;\tnot overwritten",
            crate::common::path::display_path(path)
        )));
    }
    eprint!(
        "{}: {} already exists; do you wish to overwrite (y or n)? ",
        o.command,
        path.display()
    );
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    if answer.starts_with('y') || answer.starts_with('Y') {
        Ok(true)
    } else {
        Err(io::Error::other(format!(
            "{} not overwritten",
            crate::common::path::display_path(path)
        )))
    }
}
fn write_atomic(
    input: &mut dyn Read,
    meta: &Metadata,
    dest: &Path,
    o: &Options,
    header: &Header,
    replace: bool,
) -> io::Result<u64> {
    let directory = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = TemporaryFile::new(directory)?;
    let result = if o.decompress {
        io::copy(input, temp.file())?
    } else {
        codec::compress(input, temp.file(), o.level, header)?
    };
    temp.file().set_permissions(meta.permissions())?;
    let modified = if o.decompress && !o.no_name && header.mtime != 0 {
        Some(filetime::FileTime::from_unix_time(
            i64::from(header.mtime),
            0,
        ))
    } else {
        meta.modified().ok().map(Into::into)
    };
    filetime::set_file_handle_times(temp.file(), meta.accessed().ok().map(Into::into), modified)?;
    if replace {
        temp.persist(dest)?;
    } else {
        temp.persist_noclobber(dest)?;
    }
    Ok(result)
}
fn print_list(
    size: u64,
    unpacked: u64,
    name: &str,
    header: &Header,
    count: &Count,
    verbose: bool,
) -> io::Result<()> {
    let overhead = 18 + header.name.as_ref().map_or(0, |n| n.len() as u64 + 1);
    let payload = size.saturating_sub(overhead);
    let ratio = if unpacked == 0 {
        0.0
    } else {
        (unpacked as f64 - payload as f64) * 100.0 / unpacked as f64
    };
    let mut stdout = io::stdout().lock();
    if verbose {
        let crc = if count.tail_len == 8 {
            u32::from_le_bytes(count.tail[..4].try_into().unwrap())
        } else {
            0
        };
        let date = jiff::Timestamp::from_second(i64::from(header.mtime))
            .ok()
            .and_then(|t| {
                crate::common::datetime::format(
                    &t.to_zoned(crate::common::datetime::timezone(false).ok()?),
                    "%b %e %H:%M",
                )
                .ok()
            })
            .unwrap_or_else(|| "Jan  1 00:00".into());
        write!(stdout, "defla {crc:08x} {date} ")?;
    }
    writeln!(stdout, "{size:19} {unpacked:19} {ratio:5.1}% {name}")
}
fn copy_stdout(mut input: impl Read) -> io::Result<u64> {
    let stdout = io::stdout();
    let mut out = io::BufWriter::with_capacity(65536, stdout.lock());
    let count = io::copy(&mut input, &mut out)?;
    out.flush()?;
    Ok(count)
}
struct CountingOutput<W> {
    output: W,
    bytes: u64,
}
impl<W: Write> Write for CountingOutput<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.output.write(bytes)?;
        self.bytes = self.bytes.saturating_add(written as u64);
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}
fn ratio(raw: u64, compressed: u64, header: &Header) -> f64 {
    let overhead = 18 + header.name.as_ref().map_or(0, |n| n.len() as u64 + 1);
    if raw == 0 {
        0.0
    } else {
        (raw as f64 - compressed.saturating_sub(overhead) as f64) * 100.0 / raw as f64
    }
}
fn progress(name: &str, dest: &str, raw: u64, compressed: u64, header: &Header, o: &Options) {
    if !o.verbose || o.quiet {
        return;
    }
    let ratio = ratio(raw, compressed, header);
    if name == "stdout" {
        eprintln!("{ratio:5.1}%");
    } else {
        let verb = if o.keep && dest != "stdout" {
            "created"
        } else {
            "replaced with"
        };
        eprintln!("{name}:\t{ratio:5.1}% -- {verb} {dest}");
    }
}
fn decode_stream(
    input: impl Read,
    name: &str,
    size: u64,
    o: &Options,
    totals: &mut Totals,
) -> io::Result<()> {
    let (prefix_len, compressed, input) = sniff(input)?;
    if o.force && !o.test && !o.list && !compressed {
        copy_stdout(input)?;
        return Ok(());
    }
    if prefix_len == 2 && !compressed {
        return Err(io::Error::other("not in gzip format"));
    }
    let count = Rc::new(RefCell::new(Count::default()));
    let (mut dec, header) = codec::decoder(Counting {
        inner: input,
        count: count.clone(),
    });
    if o.test || o.list {
        return verify_stream(&mut dec, name, size, o, totals, &header, &count);
    }
    let raw = copy_stdout(&mut dec)?;
    if dec.trailing_garbage() {
        return Err(trailing_warning());
    }
    let compressed = if size == 0 {
        count.borrow().bytes
    } else {
        size
    };
    progress(name, "stdout", raw, compressed, &header, o);
    Ok(())
}
fn verify_stream<R: Read>(
    dec: &mut super::decode::Decoder<R>,
    name: &str,
    size: u64,
    o: &Options,
    totals: &mut Totals,
    header: &Header,
    count: &Rc<RefCell<Count>>,
) -> io::Result<()> {
    let n = io::copy(&mut *dec, &mut io::sink())?;
    if dec.trailing_garbage() {
        return Err(trailing_warning());
    }
    if o.list {
        let count = count.borrow();
        let compressed = if size == 0 { count.bytes } else { size };
        if totals.files == 0 {
            list_header(o)?;
        }
        print_list(compressed, n, name, header, &count, o.verbose)?;
        totals.compressed = totals.compressed.saturating_add(compressed);
        totals.uncompressed = totals.uncompressed.saturating_add(n);
        totals.files += 1;
    } else if o.verbose && !o.quiet {
        if name == "stdout" {
            eprintln!(" OK");
        } else {
            eprintln!("{name}:\t OK");
        }
    }
    Ok(())
}
fn sniff(input: impl Read) -> io::Result<(usize, bool, impl Read)> {
    let mut input = BufReader::new(input);
    let mut prefix = Vec::new();
    while prefix.len() < 2 {
        let mut byte = [0u8; 1];
        if input.read(&mut byte)? == 0 {
            break;
        }
        prefix.push(byte[0]);
    }
    Ok((
        prefix.len(),
        prefix == [0x1f, 0x8b],
        io::Cursor::new(prefix).chain(input),
    ))
}
fn handle_stream(
    input: impl Read,
    name: &str,
    size: u64,
    o: &Options,
    header: &Header,
    totals: &mut Totals,
) -> io::Result<()> {
    if o.decompress {
        return decode_stream(input, name, size, o, totals);
    }
    let stdout = io::stdout();
    let mut out = CountingOutput {
        output: io::BufWriter::with_capacity(65536, stdout.lock()),
        bytes: 0,
    };
    let raw = codec::compress(&mut { input }, &mut out, o.level, header)?;
    out.flush()?;
    progress(name, "stdout", raw, out.bytes, header, o);
    Ok(())
}

fn checked_metadata(path: &Path, o: &Options) -> io::Result<Metadata> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() && !o.force {
        return Err(io::Error::other("is a symbolic link -- ignored"));
    }
    if !meta.is_file() && !meta.file_type().is_symlink() {
        return Err(io::Error::other("not a regular file -- ignored"));
    }
    Ok(meta)
}
fn process_file(path: &Path, o: &Options, totals: &mut Totals) -> io::Result<()> {
    checked_metadata(path, o)?;
    let file = File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::other("not a regular file -- ignored"));
    }
    if o.stdout || o.list || o.test {
        let name = if o.list {
            output_path(path, o, &Header::default()).unwrap_or_else(|| path.into())
        } else {
            path.to_path_buf()
        };
        let header = metadata_header(path, &meta, o);
        return handle_stream(
            file,
            &name.to_string_lossy(),
            meta.len(),
            o,
            &header,
            totals,
        );
    }
    process_regular(path, o, meta, file)
}
fn process_regular(path: &Path, o: &Options, meta: Metadata, mut file: File) -> io::Result<()> {
    if !o.decompress && path.as_os_str().to_string_lossy().ends_with(&o.suffix) {
        if !o.quiet {
            eprintln!(
                "{}: {} already has {} suffix -- unchanged",
                o.command,
                path.display(),
                o.suffix
            );
        }
        return Ok(());
    }
    let encoded_header = metadata_header(path, &meta, o);
    let (dest, raw, header, trailing) = if o.decompress {
        if output_path(path, o, &Header::default()).is_none() {
            return Err(io::Error::other("unknown suffix -- ignored"));
        }
        let (prefix_len, compressed, input) = sniff(file)?;
        if prefix_len == 2 && !compressed {
            return Err(io::Error::other("not in gzip format"));
        }
        let (mut decoder, header) = codec::decoder(input);
        let dest = output_path(path, o, &header)
            .ok_or_else(|| io::Error::other("unknown suffix -- ignored"))?;
        ensure_distinct(path, &dest)?;
        let replace = overwrite_allowed(&dest, o)?;
        let raw = write_atomic(&mut decoder, &meta, &dest, o, &header, replace)?;
        (dest, raw, header, decoder.trailing_garbage())
    } else {
        let dest = output_path(path, o, &encoded_header).unwrap();
        ensure_distinct(path, &dest)?;
        let replace = overwrite_allowed(&dest, o)?;
        let raw = write_atomic(&mut file, &meta, &dest, o, &encoded_header, replace)?;
        (dest, raw, encoded_header, false)
    };
    finish_regular(path, &dest, raw, &meta, &header, trailing, o)
}
fn finish_regular(
    path: &Path,
    dest: &Path,
    raw: u64,
    meta: &Metadata,
    header: &Header,
    trailing: bool,
    o: &Options,
) -> io::Result<()> {
    let compressed = if o.decompress {
        meta.len()
    } else {
        fs::metadata(dest)?.len()
    };
    if !o.keep {
        fs::remove_file(path)?;
    }
    if trailing {
        return Err(trailing_warning());
    }
    progress(
        &path.to_string_lossy(),
        &dest.to_string_lossy(),
        raw,
        compressed,
        header,
        o,
    );
    Ok(())
}
fn ensure_distinct(input: &Path, output: &Path) -> io::Result<()> {
    if input == output || crate::common::meta::same_file(input, output).unwrap_or(false) {
        return Err(io::Error::other("input and output are the same file"));
    }
    Ok(())
}
fn trailing_warning() -> io::Error {
    io::Error::other("decompression OK, trailing garbage ignored")
}
fn visit(path: &Path, o: &Options, status: &mut i32, totals: &mut Totals) {
    if o.recursive && fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => visit(&entry.path(), o, status, totals),
                        Err(e) => {
                            eprintln!("{}: {}: {}", o.command, path.display(), error::io_text(&e));
                            *status = 1;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("{}: {}: {}", o.command, path.display(), error::io_text(&e));
                *status = 1;
            }
        }
        return;
    }
    if let Err(e) = process_file(path, o, totals) {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return;
        }
        let message = e.to_string();
        let warning = e.kind() == io::ErrorKind::AlreadyExists
            || message.contains("-- ignored")
            || message.contains("not overwritten")
            || message.contains("trailing garbage ignored");
        if !o.quiet || !warning {
            if message.contains("not overwritten") {
                eprintln!("{}: {}", o.command, error::io_text(&e));
            } else {
                eprintln!(
                    "{}: {}: {}",
                    o.command,
                    crate::common::path::display_path(path),
                    error::io_text(&e)
                );
            }
        }
        if !warning {
            *status = 1;
        } else if *status == 0 {
            *status = 2;
        }
    }
}
fn terminal_refused(o: &Options) -> bool {
    if !o.decompress
        && (o.stdout || o.files.is_empty() || o.files.iter().any(|s| s == "-"))
        && io::stdout().is_terminal()
        && !o.force
    {
        eprintln!(
            "{}: compressed data not written to a terminal. Use -f to force compression.",
            o.command
        );
        true
    } else {
        false
    }
}
fn list_header(o: &Options) -> io::Result<()> {
    if o.list && !o.quiet {
        let header = if o.verbose {
            "method  crc     date  time           compressed        uncompressed  ratio uncompressed_name"
        } else {
            "         compressed        uncompressed  ratio uncompressed_name"
        };
        writeln!(io::stdout().lock(), "{header}")?;
    }
    Ok(())
}
fn list_totals(o: &Options, totals: &Totals) -> io::Result<()> {
    if o.list && !o.quiet && totals.files > 1 {
        let ratio = if totals.uncompressed == 0 {
            0.0
        } else {
            (totals.uncompressed as f64 - totals.compressed as f64) * 100.0
                / totals.uncompressed as f64
        };
        let prefix = if o.verbose {
            "                            "
        } else {
            ""
        };
        writeln!(
            io::stdout().lock(),
            "{prefix}{:19} {:19} {ratio:5.1}% (totals)",
            totals.compressed,
            totals.uncompressed
        )?;
    }
    Ok(())
}
fn process_stdin(o: &Options, totals: &mut Totals) -> i32 {
    let input = io::stdin();
    match handle_stream(input.lock(), "stdout", 0, o, &Header::default(), totals) {
        Ok(()) => 0,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            let warning = e.to_string().contains("trailing garbage ignored");
            if !warning || !o.quiet {
                eprintln!("{}: stdin: {}", o.command, error::io_text(&e));
            }
            if warning {
                2
            } else {
                1
            }
        }
    }
}
pub fn run(o: &Options) -> i32 {
    if terminal_refused(o) {
        return 1;
    }
    let files = if o.files.is_empty() {
        vec!["-".to_string()]
    } else {
        o.files.clone()
    };
    let mut status = 0;
    let mut totals = Totals::default();
    for file in files {
        if file == "-" {
            let result = process_stdin(o, &mut totals);
            if result == 1 {
                status = 1;
            } else if result == 2 && status == 0 {
                status = 2;
            }
        } else {
            visit(Path::new(&file), o, &mut status, &mut totals);
        }
    }
    if let Err(e) = list_totals(o, &totals) {
        return i32::from(e.kind() != io::ErrorKind::BrokenPipe);
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_header_name() {
        assert!(original_name(b"../escape").is_none());
        assert!(original_name(b"a\\b").is_none());
        assert_eq!(original_name(b"file").unwrap(), "file");
    }
    #[test]
    fn suffixes() {
        let o = super::super::options::parse("gunzip", &[]).unwrap();
        assert_eq!(
            output_path(Path::new("a.gz"), &o, &Header::default()).unwrap(),
            PathBuf::from("a")
        );
        assert_eq!(
            output_path(Path::new("a.tgz"), &o, &Header::default()).unwrap(),
            PathBuf::from("a.tar")
        );
        assert!(output_path(Path::new("a"), &o, &Header::default()).is_none());
    }
    #[test]
    fn renamed_header() {
        let o = super::super::options::parse("gunzip", &["-N".into()]).unwrap();
        let h = Header {
            name: Some(b"original".to_vec()),
            mtime: 0,
        };
        assert_eq!(
            output_path(Path::new("dir/a.gz"), &o, &h).unwrap(),
            PathBuf::from("dir/original")
        );
    }
}
