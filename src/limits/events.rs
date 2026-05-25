use super::reports::{RateLimitParseDiagnostics, RateLimitSample, SourceSpan};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default)]
pub struct RateLimitLineContext<'a> {
    pub session_id: &'a str,
    pub account_id: Option<&'a str>,
    pub source: Option<SourceSpan>,
}

pub fn parse_rate_limit_line(
    line: &str,
    context: RateLimitLineContext<'_>,
    diagnostics: &mut RateLimitParseDiagnostics,
) -> Vec<RateLimitSample> {
    let value = match serde_json::from_str::<Value>(line) {
        Ok(value) => value,
        Err(_) => {
            diagnostics.invalid_json_lines += 1;
            return Vec::new();
        }
    };

    let Some(event) = value.as_object() else {
        return Vec::new();
    };
    if string_field(event, "type").as_deref() != Some("event_msg") {
        return Vec::new();
    }

    let Some(payload) = object_field(event, "payload") else {
        diagnostics.missing_rate_limits += 1;
        return Vec::new();
    };
    let Some(rate_limits_value) = payload.get("rate_limits") else {
        diagnostics.missing_rate_limits += 1;
        return Vec::new();
    };

    diagnostics.rate_limit_events += 1;
    if rate_limits_value.is_null() {
        diagnostics.null_rate_limits += 1;
        return Vec::new();
    }

    let Some(rate_limits) = rate_limits_value.as_object() else {
        diagnostics.missing_windows += 1;
        return Vec::new();
    };
    let Some(timestamp) = event.get("timestamp").and_then(value_to_utc) else {
        diagnostics.missing_timestamps += 1;
        return Vec::new();
    };

    let plan_type = string_field(rate_limits, "plan_type");
    let limit_id = string_field(rate_limits, "limit_id");
    let mut samples = Vec::new();

    for (window_name, window_value) in rate_limit_window_entries(rate_limits) {
        if !matches!(window_name, "primary" | "secondary") {
            diagnostics.unknown_windows += 1;
        }
        let Some(window) = window_value.as_object() else {
            diagnostics.invalid_window_minutes += 1;
            continue;
        };
        if let Some(sample) = parse_window_sample(
            timestamp,
            window_name,
            window,
            &context,
            plan_type.as_deref(),
            limit_id.as_deref(),
            diagnostics,
        ) {
            samples.push(sample);
        }
    }

    if samples.is_empty() {
        diagnostics.missing_windows += 1;
    }
    diagnostics.included_samples += samples.len() as i64;
    samples
}

fn rate_limit_window_entries(rate_limits: &Map<String, Value>) -> Vec<(&str, &Value)> {
    let mut entries = rate_limits
        .iter()
        .filter(|(key, value)| {
            !matches!(key.as_str(), "plan_type" | "limit_id")
                && (matches!(key.as_str(), "primary" | "secondary") || value.is_object())
        })
        .map(|(key, value)| (key.as_str(), value))
        .collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| {
        window_sort_key(left)
            .cmp(&window_sort_key(right))
            .then_with(|| left.cmp(right))
    });
    entries
}

fn window_sort_key(window_name: &str) -> u8 {
    match window_name {
        "primary" => 0,
        "secondary" => 1,
        _ => 2,
    }
}

fn parse_window_sample(
    timestamp: DateTime<Utc>,
    window_name: &str,
    window: &Map<String, Value>,
    context: &RateLimitLineContext<'_>,
    plan_type: Option<&str>,
    limit_id: Option<&str>,
    diagnostics: &mut RateLimitParseDiagnostics,
) -> Option<RateLimitSample> {
    let Some(window_minutes) = window.get("window_minutes").and_then(value_to_i64) else {
        diagnostics.invalid_window_minutes += 1;
        return None;
    };
    if window_minutes <= 0 {
        diagnostics.invalid_window_minutes += 1;
        return None;
    }
    let Some(used_percent) = window.get("used_percent").and_then(value_to_f64) else {
        diagnostics.invalid_used_percent += 1;
        return None;
    };
    let Some(resets_at) = window.get("resets_at").and_then(value_to_unix_seconds) else {
        diagnostics.invalid_resets_at += 1;
        return None;
    };

    if !(0.0..=100.0).contains(&used_percent) {
        diagnostics.out_of_range_percent += 1;
    }

    Some(RateLimitSample {
        timestamp,
        session_id: context.session_id.to_string(),
        account_id: context.account_id.map(str::to_string),
        plan_type: plan_type.map(str::to_string),
        limit_id: limit_id.map(str::to_string),
        window: window_label(window_name, window_minutes),
        window_minutes,
        used_percent,
        remaining_percent: 100.0 - used_percent,
        resets_at,
        source: context.source.clone(),
    })
}

fn object_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Map<String, Value>> {
    object.get(key).and_then(Value::as_object)
}

fn string_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn value_to_f64(value: &Value) -> Option<f64> {
    let parsed = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(value) => value.trim().parse::<f64>().ok(),
        _ => None,
    }?;

    parsed.is_finite().then_some(parsed)
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| {
                number.as_f64().and_then(|value| {
                    if value.is_finite()
                        && value.fract() == 0.0
                        && value >= i64::MIN as f64
                        && value <= i64::MAX as f64
                    {
                        Some(value as i64)
                    } else {
                        None
                    }
                })
            }),
        Value::String(value) => value.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn value_to_unix_seconds(value: &Value) -> Option<DateTime<Utc>> {
    let seconds = value_to_i64(value)?;
    Utc.timestamp_opt(seconds, 0).single()
}

fn value_to_utc(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::String(value) => DateTime::parse_from_rfc3339(value.trim())
            .ok()
            .map(|timestamp| timestamp.with_timezone(&Utc)),
        Value::Number(_) => {
            let millis = value_to_i64(value)?;
            Utc.timestamp_millis_opt(millis).single()
        }
        _ => None,
    }
}

fn window_label(window_name: &str, window_minutes: i64) -> String {
    match window_minutes {
        300 => "5h".to_string(),
        10080 => "7d".to_string(),
        _ => window_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_primary_and_secondary_rate_limit_samples() {
        let line = json!({
            "timestamp": "2026-05-10T09:00:01.500Z",
            "type": "event_msg",
            "payload": {
                "rate_limits": {
                    "primary": {
                        "window_minutes": 300,
                        "used_percent": 42.5,
                        "resets_at": 1778421600
                    },
                    "secondary": {
                        "window_minutes": 10080,
                        "used_percent": 84.0,
                        "resets_at": 1778490000
                    },
                    "plan_type": "pro",
                    "limit_id": "fixture-alpha-pre-reset"
                }
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(
            &line,
            RateLimitLineContext {
                session_id: "rust-run-session-alpha",
                account_id: Some("account-fixture"),
                source: Some(SourceSpan {
                    path: "/tmp/session.jsonl".to_string(),
                    line_number: 3,
                }),
            },
            &mut diagnostics,
        );

        assert_eq!(samples.len(), 2);
        assert_eq!(diagnostics.rate_limit_events, 1);
        assert_eq!(diagnostics.included_samples, 2);

        let primary = &samples[0];
        assert_eq!(primary.session_id, "rust-run-session-alpha");
        assert_eq!(primary.account_id.as_deref(), Some("account-fixture"));
        assert_eq!(primary.plan_type.as_deref(), Some("pro"));
        assert_eq!(primary.limit_id.as_deref(), Some("fixture-alpha-pre-reset"));
        assert_eq!(primary.window, "5h");
        assert_eq!(primary.window_minutes, 300);
        assert_eq!(primary.used_percent, 42.5);
        assert_eq!(primary.remaining_percent, 57.5);
        assert_eq!(
            primary.resets_at,
            Utc.timestamp_opt(1778421600, 0).single().unwrap()
        );
        assert_eq!(
            primary.source.as_ref().map(|source| source.path.as_str()),
            Some("/tmp/session.jsonl")
        );
        assert_eq!(
            primary.source.as_ref().map(|source| source.line_number),
            Some(3)
        );

        let secondary = &samples[1];
        assert_eq!(secondary.window, "7d");
        assert_eq!(secondary.window_minutes, 10080);
        assert_eq!(secondary.used_percent, 84.0);

        let serialized = serde_json::to_value(primary).expect("sample json");
        assert!(serialized.get("source").is_none());
        assert!(serialized.get("sourcePath").is_none());
        assert!(serialized.get("lineNumber").is_none());
        for key in [
            "timestamp",
            "sessionId",
            "accountId",
            "planType",
            "limitId",
            "window",
            "windowMinutes",
            "usedPercent",
            "remainingPercent",
            "resetsAt",
        ] {
            assert!(serialized.get(key).is_some(), "missing key {key}");
        }
    }

    #[test]
    fn parses_primary_only_sample_without_missing_window_diagnostic() {
        let line = json!({
            "timestamp": "2026-05-12T13:05:00.000Z",
            "type": "event_msg",
            "payload": {
                "rate_limits": {
                    "primary": {
                        "window_minutes": 300,
                        "used_percent": 18.0,
                        "resets_at": 1778605200
                    },
                    "plan_type": "plus"
                }
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(
            &line,
            RateLimitLineContext {
                session_id: "rust-run-session-delta",
                account_id: None,
                source: None,
            },
            &mut diagnostics,
        );

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].account_id, None);
        assert_eq!(samples[0].plan_type.as_deref(), Some("plus"));
        assert_eq!(diagnostics.missing_windows, 0);
    }

    #[test]
    fn parses_additional_window_objects_after_standard_windows() {
        let line = json!({
            "timestamp": "2026-05-12T13:05:00.000Z",
            "type": "event_msg",
            "payload": {
                "rate_limits": {
                    "burst": {
                        "window_minutes": 60,
                        "used_percent": 11.0,
                        "resets_at": 1778605200
                    },
                    "primary": {
                        "window_minutes": 300,
                        "used_percent": 18.0,
                        "resets_at": 1778605200
                    },
                    "secondary": {
                        "window_minutes": 10080,
                        "used_percent": 22.0,
                        "resets_at": 1779206400
                    },
                    "plan_type": "team",
                    "limit_id": "fixture-extra-window"
                }
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(&line, default_context(), &mut diagnostics);

        assert_eq!(samples.len(), 3);
        assert_eq!(
            samples
                .iter()
                .map(|sample| sample.window.as_str())
                .collect::<Vec<_>>(),
            vec!["5h", "7d", "burst"]
        );
        assert_eq!(samples[2].window_minutes, 60);
        assert_eq!(samples[2].limit_id.as_deref(), Some("fixture-extra-window"));
        assert_eq!(diagnostics.unknown_windows, 1);
        assert_eq!(diagnostics.missing_windows, 0);
    }

    #[test]
    fn null_rate_limits_are_counted_and_skipped() {
        let line = json!({
            "timestamp": "2026-05-12T12:10:00.000Z",
            "type": "event_msg",
            "payload": {
                "rate_limits": null
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(&line, default_context(), &mut diagnostics);

        assert!(samples.is_empty());
        assert_eq!(diagnostics.rate_limit_events, 1);
        assert_eq!(diagnostics.null_rate_limits, 1);
    }

    #[test]
    fn missing_rate_limits_are_counted_on_event_payloads() {
        let line = json!({
            "timestamp": "2026-05-12T12:10:00.000Z",
            "type": "event_msg",
            "payload": {}
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(&line, default_context(), &mut diagnostics);

        assert!(samples.is_empty());
        assert_eq!(diagnostics.missing_rate_limits, 1);
        assert_eq!(diagnostics.rate_limit_events, 0);
    }

    #[test]
    fn invalid_json_counts_but_non_object_json_and_non_event_lines_are_skipped() {
        let mut diagnostics = RateLimitParseDiagnostics::default();

        assert!(parse_rate_limit_line("{not-json", default_context(), &mut diagnostics).is_empty());
        assert!(parse_rate_limit_line("[]", default_context(), &mut diagnostics).is_empty());
        assert!(parse_rate_limit_line(
            &json!({"type": "session_meta"}).to_string(),
            default_context(),
            &mut diagnostics
        )
        .is_empty());

        assert_eq!(diagnostics.invalid_json_lines, 1);
        assert_eq!(diagnostics.rate_limit_events, 0);
    }

    #[test]
    fn invalid_window_fields_are_counted_without_samples() {
        let line = json!({
            "timestamp": "2026-05-12T12:10:00.000Z",
            "type": "event_msg",
            "payload": {
                "rate_limits": {
                    "primary": {
                        "used_percent": 10.0,
                        "resets_at": 1778605200
                    },
                    "secondary": {
                        "window_minutes": 10080,
                        "used_percent": "not-percent",
                        "resets_at": "not-reset"
                    },
                    "tertiary": {
                        "used_percent": 1.0,
                        "resets_at": 1778605200
                    }
                }
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(&line, default_context(), &mut diagnostics);

        assert!(samples.is_empty());
        assert_eq!(diagnostics.invalid_window_minutes, 2);
        assert_eq!(diagnostics.invalid_used_percent, 1);
        assert_eq!(diagnostics.invalid_resets_at, 0);
        assert_eq!(diagnostics.unknown_windows, 1);
        assert_eq!(diagnostics.missing_windows, 1);
    }

    #[test]
    fn zero_window_minutes_are_counted_without_samples() {
        let line = json!({
            "timestamp": "2026-05-12T12:10:00.000Z",
            "type": "event_msg",
            "payload": {
                "rate_limits": {
                    "primary": {
                        "window_minutes": 0,
                        "used_percent": 10.0,
                        "resets_at": 1778605200
                    }
                }
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(&line, default_context(), &mut diagnostics);

        assert!(samples.is_empty());
        assert_eq!(diagnostics.invalid_window_minutes, 1);
        assert_eq!(diagnostics.invalid_used_percent, 0);
        assert_eq!(diagnostics.invalid_resets_at, 0);
    }

    #[test]
    fn invalid_resets_at_is_counted_after_valid_percent() {
        let line = json!({
            "timestamp": "2026-05-12T12:10:00.000Z",
            "type": "event_msg",
            "payload": {
                "rate_limits": {
                    "primary": {
                        "window_minutes": 300,
                        "used_percent": 10.0,
                        "resets_at": "not-reset"
                    }
                }
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(&line, default_context(), &mut diagnostics);

        assert!(samples.is_empty());
        assert_eq!(diagnostics.invalid_resets_at, 1);
        assert_eq!(diagnostics.missing_windows, 1);
    }

    #[test]
    fn out_of_range_percent_is_preserved_and_flagged() {
        let line = json!({
            "timestamp": "2026-05-12T12:10:00.000Z",
            "type": "event_msg",
            "payload": {
                "rate_limits": {
                    "primary": {
                        "window_minutes": "300",
                        "used_percent": 125.0,
                        "resets_at": "1778605200"
                    }
                }
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(&line, default_context(), &mut diagnostics);

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].used_percent, 125.0);
        assert_eq!(samples[0].remaining_percent, -25.0);
        assert_eq!(diagnostics.out_of_range_percent, 1);
    }

    #[test]
    fn missing_or_invalid_timestamp_skips_samples() {
        let line = json!({
            "timestamp": "not-a-date",
            "type": "event_msg",
            "payload": {
                "rate_limits": {
                    "primary": {
                        "window_minutes": 300,
                        "used_percent": 10.0,
                        "resets_at": 1778605200
                    }
                }
            }
        })
        .to_string();
        let mut diagnostics = RateLimitParseDiagnostics::default();

        let samples = parse_rate_limit_line(&line, default_context(), &mut diagnostics);

        assert!(samples.is_empty());
        assert_eq!(diagnostics.rate_limit_events, 1);
        assert_eq!(diagnostics.missing_timestamps, 1);
    }

    fn default_context<'a>() -> RateLimitLineContext<'a> {
        RateLimitLineContext {
            session_id: "session-fixture",
            account_id: None,
            source: None,
        }
    }
}
