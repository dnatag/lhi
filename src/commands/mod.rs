mod cat;
mod compact;
mod log;
mod restore;
mod snapshot;
mod watch;

pub use cat::cat;
pub use compact::compact;
pub use log::log;
pub use restore::restore;
pub use snapshot::snapshot;
pub use watch::watch;

use anyhow::{bail, Result};
use chrono::{Duration, Utc};

pub(crate) use crate::util::get_file_mode;

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Parses a "since" duration string (e.g. "5m", "1h") into a UTC timestamp.
fn parse_since(s: &str) -> Result<chrono::DateTime<Utc>> { parse_duration_ago(s) }

/// Parses a "before" time string into a UTC timestamp.
/// Accepts relative durations ("5m"), ISO 8601, or HH:MM local time.
fn parse_before(s: &str) -> Result<chrono::DateTime<Utc>> {
    if let Ok(ts) = parse_duration_ago(s) { return Ok(ts); }
    if let Ok(ts) = s.parse::<chrono::DateTime<Utc>>() { return Ok(ts); }
    if let Ok(naive) = chrono::NaiveTime::parse_from_str(s, "%H:%M") {
        let dt = chrono::Local::now().date_naive().and_time(naive);
        return match dt.and_local_timezone(chrono::Local) {
            chrono::LocalResult::Single(t) => Ok(t.with_timezone(&Utc)),
            chrono::LocalResult::Ambiguous(t, _) => Ok(t.with_timezone(&Utc)),
            chrono::LocalResult::None => bail!("Nonexistent local time (DST transition): '{s}'"),
        };
    }
    bail!("Cannot parse time: '{s}'. Use e.g. '5m', '1h', '30min', '14:30', or ISO 8601.")
}

/// Parses a human-friendly duration string (e.g. "5m", "2 hours", "1d ago")
/// and returns the corresponding UTC timestamp in the past.
fn parse_duration_ago(s: &str) -> Result<chrono::DateTime<Utc>> {
    let s = s.trim().trim_end_matches(" ago").trim();
    let (num_str, unit) = s.find(|c: char| !c.is_ascii_digit())
        .map(|i| (&s[..i], s[i..].trim()))
        .ok_or_else(|| anyhow::anyhow!("Cannot parse: '{s}'"))?;
    let num: i64 = num_str.parse()
        .map_err(|_| anyhow::anyhow!("Not a number: '{num_str}'"))?;
    let duration = match unit {
        "s" | "sec" | "second" | "seconds" => Duration::seconds(num),
        "m" | "min" | "minute" | "minutes" => Duration::minutes(num),
        "h" | "hr" | "hour" | "hours" => Duration::hours(num),
        "d" | "day" | "days" => Duration::days(num),
        _ => bail!("Unknown unit: '{unit}'"),
    };
    Ok(Utc::now() - duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_formats() {
        assert!(parse_duration_ago("5m").is_ok());
        assert!(parse_duration_ago("1h").is_ok());
        assert!(parse_duration_ago("30min").is_ok());
        assert!(parse_duration_ago("2 hours").is_ok());
        assert!(parse_duration_ago("1d").is_ok());
        assert!(parse_duration_ago("bogus").is_err());
    }

    #[test]
    fn parse_duration_ago_suffix() {
        let t = parse_duration_ago("5m ago").unwrap();
        let expected = Utc::now() - Duration::minutes(5);
        assert!((t - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn parse_duration_ago_rejects_bare_number() {
        assert!(parse_duration_ago("42").is_err());
    }

    #[test]
    fn parse_before_iso8601() {
        let t = parse_before("2026-03-14T10:30:00Z").unwrap();
        assert_eq!(t.to_rfc3339(), "2026-03-14T10:30:00+00:00");
    }

    #[test]
    fn parse_before_relative_duration() {
        let t = parse_before("1h").unwrap();
        let expected = Utc::now() - Duration::hours(1);
        assert!((t - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn parse_before_hhmm() {
        let t = parse_before("14:30").unwrap();
        let local = t.with_timezone(&chrono::Local);
        assert_eq!(local.format("%H:%M").to_string(), "14:30");
    }

    #[test]
    fn parse_before_rejects_garbage() {
        assert!(parse_before("not-a-time").is_err());
    }
}
