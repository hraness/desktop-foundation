use std::time::{SystemTime, UNIX_EPOCH};

/// RFC 3339 UTC with milliseconds, the same text as JavaScript's
/// `Date.prototype.toISOString`.
pub fn iso8601(at: SystemTime) -> String {
    let millis = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let secs = millis.div_euclid(1000);
    let ms = millis.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn matches_javascript_iso_strings() {
        assert_eq!(iso8601(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        // new Date(1790553600123).toISOString()
        assert_eq!(
            iso8601(UNIX_EPOCH + Duration::from_millis(1_790_553_600_123)),
            "2026-09-28T00:00:00.123Z"
        );
        // new Date(951782400000).toISOString(): a leap day
        assert_eq!(
            iso8601(UNIX_EPOCH + Duration::from_millis(951_782_400_000)),
            "2000-02-29T00:00:00.000Z"
        );
    }
}
