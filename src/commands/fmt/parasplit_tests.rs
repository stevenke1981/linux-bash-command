use super::*;
use std::io::{self, Cursor, Read};

fn reader(bytes: &[u8]) -> FileOrStdReader {
    FileOrStdReader {
        inner: Box::new(Cursor::new(bytes.to_vec())),
        error: None,
    }
}
#[test]
fn decoding_preserves_byte_width_for_utf8_and_invalid_sequences() {
    for bytes in [
        &b"ascii"[..],
        "\u{e9}".as_bytes(),
        "\u{732b}".as_bytes(),
        "\u{1f642}".as_bytes(),
        &b"\xff\xc3\x28\xe2\x82"[..],
    ] {
        assert_eq!(byte_width(bytes), bytes.len());
    }
    assert_eq!(decode_char(b"", 0), (None, 1));
    assert_eq!(decode_char(b"\xe2\x82", 0), (None, 1));
    assert_eq!(decode_char(b"\xc3(", 0), (None, 1));
    assert_eq!(indent_width(b" \t ", 8), 9);
}
#[test]
fn prefix_filter_preserves_partial_and_prefix_only_lines() {
    let opts = FmtOptions {
        prefix: Some("> ".into()),
        ..FmtOptions::default()
    };
    let mut input = reader(b" \t> words\n> \nunmatched");
    let mut lines = FileLines::new(&opts, &mut input);
    let first = lines.next().unwrap().get_formatline();
    assert_eq!(first.prefix_indent_end, 2);
    assert_eq!(first.indent_end, 4);
    assert_eq!(first.indent_len, 10);
    let blank = lines.next().unwrap().get_noformatline();
    assert_eq!(blank.line, b"> ");
    assert!(blank.newline);
    let unmatched = lines.next().unwrap().get_noformatline();
    assert_eq!(unmatched.line, b"unmatched");
    assert!(!unmatched.newline);
}
#[test]
fn tabs_and_trailing_space_do_not_create_phantom_words() {
    let opts = FmtOptions::default();
    let words: Vec<_> = WordSplit::new(&opts, b"a.\t \tb \t").collect();
    assert_eq!(words.len(), 2);
    assert!(words[0].ends_punct);
    assert!(words[1].sentence_start);
    assert_eq!(words[1].before_tab, Some(0));
    assert_eq!(words[1].after_tab, 8);
    let uniform = FmtOptions {
        uniform: true,
        ..FmtOptions::default()
    };
    let words: Vec<_> = WordSplit::new(&uniform, b"a\t\tb").collect();
    assert_eq!(words[1].word, b"b");
    assert_eq!(words[1].before_tab, None);
}
#[test]
fn reader_errors_are_retained_for_the_command_status() {
    struct Failed;
    impl Read for Failed {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected read failure"))
        }
    }
    impl io::BufRead for Failed {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::other("injected read failure"))
        }
        fn consume(&mut self, _: usize) {}
    }
    let opts = FmtOptions::default();
    let mut input = FileOrStdReader {
        inner: Box::new(Failed),
        error: None,
    };
    assert!(ParagraphStream::new(&opts, &mut input).next().is_none());
    assert_eq!(input.error.unwrap().to_string(), "injected read failure");
}
#[test]
fn folded_mail_headers_form_one_paragraph_and_use_single_word_spacing() {
    let opts = FmtOptions {
        mail: true,
        ..FmtOptions::default()
    };
    let mut input = reader(b"From sender\n  continuation  words\nSubject: title\n\nbody text\n");
    let mut paragraphs = ParagraphStream::new(&opts, &mut input);
    let envelope = paragraphs.next().unwrap().unwrap();
    assert!(envelope.mail_header);
    assert_eq!(envelope.lines.len(), 2);
    let words = ParaWords::new(&opts, &envelope);
    assert_eq!(
        words.words().map(|word| word.word).collect::<Vec<_>>(),
        [b"From".as_slice(), b"sender", b"continuation", b"words"]
    );
    let subject = paragraphs.next().unwrap().unwrap();
    assert!(subject.mail_header);
    assert!(paragraphs.next().unwrap().is_err());
    let body = paragraphs.next().unwrap().unwrap();
    assert!(!body.mail_header);
}
#[test]
fn mail_header_detection_rejects_empty_fields_and_indented_body_text() {
    for (bytes, expected) in [
        (b"Subject: title".as_slice(), true),
        (b"From sender", true),
        (b": empty", false),
        (b"bad field: value", false),
        (b"plain text", false),
        (b"  Subject: indented", false),
    ] {
        let opts = FmtOptions::default();
        let mut input = reader(bytes);
        let line = FileLines::new(&opts, &mut input)
            .next()
            .unwrap()
            .get_formatline();
        assert_eq!(
            ParagraphStream::is_mail_header(&line),
            expected,
            "{bytes:?}"
        );
    }
}
#[test]
fn anti_prefix_and_exact_prefix_keep_lines_outside_the_paragraph() {
    let opts = FmtOptions {
        anti_prefix: Some("#".into()),
        ..FmtOptions::default()
    };
    let mut input = reader(b" # comment\nbody\n");
    let mut lines = FileLines::new(&opts, &mut input);
    assert_eq!(lines.next().unwrap().get_noformatline().line, b" # comment");
    assert_eq!(lines.next().unwrap().get_formatline().line, b"body");
    assert_eq!(
        FileLines::match_prefix_generic(b">", b"  > text", true),
        (false, 0)
    );
    assert_eq!(
        FileLines::match_prefix_generic(b">", b"  > text", false),
        (true, 2)
    );
}
