use super::*;

/// 2024-01-02 10:00:00 UTC, a Tuesday.
const NOW: i64 = 1_704_189_600;

fn utc() -> TimeZone {
    TimeZone::UTC
}

fn parse_at(input: &str, zone: &TimeZone) -> Result<i64, InvalidDate> {
    let now = Timestamp::from_second(NOW).unwrap();
    parse(input, now, zone).map(|ts| ts.as_second())
}

fn p(input: &str) -> i64 {
    parse_at(input, &utc()).unwrap_or_else(|e| panic!("{input}: {e}"))
}

fn rejected(input: &str) {
    assert!(
        parse_at(input, &utc()).is_err(),
        "{input:?} should be rejected"
    );
}

#[test]
fn epoch_forms() {
    assert_eq!(p("@0"), 0);
    assert_eq!(p("@1700000000"), 1_700_000_000);
    assert_eq!(p("@1.9"), 1);
    assert_eq!(p("@1700000000.5"), 1_700_000_000);
    assert_eq!(p("@-5"), -5);
    assert_eq!(p("@ 5"), 5);
    rejected("@1e3");
    rejected("@");
    rejected("@.5");
}

#[test]
fn epoch_keeps_nanoseconds() {
    let ts = parse("@1.5", Timestamp::UNIX_EPOCH, &utc()).unwrap();
    assert_eq!(ts.subsec_nanosecond(), 500_000_000);
}

#[test]
fn iso_dates_and_times() {
    assert_eq!(p("2024-01-02"), 1_704_153_600);
    assert_eq!(p("2024-1-2"), 1_704_153_600);
    assert_eq!(p("2024/01/02"), 1_704_153_600);
    assert_eq!(p("2024-01-02 10:00 UTC"), 1_704_189_600);
    assert_eq!(p("2024-01-02 10:00:00.123456789"), 1_704_189_600);
    assert_eq!(p("2024-02-29"), 1_709_164_800);
    assert_eq!(p("2024-01-02 23:59:59.9999999999"), 1_704_239_999);
    assert_eq!(p("2024-01-02 1:2:3"), 1_704_157_323);
    assert_eq!(p("2024-01-02 00:00:00"), 1_704_153_600);
}

#[test]
fn iso_t_separator_and_offset() {
    assert_eq!(p("2024-01-02T05:10:00+08:00"), 1_704_143_400);
    assert_eq!(p("2024-01-02T10:00:00Z"), 1_704_189_600);
}

#[test]
fn numeric_offsets_and_named_zones() {
    assert_eq!(p("2024-01-02 10:00 +0800"), 1_704_160_800);
    assert_eq!(p("2024-01-02 10:00 +08:00"), 1_704_160_800);
    assert_eq!(p("2024-01-02 10:00 +8"), 1_704_160_800);
    assert_eq!(p("2024-01-02 10:00 -05:30"), 1_704_209_400);
    assert_eq!(p("2024-01-02 10:00 -0530"), 1_704_209_400);
    assert_eq!(p("2024-01-02 12:00:00 +08"), 1_704_168_000);
    assert_eq!(p("2024-01-02 10:00 Z"), 1_704_189_600);
    assert_eq!(p("2024-01-02 10:00 GMT"), 1_704_189_600);
    assert_eq!(p("2024-01-02 10:00 UT"), 1_704_189_600);
    assert_eq!(p("2024-01-02 10:00 EDT"), 1_704_204_000);
    assert_eq!(p("2024-01-02 12:00 PST"), 1_704_225_600);
}

#[test]
fn tz_prefix_selects_zone() {
    assert_eq!(p("TZ=\"Asia/Taipei\" 2026-01-01 00:00"), 1_767_196_800);
    assert_eq!(p("TZ=UTC 2024-01-02 10:00"), 1_704_189_600);
    rejected("TZ=\"No/Such_Zone\" 2024-01-02");
}

#[test]
fn default_zone_applies_without_zone_text() {
    let taipei = TimeZone::get("Asia/Taipei").unwrap();
    assert_eq!(
        parse_at("2024-01-02 10:00", &taipei).unwrap(),
        1_704_160_800
    );
    // An explicit offset still wins over the default zone.
    assert_eq!(
        parse_at("2024-01-02 10:00 UTC", &taipei).unwrap(),
        1_704_189_600
    );
}

#[test]
fn rfc_2822_and_ctime_forms() {
    assert_eq!(p("Thu, 30 Oct 2026 05:10:00 +0000"), 1_793_337_000);
    assert_eq!(p("Tue, 02 Jan 2024 10:00:00 GMT"), 1_704_189_600);
    assert_eq!(p("Tue, 2 Jan 2024 10:00:00 +0800"), 1_704_160_800);
    assert_eq!(p("Tue Jan 2 10:00:00 UTC 2024"), 1_704_189_600);
}

#[test]
fn weekday_is_ignored_with_an_explicit_date() {
    assert_eq!(p("Mon 2024-01-02"), 1_704_153_600);
    assert_eq!(p("2024-01-03 next monday"), 1_704_240_000);
    assert_eq!(p("2024-01-02 this friday"), 1_704_153_600);
}

#[test]
fn month_names() {
    assert_eq!(p("Jan 2 2024"), 1_704_153_600);
    assert_eq!(p("2 Jan 2024"), 1_704_153_600);
    assert_eq!(p("January 2, 2024"), 1_704_153_600);
    assert_eq!(p("Sep 3 2024"), 1_725_321_600);
    assert_eq!(p("Sept 3 2024"), 1_725_321_600);
    assert_eq!(p("Feb 29 2024 12:00"), 1_709_208_000);
    assert_eq!(p("Oct 30, 2026 10:00"), 1_793_354_400);
    assert_eq!(p("1st Jan 2024"), 1_704_067_200);
    assert_eq!(p("Jan 2 2024 3pm"), 1_704_207_600);
}

#[test]
fn month_without_year_uses_current_year() {
    // The base instant is 2024-01-02, so `Jan 5` means 2024-01-05.
    assert_eq!(p("Jan 5"), 1_704_153_600 + 3 * 86_400);
    assert_eq!(p("10/30"), parse_at("2024-10-30", &utc()).unwrap());
}

#[test]
fn slash_dates_are_month_first() {
    assert_eq!(p("10/30/2026"), 1_793_318_400);
    assert_eq!(p("10/30/26"), 1_793_318_400);
    assert_eq!(p("1/2/2024"), 1_704_153_600);
}

#[test]
fn compact_digit_forms() {
    assert_eq!(p("20240102"), 1_704_153_600);
    assert_eq!(p("20240102103000"), 1_704_191_400);
    assert_eq!(p("1030"), 1_704_191_400);
}

#[test]
fn clock_times_and_meridiem() {
    assert_eq!(p("10:30"), 1_704_191_400);
    assert_eq!(p("3pm"), 1_704_207_600);
    assert_eq!(p("2024-01-02 3pm"), 1_704_207_600);
    assert_eq!(p("2024-01-02 12pm"), 1_704_196_800);
    assert_eq!(p("2024-01-02 12:30pm"), 1_704_198_600);
    assert_eq!(p("2024-01-02 12:30 pm"), 1_704_198_600);
    assert_eq!(p("2024-01-02 12:00am"), 1_704_153_600);
    assert_eq!(p("2024-01-02 12 am"), 1_704_153_600);
    assert_eq!(p("2024-01-02 10:00:00 pm"), 1_704_232_800);
    rejected("2024-01-02 0:30am");
    rejected("2024-01-02 13pm");
}

#[test]
fn relative_units_from_explicit_date() {
    assert_eq!(p("2024-01-02 +3 days 2 hours ago"), 1_704_405_600);
    assert_eq!(p("2024-01-02 1 day 2 hours ago"), 1_704_232_800);
    assert_eq!(p("2024-01-02 2 hours 3 minutes ago"), 1_704_160_620);
    assert_eq!(p("1 day ago 2024-01-02"), 1_704_067_200);
    assert_eq!(p("2024-01-02 +1 min"), 1_704_153_660);
    assert_eq!(p("2024-01-02 2 seconds ago"), 1_704_153_598);
    assert_eq!(p("2024-01-02 +5 mins"), 1_704_153_900);
    assert_eq!(p("2024-01-02 2 days ago 1 hour"), 1_703_984_400);
    assert_eq!(p("2024-01-02 -1 day"), 1_704_067_200);
    assert_eq!(p("2024-01-02 3 weeks"), 1_705_968_000);
    assert_eq!(p("2024-01-02 3 weeks ago"), 1_702_339_200);
    assert_eq!(p("2024-01-02 +1 fortnight"), 1_705_363_200);
    assert_eq!(p("2024-01-02 +0 day"), 1_704_153_600);
}

#[test]
fn relative_months_and_years_follow_calendar() {
    // Month overflow rolls forward, as GNU does: Jan 31 + 1 month = Mar 2 in 2024.
    assert_eq!(p("2024-01-31 +1 month"), 1_709_337_600);
    assert_eq!(p("2024-01-02 next month"), 1_706_832_000);
    assert_eq!(p("2024-01-02 this month"), 1_704_153_600);
    assert_eq!(p("2024-01-02 -1 year"), 1_672_617_600);
    assert_eq!(p("2024-01-02 1 year ago"), 1_672_617_600);
    assert_eq!(p("2024-01-02 +2 years 3 months"), 1_775_088_000);
}

#[test]
fn next_last_this_with_units() {
    assert_eq!(p("2024-01-02 next week"), 1_704_758_400);
    assert_eq!(p("2024-01-02 last week"), 1_703_548_800);
    assert_eq!(p("2024-01-02 next day"), 1_704_240_000);
    assert_eq!(p("2024-01-02 this day"), 1_704_153_600);
}

#[test]
fn day_keywords() {
    assert_eq!(p("2024-01-02 yesterday"), 1_704_067_200);
    assert_eq!(p("2024-01-02 tomorrow"), 1_704_240_000);
    assert_eq!(p("2024-01-02 today"), 1_704_153_600);
    assert_eq!(p("2024-01-02 5pm tomorrow"), 1_704_301_200);
    assert_eq!(p("2024-01-02 midnight"), 1_704_153_600);
}

#[test]
fn keywords_relative_to_now() {
    assert_eq!(p("now"), NOW);
    assert_eq!(p("today"), 1_704_153_600);
    assert_eq!(p("yesterday"), 1_704_067_200);
    assert_eq!(p("tomorrow"), 1_704_240_000);
    assert_eq!(p("noon"), 1_704_196_800);
    assert_eq!(p("midnight"), 1_704_153_600);
    assert_eq!(p("+1 day"), NOW + 86_400);
    assert_eq!(p("last week"), NOW - 7 * 86_400);
    assert_eq!(p("2 hours ago"), NOW - 7_200);
}

#[test]
fn bare_weekdays_pick_the_anchor_day() {
    // 2024-01-02 is a Tuesday.
    assert_eq!(p("tuesday"), 1_704_153_600);
    assert_eq!(p("this tuesday"), 1_704_153_600);
    assert_eq!(p("monday"), 1_704_672_000);
    assert_eq!(p("next monday"), 1_704_672_000);
    assert_eq!(p("last monday"), 1_704_067_200);
    assert_eq!(p("next tuesday"), 1_704_758_400);
    assert_eq!(p("last tuesday"), 1_703_548_800);
}

#[test]
fn trailing_commas_are_ignored() {
    assert_eq!(p("Oct 30, 2026"), 1_793_318_400);
}

#[test]
fn rejects_invalid_calendar_and_clock_values() {
    rejected("2024-02-31");
    rejected("2023-02-29");
    rejected("2024-13-01");
    rejected("2024-00-10");
    rejected("2024-01-00");
    rejected("2024-01-02 24:00");
    rejected("2024-01-02 23:59:60");
    rejected("2024-01-02 10:60");
}

#[test]
fn rejects_gnu_forms_outside_the_spec() {
    rejected("x");
    rejected("30.10.2026");
    rejected("jan 2024");
    rejected("2024 jan 2");
    rejected("2024-01-02 1.5 hours ago");
    rejected("+1 hr");
    rejected("2024-01-02 10:00 Asia/Taipei");
    rejected("garbage");
}

#[test]
fn rejects_duplicate_slots() {
    rejected("2024-01-02 2024-01-03");
    rejected("10:00 11:00");
    rejected("2024-01-02 10:00 UTC +08:00");
    rejected("monday tuesday");
}

#[test]
fn ago_without_a_preceding_amount_is_rejected() {
    rejected("ago");
    rejected("2024-01-02 ago");
}

#[test]
fn whitespace_around_input_is_ignored() {
    assert_eq!(p("  2024-01-02  "), 1_704_153_600);
}

#[test]
fn empty_input_means_now_as_in_gnu_file_lines() {
    assert_eq!(p(""), NOW);
    assert_eq!(p("   "), NOW);
}

#[test]
fn fraction_truncates_past_nanoseconds() {
    assert_eq!(fraction_nanos("5"), 500_000_000);
    assert_eq!(fraction_nanos("123456789123"), 123_456_789);
    assert_eq!(fraction_nanos("000000001"), 1);
}

#[test]
fn offset_text_forms() {
    assert_eq!(parse_offset_text("Z"), Some(0));
    assert_eq!(parse_offset_text("z"), Some(0));
    assert_eq!(parse_offset_text("+08"), Some(8 * 3600));
    assert_eq!(parse_offset_text("+8"), Some(8 * 3600));
    assert_eq!(parse_offset_text("-0530"), Some(-(5 * 3600 + 30 * 60)));
    assert_eq!(parse_offset_text("+05:45"), Some(5 * 3600 + 45 * 60));
    assert_eq!(parse_offset_text("+24:00"), None);
    assert_eq!(parse_offset_text("08:00"), None);
    assert_eq!(parse_offset_text("+080"), None);
    assert_eq!(parse_offset_text("+0a"), None);
}

#[test]
fn clock_core_bounds() {
    assert_eq!(
        parse_clock_core("23:59:59.5"),
        Some(Clock {
            hour: 23,
            minute: 59,
            second: 59,
            nanos: 500_000_000
        })
    );
    assert!(parse_clock_core("24:00").is_none());
    assert!(parse_clock_core("10:00:60").is_none());
    assert!(parse_clock_core("10:00:00:00").is_none());
}

#[test]
fn weekday_and_month_lookups() {
    assert_eq!(weekday_index("sun"), Some(0));
    assert_eq!(weekday_index("thursday"), Some(4));
    assert_eq!(weekday_index("thur."), Some(4));
    assert_eq!(weekday_index("funday"), None);
    assert_eq!(month_number("dec"), Some(12));
    assert_eq!(month_number("march"), Some(3));
    assert_eq!(month_number("sept"), Some(9));
    assert_eq!(month_number("ma"), None);
    assert_eq!(month_number("janu"), None);
}

#[test]
fn zone_word_table() {
    assert_eq!(zone_word("utc"), Some(0));
    assert_eq!(zone_word("jst"), Some(9 * 3600));
    assert_eq!(zone_word("est"), Some(-5 * 3600));
    assert_eq!(zone_word("nowhere"), None);
}
