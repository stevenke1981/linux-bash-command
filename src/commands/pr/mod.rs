//! Page-bounded printing; only one page of input is retained at a time.
mod layout;
mod options;
use crate::common::{
    datetime,
    error::{io_text, usage},
    io::{input, output},
};
use options::Options;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::rc::Rc;
use std::time::SystemTime;

enum Event {
    Line(Vec<u8>),
    Break,
    End,
}
struct Source {
    reader: Box<dyn BufRead>,
    pending: VecDeque<Event>,
    ended: bool,
}
impl Source {
    fn next(&mut self) -> io::Result<Event> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(event);
        }
        if self.ended {
            return Ok(Event::End);
        }
        let mut bytes = Vec::new();
        if self.reader.read_until(b'\n', &mut bytes)? == 0 {
            self.ended = true;
            return Ok(Event::End);
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        let mut start = 0;
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'\x0c' {
                if index > start {
                    self.pending
                        .push_back(Event::Line(bytes[start..index].to_vec()));
                }
                self.pending.push_back(Event::Break);
                start = index + 1;
            }
        }
        if start < bytes.len() || !bytes.contains(&b'\x0c') {
            self.pending.push_back(Event::Line(bytes[start..].to_vec()));
        }
        Ok(self.pending.pop_front().unwrap_or(Event::End))
    }
}
fn source(reader: Box<dyn BufRead>) -> Source {
    Source {
        reader,
        pending: VecDeque::new(),
        ended: false,
    }
}
fn header(
    o: &Options,
    date: &str,
    title: &str,
    page: usize,
    out: &mut dyn Write,
) -> io::Result<()> {
    let right = format!("Page {page}");
    let space = o
        .width
        .saturating_sub(date.len() + title.len() + right.len());
    let left = space / 2;
    let right_space = space - left;
    writeln!(
        out,
        "\n\n{}{}{}{}{}{}",
        " ".repeat(o.margin),
        date,
        " ".repeat(left.max(1)),
        title,
        " ".repeat(right_space.max(1)),
        right
    )?;
    out.write_all(b"\n\n")
}
fn finish_page(
    o: &Options,
    body_lines: usize,
    forced: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    if o.omit {
        if forced && !o.no_pages {
            out.write_all(b"\x0c")?;
        }
    } else if o.form {
        out.write_all(b"\x0c")?;
    } else {
        for _ in body_lines..o.length {
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}
type Cell = Option<(Vec<u8>, i64)>;
fn page_rows(o: &Options) -> usize {
    let lines = if o.omit { o.length } else { o.length - 10 };
    (lines / if o.double { 2 } else { 1 }).max(1)
}
fn print_page(
    rows: &[Vec<Cell>],
    o: &Options,
    date: &str,
    title: &str,
    page: usize,
    forced: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    if page < o.first || page > o.last {
        return Ok(());
    }
    if !o.omit {
        header(o, date, title, page, out)?
    }
    let mut body = if o.omit { 0 } else { 5 };
    for (index, row) in rows.iter().enumerate() {
        let bytes = layout::row(row, o).map_err(io::Error::other)?;
        out.write_all(&bytes)?;
        body += 1;
        if o.double
            && (!o.omit
                || index + 1 < rows.len()
                || rows
                    .last()
                    .is_some_and(|row| row.iter().all(Option::is_some)))
        {
            out.write_all(b"\n")?;
            body += 1;
        }
    }
    finish_page(o, body, forced, out)
}
fn paginate_single(
    mut reader: Source,
    o: &Options,
    date: &str,
    title: &str,
    out: &mut dyn Write,
) -> io::Result<()> {
    let capacity = page_rows(o)
        .checked_mul(o.columns)
        .ok_or_else(|| io::Error::other("too many columns"))?;
    let mut page = 1;
    let mut number = 0i64;
    let mut offset = None;
    loop {
        let mut lines = Vec::new();
        let (mut forced, mut ended) = (false, false);
        while lines.len() < capacity {
            match reader.next()? {
                Event::End => {
                    ended = true;
                    break;
                }
                Event::Break if o.no_pages => {}
                Event::Break => {
                    forced = true;
                    break;
                }
                Event::Line(bytes) => {
                    number = number.saturating_add(1);
                    lines.push((bytes, number));
                }
            }
        }
        if lines.is_empty() && ended {
            break;
        }
        if page == o.first {
            offset = o
                .first_number
                .map(|first| (first, lines.first().map_or(1, |(_, n)| *n)));
        }
        if let Some((first, base)) = offset {
            for (_, number) in &mut lines {
                *number = first.saturating_add(number.saturating_sub(base));
            }
        }
        let height = lines.len().div_ceil(o.columns);
        let mut rows = vec![vec![None; o.columns]; height];
        if o.across {
            for (index, line) in lines.into_iter().enumerate() {
                rows[index / o.columns][index % o.columns] = Some(line);
            }
        } else {
            let size = lines.len() / o.columns;
            let extra = lines.len() % o.columns;
            let mut iter = lines.into_iter();
            for column in 0..o.columns {
                for row in rows.iter_mut().take(size + usize::from(column < extra)) {
                    row[column] = iter.next();
                }
            }
        }
        print_page(&rows, o, date, title, page, forced, out)?;
        if ended || page >= o.last {
            break;
        }
        page += 1;
    }
    Ok(())
}
fn paginate_merge(
    readers: Vec<Rc<RefCell<Source>>>,
    o: &Options,
    date: &str,
    title: &str,
    out: &mut dyn Write,
) -> io::Result<()> {
    let mut page = 1;
    let mut number = 0i64;
    let mut first_offset = None;
    loop {
        let mut rows = Vec::new();
        let mut stopped = vec![false; readers.len()];
        let mut forced = false;
        for _ in 0..page_rows(o) {
            let mut row = Vec::new();
            let mut any = false;
            number = number
                .checked_add(1)
                .ok_or_else(|| io::Error::other("line number overflow"))?;
            if page == o.first && first_offset.is_none() {
                first_offset = o.first_number.map(|first| (first, number));
            }
            let shown = if let Some((first, base)) = first_offset {
                first
                    .checked_add(number - base)
                    .ok_or_else(|| io::Error::other("line number overflow"))?
            } else {
                number
            };
            for (index, reader) in readers.iter().enumerate() {
                let cell = if stopped[index] {
                    None
                } else {
                    loop {
                        match reader.borrow_mut().next()? {
                            Event::Line(bytes) => {
                                any = true;
                                break Some((bytes, shown));
                            }
                            Event::End => {
                                stopped[index] = true;
                                break None;
                            }
                            Event::Break if o.no_pages => {}
                            Event::Break => {
                                forced = true;
                                stopped[index] = true;
                                break None;
                            }
                        }
                    }
                };
                row.push(cell);
            }
            if !any {
                number -= 1;
                break;
            }
            rows.push(row);
        }
        if rows.is_empty() && readers.iter().all(|reader| reader.borrow().ended) {
            break;
        }
        print_page(&rows, o, date, title, page, forced, out)?;
        if page >= o.last || readers.iter().all(|reader| reader.borrow().ended) {
            break;
        }
        page += 1;
    }
    Ok(())
}
pub fn run(arguments: &[String]) -> i32 {
    let (o, files) = match options::parse(arguments) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if o.merge && o.columns > 1 {
        return usage(
            "pr",
            "cannot specify number of columns when printing in parallel",
            1,
        );
    }
    let mut failed = false;
    let status = output("pr", |out| {
        if o.merge {
            let mut readers = Vec::new();
            let mut standard_input = None;
            for file in &files {
                let is_stdin = matches!(file.as_str(), "-" | "/dev/stdin");
                if is_stdin {
                    if let Some(reader) = &standard_input {
                        readers.push(Rc::clone(reader));
                        continue;
                    }
                }
                match input(file) {
                    Ok(reader) => {
                        let reader = Rc::new(RefCell::new(source(reader)));
                        if is_stdin {
                            standard_input = Some(Rc::clone(&reader));
                        }
                        readers.push(reader);
                    }
                    Err(e) => {
                        if !o.quiet {
                            eprintln!("pr: {file}: {}", io_text(&e));
                        }
                        failed = true;
                    }
                }
            }
            if !readers.is_empty() {
                let date = datetime::from_system_time(SystemTime::now())
                    .and_then(|date| datetime::format(&date, &o.date))
                    .map_err(io::Error::other)?;
                paginate_merge(readers, &o, &date, o.header.as_deref().unwrap_or(""), out)?;
            }
        } else {
            for file in &files {
                match input(file) {
                    Ok(reader) => {
                        let time = if file == "-" {
                            SystemTime::now()
                        } else {
                            std::fs::metadata(file)
                                .and_then(|meta| meta.modified())
                                .unwrap_or_else(|_| SystemTime::now())
                        };
                        let date = datetime::from_system_time(time)
                            .and_then(|date| datetime::format(&date, &o.date))
                            .map_err(io::Error::other)?;
                        let title =
                            o.header
                                .as_deref()
                                .unwrap_or(if file == "-" { "" } else { file });
                        if let Err(e) = paginate_single(source(reader), &o, &date, title, out) {
                            if e.kind() == io::ErrorKind::BrokenPipe {
                                return Err(e);
                            }
                            eprintln!("pr: {file}: {}", io_text(&e));
                            failed = true;
                        }
                    }
                    Err(e) => {
                        if !o.quiet {
                            eprintln!("pr: {file}: {}", io_text(&e));
                        }
                        failed = true;
                    }
                }
            }
        }
        Ok(())
    });
    status.max(i32::from(failed))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formfeed_events() {
        let mut input = source(Box::new(io::Cursor::new(b"a\x0cb\n")));
        assert!(matches!(input.next().unwrap(),Event::Line(x)if x==b"a"));
        assert!(matches!(input.next().unwrap(), Event::Break));
        assert!(matches!(input.next().unwrap(),Event::Line(x)if x==b"b"));
    }
    #[test]
    fn balanced_columns() {
        assert_eq!(7usize.div_ceil(3), 3);
    }
    #[test]
    fn body_capacity() {
        assert_eq!(page_rows(&Options::default()), 56);
    }
    #[test]
    fn shared_sources_feed_successive_merge_columns() {
        let reader = Rc::new(RefCell::new(source(Box::new(io::Cursor::new(
            b"a\nb\nc\nd\n",
        )))));
        let options = Options {
            merge: true,
            omit: true,
            join: true,
            ..Default::default()
        };
        let mut out = Vec::new();
        paginate_merge(vec![Rc::clone(&reader), reader], &options, "", "", &mut out).unwrap();
        assert_eq!(out, b"a\tb\nc\td\n");
    }
}
