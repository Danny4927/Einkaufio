//! RFC 3339 (UTC) rendering of domain timestamps for transport DTOs.

use einkaufio_application::Timestamp;

/// Formats whole seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn rfc3339(timestamp: Timestamp) -> String {
    let seconds = timestamp.as_unix_seconds();
    let days = seconds / 86_400;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3_600,
        time % 3_600 / 60,
        time % 60
    )
}

/// Proleptic Gregorian date of a day count since 1970-01-01 (H. Hinnant's algorithm,
/// restricted to non-negative day counts).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        for (seconds, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_709_208_000, "2024-02-29T12:00:00Z"),
            (1_798_761_599, "2026-12-31T23:59:59Z"),
            (4_102_444_800, "2100-01-01T00:00:00Z"),
        ] {
            assert_eq!(rfc3339(Timestamp::from_unix_seconds(seconds)), expected);
        }
    }
}
