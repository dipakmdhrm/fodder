//! Pure poll-scheduling computations, kept free of clocks and I/O for testability.

pub const INTERVAL_CHOICES_MINUTES: [u32; 6] = [15, 30, 60, 240, 720, 1440];

/// A feed is due if it has never been polled, its interval has elapsed, or its
/// last-poll timestamp is in the future (clock moved backwards).
pub fn is_due(last_polled_at: Option<i64>, interval_minutes: u32, now: i64) -> bool {
    match last_polled_at {
        None => true,
        Some(t) if t > now => true,
        Some(t) => now >= t + i64::from(interval_minutes) * 60,
    }
}

/// Seconds until the earliest feed becomes due (0 if one is due now).
/// `None` when there are no feeds at all.
pub fn seconds_until_next_due<I>(last_polls: I, interval_minutes: u32, now: i64) -> Option<u64>
where
    I: IntoIterator<Item = Option<i64>>,
{
    last_polls
        .into_iter()
        .map(|lp| match lp {
            None => 0,
            Some(t) if t > now => 0,
            Some(t) => (t + i64::from(interval_minutes) * 60 - now).max(0) as u64,
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u32 = 60;

    #[test]
    fn never_polled_is_due() {
        assert!(is_due(None, HOUR, 1_000));
    }

    #[test]
    fn due_exactly_at_interval() {
        assert!(!is_due(Some(1_000), HOUR, 1_000 + 3_599));
        assert!(is_due(Some(1_000), HOUR, 1_000 + 3_600));
    }

    #[test]
    fn future_last_poll_is_due() {
        assert!(is_due(Some(10_000), HOUR, 1_000));
    }

    #[test]
    fn next_due_picks_earliest() {
        let feeds = vec![Some(1_000), Some(2_000)];
        // 1h interval, now=3000: first due at 4600 (1600s), second at 5600 (2600s)
        assert_eq!(seconds_until_next_due(feeds, HOUR, 3_000), Some(1_600));
    }

    #[test]
    fn next_due_zero_when_overdue() {
        assert_eq!(
            seconds_until_next_due(vec![Some(0), Some(9_999)], HOUR, 5_000),
            Some(0)
        );
    }

    #[test]
    fn next_due_none_without_feeds() {
        assert_eq!(seconds_until_next_due(vec![], HOUR, 5_000), None);
    }

    #[test]
    fn interval_change_reschedules() {
        // polled at 1000; at now=2000 a 15m interval is due, a 1h one is not
        assert!(is_due(Some(1_000), 15, 2_000));
        assert!(!is_due(Some(1_000), HOUR, 2_000));
    }
}
