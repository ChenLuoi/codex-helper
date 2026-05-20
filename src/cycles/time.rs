use crate::error::AppError;
use chrono::{
    DateTime, Datelike, FixedOffset, Local, LocalResult, Offset, SecondsFormat, TimeZone, Timelike,
    Utc,
};

pub(super) struct ParsedWeeklyCycleAnchorTime {
    pub(super) at: DateTime<Utc>,
    pub(super) at_iso: String,
    pub(super) input: String,
    pub(super) time_zone: String,
}

#[derive(Clone, Copy)]
struct AnchorDateTimeParts {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

pub(super) fn parse_cycle_add_times(parts: &[String]) -> Result<Vec<String>, AppError> {
    let tokens = parts
        .iter()
        .flat_map(|part| part.split(','))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut times = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let next = tokens.get(index + 1);
        if next.is_some_and(|next| is_date_only_token(token) && is_time_only_token(next)) {
            times.push(format!("{token} {}", next.expect("checked")));
            index += 2;
        } else {
            times.push(token.clone());
            index += 1;
        }
    }
    if times.is_empty() {
        return Err(AppError::new(
            "cycle add requires at least one weekly cycle start time.",
        ));
    }
    Ok(times)
}

pub(super) fn parse_weekly_cycle_anchor_time(
    input: &str,
) -> Result<ParsedWeeklyCycleAnchorTime, AppError> {
    let trimmed = input.trim();
    let (date_part, time_part, offset_part) = split_anchor_time(trimmed)?;
    let date = date_part.split('-').collect::<Vec<_>>();
    if date.len() != 3 {
        return Err(invalid_anchor_time(input));
    }
    let year = parse_date_part(date[0], "year", input)? as i32;
    let month = parse_date_part(date[1], "month", input)? as u32;
    let day = parse_date_part(date[2], "day", input)? as u32;
    let (hour, minute, second) = match time_part {
        Some(time) => {
            let parts = time.split(':').collect::<Vec<_>>();
            if parts.len() < 2 || parts.len() > 3 {
                return Err(invalid_anchor_time(input));
            }
            (
                parse_date_part(parts[0], "hour", input)? as u32,
                parse_date_part(parts[1], "minute", input)? as u32,
                match parts.get(2) {
                    Some(value) => parse_date_part(value, "second", input)? as u32,
                    None => 0,
                },
            )
        }
        None => (0, 0, 0),
    };
    let parts = AnchorDateTimeParts {
        year,
        month,
        day,
        hour,
        minute,
        second,
    };
    let (at, time_zone) = match offset_part {
        Some(offset) => (
            build_offset_date(parts, offset, input)?,
            format_offset_time_zone(offset),
        ),
        None => (build_local_date(parts, input)?, local_time_zone()),
    };

    Ok(ParsedWeeklyCycleAnchorTime {
        at,
        at_iso: iso_string(at),
        input: trimmed.to_string(),
        time_zone,
    })
}

pub(super) fn assert_iso_timestamp(value: &str, path: &str) -> Result<(), AppError> {
    let date = parse_iso_timestamp(value)
        .ok_or_else(|| AppError::new(format!("Expected {path} to be a UTC ISO timestamp.")))?;
    if iso_string(date) != value {
        return Err(AppError::new(format!(
            "Expected {path} to be a UTC ISO timestamp."
        )));
    }
    Ok(())
}

pub(super) fn parse_iso_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

pub(super) fn iso_string(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(super) fn format_date_time(date: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.year(),
        local.month(),
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
    )
}

pub(super) fn local_date_key(date: DateTime<Utc>) -> String {
    let local = date.with_timezone(&Local);
    format!("{}-{:02}-{:02}", local.year(), local.month(), local.day())
}

pub(super) fn weekly_cycle_anchor_id(date: DateTime<Utc>) -> String {
    format!("anc_{}", compact_iso_timestamp(date))
}

pub(super) fn compact_iso_timestamp(date: DateTime<Utc>) -> String {
    iso_string(date).replace(['-', ':'], "").replace('.', "")
}

fn is_date_only_token(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
}

fn is_time_only_token(value: &str) -> bool {
    let core = value
        .strip_suffix('Z')
        .or_else(|| {
            value.get(..value.len().saturating_sub(6)).filter(|_| {
                value.len() >= 6
                    && matches!(value.as_bytes()[value.len() - 6], b'+' | b'-')
                    && value.as_bytes()[value.len() - 3] == b':'
            })
        })
        .unwrap_or(value);
    let parts = core.split(':').collect::<Vec<_>>();
    (parts.len() == 2 || parts.len() == 3)
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn split_anchor_time(input: &str) -> Result<(&str, Option<&str>, Option<&str>), AppError> {
    let (date_part, rest) = if let Some((date, time)) = input.split_once('T') {
        (date, Some(time))
    } else if let Some((date, time)) = input.split_once(' ') {
        (date, Some(time))
    } else {
        (input, None)
    };
    let Some(rest) = rest else {
        return Ok((date_part, None, None));
    };
    if let Some(time) = rest.strip_suffix('Z') {
        return Ok((date_part, Some(time), Some("Z")));
    }
    if rest.len() >= 6 {
        let offset_index = rest.len() - 6;
        let offset = &rest[offset_index..];
        if matches!(offset.as_bytes()[0], b'+' | b'-') && offset.as_bytes()[3] == b':' {
            return Ok((date_part, Some(&rest[..offset_index]), Some(offset)));
        }
    }
    Ok((date_part, Some(rest), None))
}

fn invalid_anchor_time(input: &str) -> AppError {
    AppError::new(format!(
        "Invalid weekly cycle anchor time: {input}. Expected YYYY-MM-DD, YYYY-MM-DD HH:mm, or an ISO time with offset."
    ))
}

fn parse_date_part(value: &str, label: &str, input: &str) -> Result<i64, AppError> {
    value.parse::<i64>().map_err(|_| {
        AppError::new(format!(
            "Invalid {label} in weekly cycle anchor time: {input}."
        ))
    })
}

fn build_local_date(parts: AnchorDateTimeParts, input: &str) -> Result<DateTime<Utc>, AppError> {
    let naive = chrono::NaiveDate::from_ymd_opt(parts.year, parts.month, parts.day)
        .and_then(|date| date.and_hms_opt(parts.hour, parts.minute, parts.second))
        .ok_or_else(|| {
            AppError::new(format!("Invalid local weekly cycle anchor time: {input}."))
        })?;
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => Ok(value.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => Ok(earliest.with_timezone(&Utc)),
        LocalResult::None => Err(AppError::new(format!(
            "Invalid local weekly cycle anchor time: {input}."
        ))),
    }
}

fn build_offset_date(
    parts: AnchorDateTimeParts,
    offset: &str,
    input: &str,
) -> Result<DateTime<Utc>, AppError> {
    let offset_minutes = parse_offset_minutes(offset)?;
    let offset = FixedOffset::east_opt(offset_minutes * 60)
        .ok_or_else(|| AppError::new(format!("Invalid timezone offset: {offset}.")))?;
    offset
        .with_ymd_and_hms(
            parts.year,
            parts.month,
            parts.day,
            parts.hour,
            parts.minute,
            parts.second,
        )
        .single()
        .map(|date| date.with_timezone(&Utc))
        .ok_or_else(|| AppError::new(format!("Invalid offset weekly cycle anchor time: {input}.")))
}

fn parse_offset_minutes(offset: &str) -> Result<i32, AppError> {
    if offset == "Z" {
        return Ok(0);
    }
    if offset.len() != 6 || offset.as_bytes()[3] != b':' {
        return Err(AppError::new(format!("Invalid timezone offset: {offset}.")));
    }
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let hour = offset[1..3]
        .parse::<i32>()
        .map_err(|_| AppError::new(format!("Invalid timezone offset: {offset}.")))?;
    let minute = offset[4..6]
        .parse::<i32>()
        .map_err(|_| AppError::new(format!("Invalid timezone offset: {offset}.")))?;
    if hour > 23 || minute > 59 {
        return Err(AppError::new(format!("Invalid timezone offset: {offset}.")));
    }
    Ok(sign * (hour * 60 + minute))
}

fn format_offset_time_zone(offset: &str) -> String {
    if offset == "Z" {
        "UTC".to_string()
    } else {
        format!("UTC{offset}")
    }
}

fn local_time_zone() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            let offset = Local::now().offset().fix().local_minus_utc();
            if offset == 8 * 60 * 60 {
                "Asia/Shanghai".to_string()
            } else {
                "local".to_string()
            }
        })
}
