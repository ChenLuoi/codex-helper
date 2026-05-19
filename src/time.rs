use crate::error::AppError;
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, TimeZone,
    Timelike,
};

pub const ALL_USAGE_RANGE_START: &str = "1900-01-01T00:00:00+00:00";
pub const ALL_USAGE_RANGE_END: &str = "9999-12-31T23:59:59.999+00:00";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DateBound {
    Start,
    End,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DateRange {
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct RawRangeOptions {
    pub start: Option<String>,
    pub end: Option<String>,
    pub all: bool,
    pub today: bool,
    pub yesterday: bool,
    pub month: bool,
    pub last: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StatGroupBy {
    Hour,
    Day,
    Week,
    Month,
    Model,
    Cwd,
    Account,
}

impl StatGroupBy {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "hour" => Ok(Self::Hour),
            "day" => Ok(Self::Day),
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            "model" => Ok(Self::Model),
            "cwd" => Ok(Self::Cwd),
            "account" => Ok(Self::Account),
            _ => Err(AppError::invalid_input(
                "Invalid group value. Expected one of: hour, day, week, month, model, cwd, account.",
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Model => "model",
            Self::Cwd => "cwd",
            Self::Account => "account",
        }
    }
}

pub fn parse_date_bound(
    value: &str,
    bound: DateBound,
    offset: FixedOffset,
) -> Result<DateTime<FixedOffset>, AppError> {
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let time = match bound {
            DateBound::Start => date.and_hms_milli_opt(0, 0, 0, 0),
            DateBound::End => date.and_hms_milli_opt(23, 59, 59, 999),
        }
        .expect("valid date-only time");
        return local_datetime(offset, time, value);
    }

    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Ok(date);
    }

    if let Ok(date) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return local_datetime(offset, date, value);
    }

    if let Ok(date) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M") {
        return local_datetime(offset, date, value);
    }

    let name = match bound {
        DateBound::Start => "start",
        DateBound::End => "end",
    };
    Err(AppError::invalid_input(format!(
        "Invalid {name} time: {value}"
    )))
}

pub fn resolve_date_range(
    raw: &RawRangeOptions,
    now: DateTime<FixedOffset>,
) -> Result<DateRange, AppError> {
    let quick_ranges = [
        raw.all,
        raw.today,
        raw.yesterday,
        raw.month,
        raw.last.is_some(),
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count();

    if quick_ranges > 1 {
        return Err(AppError::invalid_input(
            "Use only one quick range option: --all, --today, --yesterday, --month, or --last.",
        ));
    }

    if quick_ranges == 1 && (raw.start.is_some() || raw.end.is_some()) {
        return Err(AppError::invalid_input(
            "Quick range options cannot be combined with --start or --end.",
        ));
    }

    let offset = *now.offset();

    if raw.all {
        return Ok(DateRange {
            start: DateTime::parse_from_rfc3339(ALL_USAGE_RANGE_START).expect("valid all start"),
            end: DateTime::parse_from_rfc3339(ALL_USAGE_RANGE_END).expect("valid all end"),
        });
    }

    if raw.today {
        return Ok(DateRange {
            start: start_of_day(now),
            end: now,
        });
    }

    if raw.yesterday {
        let yesterday = start_of_day(now) - Duration::days(1);
        return Ok(DateRange {
            start: yesterday,
            end: end_of_day(yesterday),
        });
    }

    if raw.month {
        return Ok(DateRange {
            start: local_datetime(
                offset,
                NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
                    .expect("valid month start")
                    .and_hms_milli_opt(0, 0, 0, 0)
                    .expect("valid month start time"),
                "month start",
            )?,
            end: now,
        });
    }

    if let Some(last) = &raw.last {
        return Ok(DateRange {
            start: now - Duration::milliseconds(parse_duration_ms(last)?),
            end: now,
        });
    }

    let end = match &raw.end {
        Some(end) => parse_date_bound(end, DateBound::End, offset)?,
        None => now,
    };
    let start = match &raw.start {
        Some(start) => parse_date_bound(start, DateBound::Start, offset)?,
        None => end - Duration::days(7),
    };

    Ok(DateRange { start, end })
}

pub fn parse_duration_ms(value: &str) -> Result<i64, AppError> {
    let trimmed = value.trim();
    let digits = trimmed
        .chars()
        .take_while(|char| char.is_ascii_digit())
        .collect::<String>();
    let unit = &trimmed[digits.len()..];

    if digits.is_empty() || !matches!(unit, "h" | "d" | "w" | "mo") {
        return Err(AppError::invalid_input(
            "Invalid --last value. Use a duration like 12h, 7d, 2w, or 1mo.",
        ));
    }

    let amount = digits.parse::<i64>().map_err(|_| {
        AppError::invalid_input("Invalid --last value. Duration must be a positive integer.")
    })?;
    if amount <= 0 {
        return Err(AppError::invalid_input(
            "Invalid --last value. Duration must be a positive integer.",
        ));
    }

    let hours = match unit {
        "h" => amount,
        "d" => amount * 24,
        "w" => amount * 7 * 24,
        "mo" => amount * 30 * 24,
        _ => unreachable!("validated unit"),
    };

    Ok(hours * 60 * 60 * 1000)
}

pub fn resolve_group_by(
    explicit: Option<&str>,
    raw: &RawRangeOptions,
    range: &DateRange,
) -> Result<StatGroupBy, AppError> {
    if let Some(value) = explicit {
        return StatGroupBy::parse(value);
    }

    if raw.all {
        return Ok(StatGroupBy::Month);
    }

    if raw.month {
        return Ok(StatGroupBy::Day);
    }

    let duration = range.end - range.start;
    if duration <= Duration::hours(48) {
        return Ok(StatGroupBy::Hour);
    }

    if duration <= Duration::days(31) {
        return Ok(StatGroupBy::Day);
    }

    if range.end <= add_months(range.start, 6)? {
        return Ok(StatGroupBy::Week);
    }

    Ok(StatGroupBy::Month)
}

fn start_of_day(date: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    local_datetime(
        *date.offset(),
        NaiveDate::from_ymd_opt(date.year(), date.month(), date.day())
            .expect("valid date")
            .and_hms_milli_opt(0, 0, 0, 0)
            .expect("valid start of day"),
        "start of day",
    )
    .expect("fixed offset start of day")
}

fn end_of_day(date: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    local_datetime(
        *date.offset(),
        NaiveDate::from_ymd_opt(date.year(), date.month(), date.day())
            .expect("valid date")
            .and_hms_milli_opt(23, 59, 59, 999)
            .expect("valid end of day"),
        "end of day",
    )
    .expect("fixed offset end of day")
}

fn add_months(date: DateTime<FixedOffset>, months: i32) -> Result<DateTime<FixedOffset>, AppError> {
    let month_zero = date.month0() as i32 + months;
    let year = date.year() + month_zero.div_euclid(12);
    let month = month_zero.rem_euclid(12) as u32 + 1;
    let day = date.day().min(days_in_month(year, month));
    let naive = NaiveDate::from_ymd_opt(year, month, day)
        .expect("valid adjusted month")
        .and_hms_nano_opt(date.hour(), date.minute(), date.second(), date.nanosecond())
        .expect("valid adjusted time");
    local_datetime(*date.offset(), naive, "month adjustment")
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let next = NaiveDate::from_ymd_opt(next_year, next_month, 1).expect("valid next month");
    (next - Duration::days(1)).day()
}

fn local_datetime(
    offset: FixedOffset,
    date: NaiveDateTime,
    value: &str,
) -> Result<DateTime<FixedOffset>, AppError> {
    match offset.from_local_datetime(&date) {
        LocalResult::Single(date) => Ok(date),
        _ => Err(AppError::invalid_input(format!(
            "Invalid local time: {value}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset() -> FixedOffset {
        FixedOffset::east_opt(8 * 60 * 60).expect("offset")
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-05-17T12:34:56.000+08:00").expect("now")
    }

    #[test]
    fn parses_date_only_bounds_as_local_day_edges() {
        let start = parse_date_bound("2026-05-01", DateBound::Start, offset()).expect("start");
        let end = parse_date_bound("2026-05-01", DateBound::End, offset()).expect("end");

        assert_eq!(start.to_rfc3339(), "2026-05-01T00:00:00+08:00");
        assert_eq!(end.to_rfc3339(), "2026-05-01T23:59:59.999+08:00");
    }

    #[test]
    fn resolves_quick_ranges() {
        let range = resolve_date_range(
            &RawRangeOptions {
                today: true,
                ..RawRangeOptions::default()
            },
            now(),
        )
        .expect("range");

        assert_eq!(range.start.to_rfc3339(), "2026-05-17T00:00:00+08:00");
        assert_eq!(range.end.to_rfc3339(), "2026-05-17T12:34:56+08:00");

        let yesterday = resolve_date_range(
            &RawRangeOptions {
                yesterday: true,
                ..RawRangeOptions::default()
            },
            now(),
        )
        .expect("range");
        assert_eq!(yesterday.start.to_rfc3339(), "2026-05-16T00:00:00+08:00");
        assert_eq!(yesterday.end.to_rfc3339(), "2026-05-16T23:59:59.999+08:00");
    }

    #[test]
    fn parses_last_durations_like_typescript() {
        assert_eq!(parse_duration_ms("12h").expect("duration"), 43_200_000);
        assert_eq!(parse_duration_ms("7d").expect("duration"), 604_800_000);
        assert_eq!(parse_duration_ms("2w").expect("duration"), 1_209_600_000);
        assert_eq!(parse_duration_ms("1mo").expect("duration"), 2_592_000_000);
        assert!(parse_duration_ms("0d").is_err());
        assert!(parse_duration_ms("3m").is_err());
    }

    #[test]
    fn rejects_conflicting_quick_ranges() {
        let error = resolve_date_range(
            &RawRangeOptions {
                today: true,
                last: Some("12h".to_string()),
                ..RawRangeOptions::default()
            },
            now(),
        )
        .expect_err("conflict");

        assert_eq!(
            error.message(),
            "Use only one quick range option: --all, --today, --yesterday, --month, or --last."
        );
    }

    #[test]
    fn resolves_default_group_by_from_range() {
        let raw = RawRangeOptions::default();
        let hour_range = DateRange {
            start: now() - Duration::hours(12),
            end: now(),
        };
        let day_range = DateRange {
            start: now() - Duration::days(7),
            end: now(),
        };
        let week_range = DateRange {
            start: now() - Duration::days(90),
            end: now(),
        };
        let month_range = DateRange {
            start: now() - Duration::days(220),
            end: now(),
        };

        assert_eq!(
            resolve_group_by(None, &raw, &hour_range).expect("group"),
            StatGroupBy::Hour
        );
        assert_eq!(
            resolve_group_by(None, &raw, &day_range).expect("group"),
            StatGroupBy::Day
        );
        assert_eq!(
            resolve_group_by(None, &raw, &week_range).expect("group"),
            StatGroupBy::Week
        );
        assert_eq!(
            resolve_group_by(None, &raw, &month_range).expect("group"),
            StatGroupBy::Month
        );
    }

    #[test]
    fn all_and_month_override_default_group_by() {
        let range = DateRange {
            start: now() - Duration::hours(1),
            end: now(),
        };

        assert_eq!(
            resolve_group_by(
                None,
                &RawRangeOptions {
                    all: true,
                    ..RawRangeOptions::default()
                },
                &range
            )
            .expect("group"),
            StatGroupBy::Month
        );
        assert_eq!(
            resolve_group_by(
                None,
                &RawRangeOptions {
                    month: true,
                    ..RawRangeOptions::default()
                },
                &range
            )
            .expect("group"),
            StatGroupBy::Day
        );
        assert_eq!(
            resolve_group_by(Some("cwd"), &RawRangeOptions::default(), &range).expect("group"),
            StatGroupBy::Cwd
        );
    }
}
