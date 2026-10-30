use super::*;

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

fn run(args: &[&str]) -> LsOptions {
    match parse(&owned(args)) {
        Parsed::Run(options, _) => *options,
        Parsed::Exit(code) => panic!("exit {code} for {args:?}"),
    }
}

#[test]
fn block_sizes_use_units_and_suffixes() {
    assert_eq!(
        parse_block_size("1K").unwrap(),
        SizeFormat::Scaled {
            unit: 1024,
            suffix: ""
        }
    );
    assert_eq!(
        parse_block_size("K").unwrap(),
        SizeFormat::Scaled {
            unit: 1024,
            suffix: "K"
        }
    );
    assert_eq!(
        parse_block_size("KB").unwrap(),
        SizeFormat::Scaled {
            unit: 1000,
            suffix: "kB"
        }
    );
    assert_eq!(
        parse_block_size("1").unwrap(),
        SizeFormat::Scaled {
            unit: 1,
            suffix: ""
        }
    );
    assert!(parse_block_size("0").is_err());
    assert!(parse_block_size("Q").is_err());
}

#[test]
fn time_and_sort_words_are_validated() {
    assert_eq!(parse_time("ctime"), Ok(TimeField::Changed));
    assert!(parse_time("later").is_err());
    assert_eq!(parse_sort("extension"), Ok(SortKey::Extension));
    assert!(parse_sort("random").is_err());
}

#[test]
fn short_flags_cluster_and_last_format_wins() {
    let options = run(&["-1xl"]);
    assert_eq!(options.format, Format::Long);
    let options = run(&["-lC"]);
    assert_eq!(options.format, Format::Columns);
}

#[test]
fn value_options_accept_attached_and_separate_values() {
    let options = run(&["-I", "*.o", "-I*.tmp", "--hide=x*", "-w", "40", "-T4"]);
    assert_eq!(options.ignore, ["*.o", "*.tmp"]);
    assert_eq!(options.hide, ["x*"]);
    assert_eq!(options.width, Some(40));
    assert_eq!(options.tabsize, 4);
}

#[test]
fn c_and_u_choose_the_time_field_unless_time_is_given() {
    assert_eq!(run(&["-c"]).time, TimeField::Changed);
    assert_eq!(run(&["-u"]).time, TimeField::Accessed);
    assert_eq!(run(&["--time=birth", "-c"]).time, TimeField::Birth);
}

#[test]
fn quoting_and_indicator_styles_parse() {
    assert_eq!(run(&["-Q"]).quoting, Quoting::C);
    assert_eq!(run(&["-N"]).quoting, Quoting::Literal);
    assert_eq!(
        run(&["--quoting-style=shell-always"]).quoting,
        Quoting::ShellAlways
    );
    assert_eq!(
        run(&["--indicator-style=file-type"]).indicator,
        Indicator::FileType
    );
    assert_eq!(run(&["-p"]).indicator, Indicator::Slash);
}

#[test]
fn unsupported_and_invalid_options_exit_with_usage_status() {
    assert!(matches!(parse(&owned(&["-D"])), Parsed::Exit(2)));
    assert!(matches!(parse(&owned(&["--sort=bogus"])), Parsed::Exit(1)));
    assert!(matches!(parse(&owned(&["-Z"])), Parsed::Exit(2)));
}
