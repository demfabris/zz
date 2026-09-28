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
    let offset = platform_offset(timestamp)?;
    LAST_OFFSET.set(Some((timestamp, offset)));
    Some(offset)
}

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
    use chrono::{Local, Offset as _};

    use super::*;

    #[test]
    fn offsets_match_the_platform_zone_across_a_year() {
        let start = 1_767_225_600_i64;
        for day in 0..366 {
            for hour in [0, 1, 2, 3, 12, 23] {
                let timestamp = start + day * 86_400 + hour * 3_600;
                let expected = Local.timestamp_opt(timestamp, 0).single().expect("local");
                let actual = local_time(timestamp).expect("local time");
                assert_eq!(actual, expected);
                assert_eq!(
                    actual.offset().local_minus_utc(),
                    expected.offset().fix().local_minus_utc()
                );
                assert_eq!(
                    actual.format("%a %b %e %H:%M:%S %Y %z").to_string(),
                    expected.format("%a %b %e %H:%M:%S %Y %z").to_string()
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
