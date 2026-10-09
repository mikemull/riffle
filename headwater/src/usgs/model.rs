use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, NaiveDate, Utc};

/// When a reading applies. The two USGS services differ in kind, not just
/// precision: a daily value summarizes a *local* calendar day, so it's kept
/// as a plain date rather than pinned to some instant; a continuous reading
/// is an instant, normalized to UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadingTime {
    Date(NaiveDate),
    Instant(DateTime<Utc>),
}

impl ReadingTime {
    /// Parse the API's `time` field: a bare `YYYY-MM-DD` for daily values,
    /// an RFC 3339 timestamp with offset for continuous ones.
    pub fn parse(s: &str) -> Result<Self> {
        if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return Ok(Self::Date(date));
        }
        DateTime::parse_from_rfc3339(s)
            .map(|dt| Self::Instant(dt.with_timezone(&Utc)))
            .with_context(|| format!("unrecognized reading time '{s}'"))
    }

    /// The calendar date (UTC date, for an instant).
    pub fn date(&self) -> NaiveDate {
        match self {
            Self::Date(d) => *d,
            Self::Instant(dt) => dt.date_naive(),
        }
    }

    pub fn year(&self) -> i32 {
        self.date().year()
    }
}

#[derive(Debug, Clone)]
pub struct SiteReading {
    pub site_no: String,
    pub param_cd: String,
    pub datetime: ReadingTime,
    pub value: f64,
    pub qualifiers: String,
    pub latitude: f64,
    pub longitude: f64,
    pub approval_status: String,
    pub last_modified: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_daily_date() {
        let t = ReadingTime::parse("2025-06-01").unwrap();
        assert_eq!(t, ReadingTime::Date(NaiveDate::from_ymd_opt(2025, 6, 1).unwrap()));
    }

    #[test]
    fn parses_continuous_instant_to_utc() {
        let t = ReadingTime::parse("2025-06-01T00:15:00-05:00").unwrap();
        let ReadingTime::Instant(dt) = t else { panic!("expected an instant") };
        assert_eq!(dt.to_rfc3339(), "2025-06-01T05:15:00+00:00");
    }

    #[test]
    fn rejects_garbage() {
        assert!(ReadingTime::parse("June 1").is_err());
    }
}
