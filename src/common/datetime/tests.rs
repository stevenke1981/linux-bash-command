use super::*;

#[test]
fn empty_tz_value_means_utc() {
    assert_eq!(zone_from_name(""), TimeZone::UTC);
    assert_eq!(zone_from_name(":"), TimeZone::UTC);
}

#[test]
fn iana_names_resolve() {
    let taipei = zone_from_name("Asia/Taipei");
    let ts = Timestamp::from_second(0).unwrap();
    assert_eq!(taipei.to_offset_info(ts).offset().seconds(), 8 * 3600);
    assert_eq!(zone_from_name(":Asia/Taipei"), taipei);
}

#[test]
fn unknown_names_fall_back_to_utc() {
    assert_eq!(zone_from_name("No/Such_Zone"), TimeZone::UTC);
}

#[test]
fn posix_style_fixed_zones() {
    let ts = Timestamp::from_second(0).unwrap();
    // POSIX `JST-9` means UTC+9.
    assert_eq!(
        zone_from_name("JST-9")
            .to_offset_info(ts)
            .offset()
            .seconds(),
        9 * 3600
    );
    // A bare name with a positive value is west of UTC.
    assert_eq!(
        zone_from_name("EST5").to_offset_info(ts).offset().seconds(),
        -5 * 3600
    );
    assert_eq!(
        zone_from_name("ABC-5:30")
            .to_offset_info(ts)
            .offset()
            .seconds(),
        5 * 3600 + 30 * 60
    );
    assert_eq!(posix_fixed_zone("AB+1"), None);
    assert_eq!(posix_fixed_zone("ABC+99"), None);
}

#[test]
fn format_in_uses_the_given_zone() {
    let ts = Timestamp::from_second(1_704_207_845).unwrap();
    assert_eq!(
        format_in("%F %T %Z", ts, &TimeZone::UTC),
        "2024-01-02 15:04:05 UTC"
    );
    let taipei = zone_from_name("Asia/Taipei");
    assert_eq!(
        format_in("%F %T %z", ts, &taipei),
        "2024-01-02 23:04:05 +0800"
    );
}

#[test]
fn system_time_converts_before_and_after_epoch() {
    let after = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(42);
    assert_eq!(from_system_time(after).as_second(), 42);
    let before = SystemTime::UNIX_EPOCH - std::time::Duration::from_secs(7);
    assert_eq!(from_system_time(before).as_second(), -7);
}

#[test]
fn now_is_after_2020() {
    assert!(now().as_second() > 1_577_836_800);
}
