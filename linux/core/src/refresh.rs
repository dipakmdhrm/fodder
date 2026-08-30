//! Presentation helpers for reporting a completed refresh to the user.
//!
//! The GTK viewer turns a [`crate::ipc::IpcMessage::RefreshFinished`] into a
//! toast; the human-readable summary is built here so it can be unit-tested
//! without any UI.

/// Build the one-line summary shown when a refresh finishes, e.g.
/// `"Up to date · 1.8s"`, `"3 new articles · 2.4s"`, or
/// `"1 new article · 1 error · 0.6s"`.
///
/// - `new_articles` — genuinely-new items inserted across the polled feed(s).
/// - `errors` — feeds whose poll hard-errored (rate-limits/304s are not errors).
/// - `duration_ms` — wall-clock time the refresh took.
pub fn format_refresh_summary(new_articles: usize, errors: usize, duration_ms: u64) -> String {
    let mut parts = Vec::with_capacity(3);

    parts.push(match new_articles {
        0 => "Up to date".to_string(),
        1 => "1 new article".to_string(),
        n => format!("{n} new articles"),
    });

    match errors {
        0 => {}
        1 => parts.push("1 error".to_string()),
        n => parts.push(format!("{n} errors")),
    }

    parts.push(format_duration(duration_ms));

    parts.join(" · ")
}

/// Format a poll duration for display. Sub-second and multi-second refreshes
/// read as `"0.6s"` / `"2.4s"`; anything past a minute switches to `"1m03s"`.
fn format_duration(duration_ms: u64) -> String {
    if duration_ms >= 60_000 {
        let secs = duration_ms / 1000;
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_new_articles_reads_up_to_date() {
        assert_eq!(format_refresh_summary(0, 0, 1800), "Up to date · 1.8s");
    }

    #[test]
    fn singular_vs_plural_articles() {
        assert_eq!(format_refresh_summary(1, 0, 600), "1 new article · 0.6s");
        assert_eq!(format_refresh_summary(3, 0, 2400), "3 new articles · 2.4s");
    }

    #[test]
    fn errors_are_included_and_pluralized() {
        assert_eq!(
            format_refresh_summary(1, 1, 600),
            "1 new article · 1 error · 0.6s"
        );
        assert_eq!(
            format_refresh_summary(0, 2, 900),
            "Up to date · 2 errors · 0.9s"
        );
    }

    #[test]
    fn long_refresh_switches_to_minutes() {
        assert_eq!(format_refresh_summary(0, 0, 63_000), "Up to date · 1m03s");
    }

    #[test]
    fn sub_100ms_still_shows_a_tenth() {
        // Never render "0.0s" as the whole story — it rounds, but the seconds
        // suffix keeps it readable for a near-instant 304-only refresh.
        assert_eq!(format_refresh_summary(0, 0, 40), "Up to date · 0.0s");
    }
}
