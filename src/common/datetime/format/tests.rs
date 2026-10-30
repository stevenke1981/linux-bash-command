use super::*;

/// 2024-01-02 15:04:05.123456789 UTC, a Tuesday. Expected strings were checked against GNU
/// `date -u` in the C locale.
fn sample() -> BrokenDown {
    BrokenDown {
        year: 2024,
        month: 1,
        day: 2,
        hour: 15,
        minute: 4,
        second: 5,
        nanosecond: 123_456_789,
        weekday: 2,
        yday: 2,
        offset_seconds: 0,
        abbreviation: "UTC".to_string(),
        epoch: 1_704_207_845,
        iso_year: 2024,
        iso_week: 1,
    }
}

fn fmt(text: &str) -> String {
    strftime(text, &sample())
}

fn with_year(year: i32) -> BrokenDown {
    BrokenDown { year, ..sample() }
}

#[test]
fn names_follow_c_locale() {
    assert_eq!(fmt("%a|%A|%b|%B|%h"), "Tue|Tuesday|Jan|January|Jan");
}

#[test]
fn composite_date_and_time() {
    assert_eq!(fmt("%c"), "Tue Jan  2 15:04:05 2024");
    assert_eq!(fmt("%x|%X"), "01/02/24|15:04:05");
    assert_eq!(fmt("%D %F %T %R"), "01/02/24 2024-01-02 15:04:05 15:04");
    assert_eq!(fmt("%r"), "03:04:05 PM");
}

#[test]
fn numeric_fields_are_zero_padded_by_default() {
    assert_eq!(fmt("%C|%d|%e|%H|%I|%j|%k|%l"), "20|02| 2|15|03|002|15| 3");
    assert_eq!(fmt("%m|%M|%S|%y|%Y"), "01|04|05|24|2024");
    assert_eq!(fmt("%g|%G|%V|%U|%W"), "24|2024|01|00|01");
}

#[test]
fn weekday_numbers() {
    assert_eq!(fmt("%u|%w"), "2|2");
    let sunday = BrokenDown {
        weekday: 0,
        ..sample()
    };
    assert_eq!(strftime("%u|%w", &sunday), "7|0");
}

#[test]
fn quarter_and_am_pm() {
    assert_eq!(fmt("%q|%p|%P|%#p|%^p"), "1|PM|pm|pm|PM");
}

#[test]
fn nanoseconds_and_fraction_width() {
    assert_eq!(fmt("%N"), "123456789");
    assert_eq!(fmt("%3N"), "123");
    assert_eq!(fmt("%-3N"), "123");
    assert_eq!(fmt("%10N"), "1234567890");
    assert_eq!(fmt("%_3N"), "123");
}

#[test]
fn epoch_seconds() {
    assert_eq!(fmt("%s"), "1704207845");
    assert_eq!(fmt("%_10s|%010s"), "1704207845|1704207845");
}

#[test]
fn zone_offsets_all_colon_forms() {
    assert_eq!(fmt("%z|%:z|%::z|%:::z"), "+0000|+00:00|+00:00:00|+00");
    assert_eq!(fmt("%Z"), "UTC");
    let taipei = BrokenDown {
        offset_seconds: 8 * 3600,
        abbreviation: "CST".to_string(),
        ..sample()
    };
    assert_eq!(strftime("%z|%:z|%:::z|%Z", &taipei), "+0800|+08:00|+08|CST");
    let half = BrokenDown {
        offset_seconds: -(5 * 3600 + 30 * 60),
        ..sample()
    };
    assert_eq!(strftime("%z|%:z", &half), "-0530|-05:30");
    let with_seconds = BrokenDown {
        offset_seconds: 3600 + 2 * 60 + 3,
        ..sample()
    };
    assert_eq!(strftime("%::z|%:::z", &with_seconds), "+01:02:03|+01:02:03");
}

#[test]
fn percent_and_control_characters() {
    assert_eq!(fmt("%%|%n|%t|"), "%|\n|\t|");
    assert_eq!(fmt("100%%"), "100%");
}

#[test]
fn dash_flag_disables_padding() {
    assert_eq!(fmt("%-d|%-m|%-e|%-H|%-j|%-S"), "2|1|2|15|2|5");
    assert_eq!(fmt("%-10a|%-b"), "Tue|Jan");
}

#[test]
fn underscore_flag_pads_with_spaces() {
    assert_eq!(fmt("%_d|%_m|%_j|%_u|%_H"), " 2| 1|  2|2|15");
    assert_eq!(fmt("%_Y"), "2024");
    assert_eq!(strftime("%_Y", &with_year(999)), " 999");
}

#[test]
fn zero_flag_pads_text_and_numbers_with_zeros() {
    assert_eq!(fmt("%0e|%03d"), "02|002");
    assert_eq!(fmt("%010a"), "0000000Tue");
}

#[test]
fn caret_uppercases_text() {
    assert_eq!(fmt("%^a|%^A|%^b|%^p|%^Z"), "TUE|TUESDAY|JAN|PM|UTC");
}

#[test]
fn hash_swaps_case() {
    assert_eq!(fmt("%#a|%#B|%#p|%#Z"), "TUE|JANUARY|pm|utc");
}

#[test]
fn width_pads_text_with_spaces() {
    assert_eq!(
        fmt("%10a|%-10a|%^10a|%#10b"),
        "       Tue|Tue|       TUE|       JAN"
    );
}

#[test]
fn width_on_numeric_fields() {
    assert_eq!(fmt("%5d|%1Y|%8j"), "00002|2024|00000002");
    assert_eq!(fmt("%_5d"), "    2");
}

#[test]
fn modifiers_e_and_o_are_accepted() {
    assert_eq!(fmt("%Ec|%Oy|%EC"), "Tue Jan  2 15:04:05 2024|24|20");
}

#[test]
fn unknown_directives_are_copied() {
    assert_eq!(fmt("%Q|%+|%E|%-"), "%Q|%+|%E|%-");
}

#[test]
fn trailing_percent_is_literal() {
    assert_eq!(fmt("abc%"), "abc%");
    assert_eq!(fmt("%"), "%");
}

#[test]
fn years_below_one_thousand_are_padded_except_in_c() {
    let early = BrokenDown {
        iso_year: 999,
        ..with_year(999)
    };
    assert_eq!(
        strftime("%Y|%F|%C|%y|%G", &early),
        "0999|0999-01-02|09|99|0999"
    );
    assert_eq!(strftime("%c", &early), "Tue Jan  2 15:04:05 999");
    assert_eq!(strftime("%10Y|%-Y", &early), "0000000999|999");
}

#[test]
fn iso_week_fields_use_their_own_year() {
    let boundary = BrokenDown {
        year: 2025,
        month: 12,
        day: 31,
        weekday: 3,
        yday: 365,
        iso_year: 2026,
        iso_week: 1,
        ..sample()
    };
    assert_eq!(strftime("%G-W%V", &boundary), "2026-W01");
    assert_eq!(strftime("%Y", &boundary), "2025");
}

#[test]
fn broken_down_matches_jiff_for_utc_instant() {
    let ts = jiff::Timestamp::from_second(1_704_207_845).unwrap();
    let fields = broken_down(ts, &jiff::tz::TimeZone::UTC);
    assert_eq!(fields.year, 2024);
    assert_eq!(fields.month, 1);
    assert_eq!(fields.day, 2);
    assert_eq!(fields.hour, 15);
    assert_eq!(fields.minute, 4);
    assert_eq!(fields.second, 5);
    assert_eq!(fields.weekday, 2);
    assert_eq!(fields.yday, 2);
    assert_eq!(fields.offset_seconds, 0);
    assert_eq!(fields.abbreviation, "UTC");
    assert_eq!(fields.epoch, 1_704_207_845);
    assert_eq!((fields.iso_year, fields.iso_week), (2024, 1));
}

#[test]
fn broken_down_applies_zone_offset() {
    let ts = jiff::Timestamp::from_second(0).unwrap();
    let zone = jiff::tz::TimeZone::fixed(jiff::tz::Offset::from_seconds(8 * 3600).unwrap());
    let fields = broken_down(ts, &zone);
    assert_eq!(
        (fields.hour, fields.day, fields.offset_seconds),
        (8, 1, 8 * 3600)
    );
    assert_eq!(strftime("%F %T %z", &fields), "1970-01-01 08:00:00 +0800");
}

#[test]
fn broken_down_reports_fraction() {
    let ts = jiff::Timestamp::new(0, 500_000_000).unwrap();
    let fields = broken_down(ts, &jiff::tz::TimeZone::UTC);
    assert_eq!(fields.nanosecond, 500_000_000);
    assert_eq!(strftime("%N", &fields), "500000000");
}

#[test]
fn unicode_text_passes_through() {
    assert_eq!(fmt("日 %d 時"), "日 02 時");
}
