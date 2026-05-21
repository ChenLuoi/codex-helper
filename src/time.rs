use crate::error::AppError;
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, NaiveDate, Offset, TimeZone,
    Timelike, Utc,
};

pub const ALL_USAGE_RANGE_START: &str = "1900-01-01T00:00:00+00:00";
pub const ALL_USAGE_RANGE_END: &str = "9999-12-31T23:59:59.999+00:00";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DateBound {
    Start,
    End,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct DateRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
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
                "Invalid group-by value. Expected one of: hour, day, week, month, model, cwd, account.",
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

pub fn parse_date_bound(value: &str, bound: DateBound) -> Result<DateTime<Utc>, AppError> {
    if value.len() == 10 {
        let parts = value.split('-').collect::<Vec<_>>();
        if parts.len() == 3 {
            if let (Ok(year), Ok(month), Ok(day)) = (
                parts[0].parse::<i32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            ) {
                let parsed = match bound {
                    DateBound::Start => local_to_utc_checked(year, month, day, 0, 0, 0, 0),
                    DateBound::End => local_to_utc_checked(year, month, day, 23, 59, 59, 999),
                };
                return parsed.ok_or_else(|| invalid_time_error(bound, value));
            }
        }
    }

    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Ok(date.with_timezone(&Utc));
    }

    for pattern in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(date) = chrono::NaiveDateTime::parse_from_str(value, pattern) {
            return local_naive_to_utc(date, value);
        }
    }

    Err(invalid_time_error(bound, value))
}

pub fn resolve_date_range(
    raw: &RawRangeOptions,
    now: DateTime<Utc>,
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
        return Err(AppError::new(
            "Use only one quick range option: --all, --today, --yesterday, --month, or --last.",
        ));
    }

    if quick_ranges == 1 && (raw.start.is_some() || raw.end.is_some()) {
        return Err(AppError::new(
            "Quick range options cannot be combined with --start or --end.",
        ));
    }

    if raw.all {
        return Ok(DateRange {
            start: local_to_utc(1900, 1, 1, 0, 0, 0, 0),
            end: local_to_utc(9999, 12, 31, 23, 59, 59, 999),
        });
    }

    if raw.today {
        let local = now.with_timezone(&Local);
        return Ok(DateRange {
            start: local_to_utc(local.year(), local.month(), local.day(), 0, 0, 0, 0),
            end: now,
        });
    }

    if raw.yesterday {
        let local = now.with_timezone(&Local);
        let start_today = local_to_utc(local.year(), local.month(), local.day(), 0, 0, 0, 0);
        let start = start_today - Duration::days(1);
        return Ok(DateRange {
            start,
            end: start + Duration::days(1) - Duration::milliseconds(1),
        });
    }

    if raw.month {
        let local = now.with_timezone(&Local);
        return Ok(DateRange {
            start: local_to_utc(local.year(), local.month(), 1, 0, 0, 0, 0),
            end: now,
        });
    }

    if let Some(last) = &raw.last {
        let duration_ms = parse_duration_ms(last)?;
        let start = now
            .checked_sub_signed(Duration::milliseconds(duration_ms))
            .ok_or_else(|| {
                AppError::invalid_input("Invalid --last value. Duration is too large.")
            })?;
        return Ok(DateRange { start, end: now });
    }

    let end = match &raw.end {
        Some(end) => parse_date_bound(end, DateBound::End)?,
        None => now,
    };
    let start = match &raw.start {
        Some(start) => parse_date_bound(start, DateBound::Start)?,
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
        "h" => Some(amount),
        "d" => amount.checked_mul(24),
        "w" => amount
            .checked_mul(7)
            .and_then(|amount| amount.checked_mul(24)),
        "mo" => amount
            .checked_mul(30)
            .and_then(|amount| amount.checked_mul(24)),
        _ => unreachable!("validated unit"),
    }
    .ok_or_else(|| AppError::invalid_input("Invalid --last value. Duration is too large."))?;

    hours
        .checked_mul(60)
        .and_then(|minutes| minutes.checked_mul(60))
        .and_then(|seconds| seconds.checked_mul(1000))
        .ok_or_else(|| AppError::invalid_input("Invalid --last value. Duration is too large."))
}

fn invalid_time_error(bound: DateBound, value: &str) -> AppError {
    let name = match bound {
        DateBound::Start => "start",
        DateBound::End => "end",
    };
    AppError::invalid_input(format!("Invalid {name} time: {value}"))
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

    if range.end <= add_months_local(range.start, 6)? {
        return Ok(StatGroupBy::Week);
    }

    Ok(StatGroupBy::Month)
}

fn add_months_local(date: DateTime<Utc>, months: i32) -> Result<DateTime<Utc>, AppError> {
    let local = date.with_timezone(&Local);
    let month_zero = local.month0() as i32 + months;
    let year = local.year() + month_zero.div_euclid(12);
    let month = month_zero.rem_euclid(12) as u32 + 1;
    let day = local.day().min(days_in_month(year, month));
    local_to_utc_checked(
        year,
        month,
        day,
        local.hour(),
        local.minute(),
        local.second(),
        local.timestamp_subsec_millis(),
    )
    .ok_or_else(|| AppError::new("Invalid local time: month adjustment"))
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

fn local_naive_to_utc(date: chrono::NaiveDateTime, value: &str) -> Result<DateTime<Utc>, AppError> {
    match Local.from_local_datetime(&date) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => Ok(earliest.with_timezone(&Utc)),
        LocalResult::None => Err(AppError::new(format!("Invalid local time: {value}"))),
    }
}

pub fn local_to_utc(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
) -> DateTime<Utc> {
    local_to_utc_checked(year, month, day, hour, minute, second, millis).expect("valid local date")
}

pub fn local_to_utc_checked(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
) -> Option<DateTime<Utc>> {
    let local_result = Local.with_ymd_and_hms(year, month, day, hour, minute, second);
    match local_result {
        LocalResult::Single(value) => value
            .with_nanosecond(millis * 1_000_000)
            .map(|value| value.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => earliest
            .with_nanosecond(millis * 1_000_000)
            .map(|value| value.with_timezone(&Utc)),
        LocalResult::None => {
            let offset_seconds = Local::now().offset().fix().local_minus_utc();
            let offset = FixedOffset::east_opt(offset_seconds)?;
            offset
                .with_ymd_and_hms(year, month, day, hour, minute, second)
                .single()?
                .with_nanosecond(millis * 1_000_000)
                .map(|value| value.with_timezone(&Utc))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-17T04:34:56.000Z")
            .expect("now")
            .with_timezone(&Utc)
    }

    #[test]
    fn parses_date_only_bounds_as_local_day_edges() {
        let start = parse_date_bound("2026-05-01", DateBound::Start)
            .expect("start")
            .with_timezone(&Local);
        let end = parse_date_bound("2026-05-01", DateBound::End)
            .expect("end")
            .with_timezone(&Local);

        assert_eq!(
            (
                start.year(),
                start.month(),
                start.day(),
                start.hour(),
                start.minute()
            ),
            (2026, 5, 1, 0, 0)
        );
        assert_eq!(
            (
                end.year(),
                end.month(),
                end.day(),
                end.hour(),
                end.minute(),
                end.second(),
                end.timestamp_subsec_millis()
            ),
            (2026, 5, 1, 23, 59, 59, 999)
        );
    }

    #[test]
    fn parses_local_t_separator_bounds_like_stats_cli() {
        let parsed = parse_date_bound("2026-05-01T12:34", DateBound::Start)
            .expect("local datetime")
            .with_timezone(&Local);

        assert_eq!(
            (
                parsed.year(),
                parsed.month(),
                parsed.day(),
                parsed.hour(),
                parsed.minute()
            ),
            (2026, 5, 1, 12, 34)
        );
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

        let start = range.start.with_timezone(&Local);
        assert_eq!(
            (
                start.year(),
                start.month(),
                start.day(),
                start.hour(),
                start.minute()
            ),
            (2026, 5, 17, 0, 0)
        );
        assert_eq!(range.end, now());

        let yesterday = resolve_date_range(
            &RawRangeOptions {
                yesterday: true,
                ..RawRangeOptions::default()
            },
            now(),
        )
        .expect("range");
        let yesterday_start = yesterday.start.with_timezone(&Local);
        let yesterday_end = yesterday.end.with_timezone(&Local);
        assert_eq!(
            (
                yesterday_start.year(),
                yesterday_start.month(),
                yesterday_start.day(),
                yesterday_start.hour(),
                yesterday_start.minute()
            ),
            (2026, 5, 16, 0, 0)
        );
        assert_eq!(
            (
                yesterday_end.year(),
                yesterday_end.month(),
                yesterday_end.day(),
                yesterday_end.hour(),
                yesterday_end.minute(),
                yesterday_end.second(),
                yesterday_end.timestamp_subsec_millis()
            ),
            (2026, 5, 16, 23, 59, 59, 999)
        );
    }

    #[test]
    fn parses_last_durations_like_typescript() {
        assert_eq!(parse_duration_ms("12h").expect("duration"), 43_200_000);
        assert_eq!(parse_duration_ms("7d").expect("duration"), 604_800_000);
        assert_eq!(parse_duration_ms("2w").expect("duration"), 1_209_600_000);
        assert_eq!(parse_duration_ms("1mo").expect("duration"), 2_592_000_000);
        assert!(parse_duration_ms("0d").is_err());
        assert!(parse_duration_ms("3m").is_err());
        assert!(parse_duration_ms("9223372036854775807d").is_err());
    }

    #[test]
    fn invalid_date_only_bounds_return_errors() {
        let error = parse_date_bound("2026-02-31", DateBound::Start).expect_err("invalid date");

        assert_eq!(error.message(), "Invalid start time: 2026-02-31");
        assert_eq!(error.exit_code(), 2);
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
