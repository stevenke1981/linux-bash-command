use super::*;
use crate::commands::stat::platform::S_IFREG;

fn stat_of(mode: u32, size: u64, blocks: u64) -> RawStat {
    RawStat {
        dev: 1,
        ino: 2,
        mode,
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
        size,
        blocks,
        blksize: 4096,
        atime: (0, 0),
        mtime: (0, 0),
        ctime: (0, 0),
        btime: None,
    }
}

fn config_for(list: &[&str]) -> Result<Config, i32> {
    let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let items = Parser::new("du", SPECS).parse(&args).unwrap();
    configure(&items)
}

#[test]
fn apparent_size_counts_files_not_directories() {
    assert_eq!(entry_bytes(&stat_of(S_IFREG, 3001, 8), false, true), 3001);
    assert_eq!(entry_bytes(&stat_of(S_IFDIR, 4096, 8), true, true), 0);
}

#[test]
fn block_usage_counts_directories_too() {
    assert_eq!(entry_bytes(&stat_of(S_IFREG, 3001, 8), false, false), 4096);
    assert_eq!(entry_bytes(&stat_of(S_IFDIR, 4096, 8), true, false), 4096);
}

#[test]
fn directory_detection_uses_type_bits() {
    assert!(is_directory(&stat_of(S_IFDIR | 0o755, 0, 0)));
    assert!(!is_directory(&stat_of(S_IFREG | 0o644, 0, 0)));
}

#[test]
fn default_scale_is_one_kib_blocks() {
    let config = config_for(&[]).unwrap();
    assert_eq!(config.scale, Scale::Unit(1024));
    assert!(!config.apparent);
    assert_eq!(config.deref, Deref::Never);
}

#[test]
fn bytes_option_sets_apparent_and_byte_scale() {
    let config = config_for(&["-b", "x"]).unwrap();
    assert!(config.apparent);
    assert_eq!(config.scale, Scale::Bytes);
    assert_eq!(config.operands, vec!["x"]);
}

#[test]
fn last_scale_option_wins() {
    assert_eq!(config_for(&["-h"]).unwrap().scale, Scale::Human(1024));
    assert_eq!(config_for(&["--si"]).unwrap().scale, Scale::Human(1000));
    assert_eq!(config_for(&["-m"]).unwrap().scale, Scale::Unit(1024 * 1024));
    assert_eq!(config_for(&["-B", "2K"]).unwrap().scale, Scale::Unit(2048));
    assert_eq!(config_for(&["-h", "-k"]).unwrap().scale, Scale::Unit(1024));
}

#[test]
fn dereference_options() {
    assert_eq!(config_for(&["-L"]).unwrap().deref, Deref::All);
    assert_eq!(config_for(&["-D"]).unwrap().deref, Deref::Operands);
    assert_eq!(config_for(&["-H"]).unwrap().deref, Deref::Operands);
    assert_eq!(config_for(&["-L", "-P"]).unwrap().deref, Deref::Never);
}

#[test]
fn bad_numbers_are_rejected() {
    assert!(config_for(&["-B", "3x"]).is_err());
    assert!(config_for(&["-B", "0"]).is_err());
    assert!(config_for(&["-d", "many"]).is_err());
    assert!(config_for(&["-t", "bogus"]).is_err());
    assert!(config_for(&["--time=sometime"]).is_err());
    assert!(config_for(&["--time-style=weird"]).is_err());
}

#[test]
fn threshold_keeps_sign() {
    assert_eq!(
        config_for(&["-t", "2K"]).unwrap().threshold,
        Some((false, 2048))
    );
    assert_eq!(
        config_for(&["-t", "-2K"]).unwrap().threshold,
        Some((true, 2048))
    );
}

#[test]
fn time_fields_and_words() {
    assert_eq!(
        config_for(&["--time"]).unwrap().time,
        Some(TimeField::Modified)
    );
    assert_eq!(
        config_for(&["--time=atime"]).unwrap().time,
        Some(TimeField::Accessed)
    );
    assert_eq!(
        config_for(&["--time=status"]).unwrap().time,
        Some(TimeField::Changed)
    );
    assert_eq!(
        config_for(&["--time=creation"]).unwrap().time,
        Some(TimeField::Born)
    );
}

#[test]
fn exclusion_patterns_accumulate() {
    let config = config_for(&["--exclude=*.o", "--exclude", "tmp*"]).unwrap();
    assert_eq!(config.excludes, vec!["*.o", "tmp*"]);
}

#[test]
fn inodes_is_unsupported() {
    assert_eq!(config_for(&["--inodes"]).err(), Some(EXIT_FAIL));
}

#[test]
fn should_print_respects_summarize_and_depth() {
    let config = config_for(&["-d", "1"]).unwrap();
    let walk = Walk {
        config: &config,
        out: crate::common::io::stdout_writer(),
        zone: TimeZone::UTC,
        seen: HashSet::new(),
        ancestors: Vec::new(),
        status: EXIT_OK,
    };
    assert!(walk.should_print(0, true, true));
    assert!(walk.should_print(1, false, true));
    assert!(!walk.should_print(2, false, true));
    assert!(!walk.should_print(1, false, false), "files need -a");
    assert!(
        walk.should_print(0, true, false),
        "operands are always listed"
    );
}

#[test]
fn threshold_hides_small_or_large_entries() {
    let small = config_for(&["-t", "2K"]).unwrap();
    let walk = Walk {
        config: &small,
        out: crate::common::io::stdout_writer(),
        zone: TimeZone::UTC,
        seen: HashSet::new(),
        ancestors: Vec::new(),
        status: EXIT_OK,
    };
    assert!(walk.below_threshold(100));
    assert!(!walk.below_threshold(4096));
}
