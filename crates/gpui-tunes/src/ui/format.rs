//! Text formatting helpers for the views.

use std::time::Duration;

/// Format as `m:ss`, or `h:mm:ss` from one hour.
pub fn fmt_duration(d: Duration) -> String {
    let total = d.as_secs();
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Format a running total: `m:ss`, `h:mm:ss` from one hour, `d:hh:mm:ss` from one day.
pub fn fmt_total(d: Duration) -> String {
    let total = d.as_secs();
    let (days, rest) = (total / 86_400, total % 86_400);
    if days == 0 {
        return fmt_duration(Duration::from_secs(rest));
    }
    let (h, m, s) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    format!("{days}:{h:02}:{m:02}:{s:02}")
}

/// Status-bar text for a list of `count` tracks lasting `total`.
pub fn fmt_status(count: usize, total: Duration) -> String {
    let noun = if count == 1 { "song" } else { "songs" };
    format!("{count} {noun}, {}", fmt_total(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_minutes_and_seconds() {
        assert_eq!(fmt_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(fmt_duration(Duration::from_secs(65)), "1:05");
        assert_eq!(fmt_duration(Duration::from_millis(185_900)), "3:05");
    }

    #[test]
    fn total_switches_units_at_hour_and_day() {
        assert_eq!(fmt_total(Duration::from_secs(59)), "0:59");
        assert_eq!(fmt_total(Duration::from_secs(3_599)), "59:59");
        assert_eq!(fmt_total(Duration::from_hours(1)), "1:00:00");
        assert_eq!(fmt_total(Duration::from_secs(86_399)), "23:59:59");
        assert_eq!(fmt_total(Duration::from_hours(24)), "1:00:00:00");
        assert_eq!(fmt_total(Duration::from_secs(111_852)), "1:07:04:12");
    }

    #[test]
    fn status_pluralises_songs() {
        assert_eq!(fmt_status(0, Duration::ZERO), "0 songs, 0:00");
        assert_eq!(fmt_status(1, Duration::from_secs(65)), "1 song, 1:05");
        assert_eq!(
            fmt_status(473, Duration::from_secs(111_852)),
            "473 songs, 1:07:04:12"
        );
    }

    #[test]
    fn formats_hours() {
        assert_eq!(fmt_duration(Duration::from_hours(1)), "1:00:00");
        assert_eq!(fmt_duration(Duration::from_secs(3_661)), "1:01:01");
    }
}
