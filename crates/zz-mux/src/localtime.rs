use std::cell::Cell;

use chrono::{DateTime, FixedOffset, TimeZone as _};

thread_local! {
    static LAST_OFFSET: Cell<Option<(i64, i32)>> = const { Cell::new(None) };
}

pub fn local_time(timestamp: i64) -> Option<DateTime<FixedOffset>> {
    FixedOffset::east_opt(offset_seconds(timestamp)?)?
        .timestamp_opt(timestamp, 0)
        .single()
}

fn offset_seconds(timestamp: i64) -> Option<i32> {
    if let Some((second, offset)) = LAST_OFFSET.get()
        && second == timestamp
    {
        return Some(offset);
    }
    follow_zone_changes();
    let offset = platform_offset(timestamp)?;
    LAST_OFFSET.set(Some((timestamp, offset)));
    Some(offset)
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
fn follow_zone_changes() {
    unsafe extern "C" {
        fn tzset();
    }
    thread_local! {
        static LAST_TZSET: Cell<u64> = const { Cell::new(u64::MAX) };
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    if LAST_TZSET.replace(now) != now {
        unsafe { tzset() };
    }
}

#[cfg(not(target_os = "linux"))]
const fn follow_zone_changes() {}

#[cfg(unix)]
#[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
fn platform_offset(timestamp: i64) -> Option<i32> {
    let time = libc::time_t::try_from(timestamp).ok()?;
    let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::localtime_r(&raw const time, &raw mut tm) }.is_null() {
        return None;
    }
    i32::try_from(tm.tm_gmtoff).ok()
}

#[cfg(not(unix))]
fn platform_offset(timestamp: i64) -> Option<i32> {
    use chrono::{Local, Offset as _};

    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|time| time.offset().fix().local_minus_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[allow(unsafe_code, clippy::undocumented_unsafe_blocks)]
    fn expected_fields(timestamp: i64) -> (i32, u32, u32, u32, u32, u32, i32) {
        let time = libc::time_t::try_from(timestamp).expect("time_t");
        let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
        assert!(!unsafe { libc::localtime_r(&raw const time, &raw mut tm) }.is_null());
        let field = |value: libc::c_int| u32::try_from(value).expect("tm field");
        (
            tm.tm_year + 1900,
            field(tm.tm_mon + 1),
            field(tm.tm_mday),
            field(tm.tm_hour),
            field(tm.tm_min),
            field(tm.tm_sec),
            i32::try_from(tm.tm_gmtoff).expect("offset"),
        )
    }

    #[cfg(not(unix))]
    fn expected_fields(timestamp: i64) -> (i32, u32, u32, u32, u32, u32, i32) {
        use chrono::{Datelike as _, Local, Offset as _, Timelike as _};

        let time = Local.timestamp_opt(timestamp, 0).single().expect("local");
        (
            time.year(),
            time.month(),
            time.day(),
            time.hour(),
            time.minute(),
            time.second(),
            time.offset().fix().local_minus_utc(),
        )
    }

    #[test]
    fn fields_match_the_platform_zone_across_a_year() {
        use chrono::{Datelike as _, Timelike as _};

        let start = 1_767_225_600_i64;
        for day in 0..366 {
            for hour in [0, 1, 2, 3, 12, 23] {
                let timestamp = start + day * 86_400 + hour * 3_600 + 17;
                let actual = local_time(timestamp).expect("local time");
                assert_eq!(actual.timestamp(), timestamp);
                assert_eq!(
                    (
                        actual.year(),
                        actual.month(),
                        actual.day(),
                        actual.hour(),
                        actual.minute(),
                        actual.second(),
                        actual.offset().local_minus_utc(),
                    ),
                    expected_fields(timestamp),
                    "{timestamp}"
                );
            }
        }
    }

    #[test]
    fn a_repeated_second_reuses_its_offset() {
        let timestamp = 1_790_000_000;
        assert_eq!(local_time(timestamp), local_time(timestamp));
        assert_eq!(LAST_OFFSET.get().map(|(second, _)| second), Some(timestamp));
    }
}
