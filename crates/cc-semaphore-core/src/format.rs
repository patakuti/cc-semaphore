//! Elapsed-time formatting for the session list. See 02_design.md §4.2.

/// Formats an elapsed duration (in whole seconds, clamped to >= 0) as
/// `"12s"`, `"7m12s"`, or `"2h41m"`.
pub fn format_elapsed(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

/// Convenience wrapper: elapsed seconds between a `since` epoch-ms timestamp
/// and `now` epoch-ms.
pub fn elapsed_since_ms(since_ms: i64, now_ms: i64) -> String {
    format_elapsed((now_ms - since_ms) / 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_seconds() {
        assert_eq!(format_elapsed(0), "0s");
        assert_eq!(format_elapsed(12), "12s");
        assert_eq!(format_elapsed(59), "59s");
    }

    #[test]
    fn formats_minutes() {
        assert_eq!(format_elapsed(60), "1m0s");
        assert_eq!(format_elapsed(432), "7m12s");
        assert_eq!(format_elapsed(3599), "59m59s");
    }

    #[test]
    fn formats_hours() {
        assert_eq!(format_elapsed(3600), "1h0m");
        assert_eq!(format_elapsed(9660), "2h41m");
    }

    #[test]
    fn clamps_negative_to_zero() {
        assert_eq!(format_elapsed(-5), "0s");
    }
}
