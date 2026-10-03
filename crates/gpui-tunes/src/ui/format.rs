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
    fn formats_hours() {
        assert_eq!(fmt_duration(Duration::from_hours(1)), "1:00:00");
        assert_eq!(fmt_duration(Duration::from_secs(3_661)), "1:01:01");
    }
}
