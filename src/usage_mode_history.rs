use crate::error::AppError;
use crate::storage::{path_to_string, write_sensitive_file};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

pub const USAGE_MODE_HISTORY_STORE_VERSION: u8 = 1;
pub const FAST_ON_SOURCE: &str = "fast on";
pub const FAST_OFF_SOURCE: &str = "fast off";
pub const MANUAL_FILE_EDIT_SOURCE: &str = "manual file edit";
const LEGACY_MODE_FAST_ON_SOURCE: &str = "mode fast on";
const LEGACY_MODE_FAST_OFF_SOURCE: &str = "mode fast off";

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageModeDefault {
    pub fast: bool,
    pub observed_at: String,
    pub source: String,
}

impl UsageModeDefault {
    pub fn manual(fast: bool, observed_at: DateTime<Utc>) -> Self {
        Self {
            fast,
            observed_at: format_usage_mode_history_iso(observed_at),
            source: MANUAL_FILE_EDIT_SOURCE.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageModeSwitchEvent {
    pub timestamp: String,
    pub fast: bool,
    pub source: String,
}

impl UsageModeSwitchEvent {
    pub fn fast_command(fast: bool, timestamp: DateTime<Utc>) -> Self {
        Self {
            timestamp: format_usage_mode_history_iso(timestamp),
            fast,
            source: source_for_fast_command(fast).to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageModeHistoryStore {
    pub version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_mode: Option<UsageModeDefault>,
    pub switches: Vec<UsageModeSwitchEvent>,
}

#[derive(Debug, Clone)]
pub struct UsageModeHistory {
    default_fast: Option<bool>,
    switches: Vec<UsageModeSwitch>,
}

impl UsageModeHistory {
    pub fn fast_at(&self, timestamp: DateTime<Utc>) -> Option<bool> {
        let mut fast = self.default_fast;
        for entry in &self.switches {
            if entry.timestamp > timestamp {
                break;
            }
            fast = Some(entry.fast);
        }
        fast
    }
}

#[derive(Debug, Clone)]
struct UsageModeSwitch {
    timestamp: DateTime<Utc>,
    fast: bool,
}

pub fn read_usage_mode_history_store(path: &Path) -> Result<UsageModeHistoryStore, AppError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(empty_usage_mode_history_store());
        }
        Err(error) => return Err(AppError::new(error.to_string())),
    };

    parse_usage_mode_history_store(&content, path)
}

pub fn write_usage_mode_history_store(
    path: &Path,
    store: &UsageModeHistoryStore,
) -> Result<(), AppError> {
    let normalized = normalize_usage_mode_history_store(store.clone())?;
    let content = serde_json::to_string_pretty(&normalized)
        .map_err(|error| AppError::new(error.to_string()))?;
    write_sensitive_file(path, &format!("{content}\n"))
        .map_err(|error| AppError::new(error.to_string()))
}

pub fn record_usage_mode_switch(
    store: UsageModeHistoryStore,
    fast: bool,
    timestamp: DateTime<Utc>,
) -> Result<UsageModeHistoryStore, AppError> {
    let mut normalized = normalize_usage_mode_history_store(store)?;
    normalized
        .switches
        .push(UsageModeSwitchEvent::fast_command(fast, timestamp));
    normalize_usage_mode_history_store(normalized)
}

pub fn read_optional_usage_mode_history(path: &Path) -> Result<Option<UsageModeHistory>, AppError> {
    if !path.exists() {
        return Ok(None);
    }

    usage_mode_history_from_store(read_usage_mode_history_store(path)?)
}

pub fn usage_mode_history_from_store(
    store: UsageModeHistoryStore,
) -> Result<Option<UsageModeHistory>, AppError> {
    let store = normalize_usage_mode_history_store(store)?;
    if store.default_mode.is_none() && store.switches.is_empty() {
        return Ok(None);
    }

    let default_fast = store.default_mode.map(|mode| mode.fast);
    let switches = store
        .switches
        .into_iter()
        .map(|entry| {
            Ok(UsageModeSwitch {
                timestamp: parse_timestamp(&entry.timestamp, "switch.timestamp")?,
                fast: entry.fast,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(Some(UsageModeHistory {
        default_fast,
        switches,
    }))
}

pub fn format_usage_mode_history_iso(date: DateTime<Utc>) -> String {
    date.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn empty_usage_mode_history_store() -> UsageModeHistoryStore {
    UsageModeHistoryStore {
        version: USAGE_MODE_HISTORY_STORE_VERSION,
        default_mode: None,
        switches: Vec::new(),
    }
}

fn parse_usage_mode_history_store(
    content: &str,
    path: &Path,
) -> Result<UsageModeHistoryStore, AppError> {
    if content.trim().is_empty() {
        return Ok(empty_usage_mode_history_store());
    }

    let parsed: UsageModeHistoryStore = serde_json::from_str(content).map_err(|error| {
        AppError::new(format!(
            "Failed to parse {}: {}",
            path_to_string(path),
            error
        ))
    })?;

    if parsed.version != USAGE_MODE_HISTORY_STORE_VERSION {
        return Err(AppError::new(format!(
            "Unsupported usage mode history version in {}: {}.",
            path_to_string(path),
            parsed.version
        )));
    }

    normalize_usage_mode_history_store(parsed)
}

fn normalize_usage_mode_history_store(
    mut store: UsageModeHistoryStore,
) -> Result<UsageModeHistoryStore, AppError> {
    if store.version != USAGE_MODE_HISTORY_STORE_VERSION {
        return Err(AppError::new(format!(
            "Unsupported usage mode history version: {}.",
            store.version
        )));
    }

    if let Some(default_mode) = &mut store.default_mode {
        parse_timestamp(&default_mode.observed_at, "defaultMode.observedAt")?;
        validate_source(&default_mode.source, default_mode.fast, "defaultMode")?;
    }

    for entry in &mut store.switches {
        parse_timestamp(&entry.timestamp, "switch.timestamp")?;
        validate_source(&entry.source, entry.fast, "switch")?;
    }

    store.switches.sort_by(|left, right| {
        parse_timestamp(&left.timestamp, "switch.timestamp")
            .expect("switch timestamp validated")
            .cmp(
                &parse_timestamp(&right.timestamp, "switch.timestamp")
                    .expect("switch timestamp validated"),
            )
    });

    Ok(UsageModeHistoryStore {
        version: USAGE_MODE_HISTORY_STORE_VERSION,
        default_mode: store.default_mode,
        switches: store.switches,
    })
}

fn parse_timestamp(value: &str, path: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| AppError::new(format!("Expected {path} to be a valid date string.")))
}

fn validate_source(source: &str, fast: bool, field: &str) -> Result<(), AppError> {
    match source {
        FAST_ON_SOURCE | LEGACY_MODE_FAST_ON_SOURCE if fast => Ok(()),
        FAST_OFF_SOURCE | LEGACY_MODE_FAST_OFF_SOURCE if !fast => Ok(()),
        MANUAL_FILE_EDIT_SOURCE => Ok(()),
        FAST_ON_SOURCE | FAST_OFF_SOURCE | LEGACY_MODE_FAST_ON_SOURCE
        | LEGACY_MODE_FAST_OFF_SOURCE => Err(AppError::new(format!(
            "Expected {field}.source to match its fast value."
        ))),
        _ => Err(AppError::new(format!(
            "Expected {field}.source to be fast on, fast off, mode fast on, mode fast off, or manual file edit."
        ))),
    }
}

fn source_for_fast_command(fast: bool) -> &'static str {
    if fast {
        FAST_ON_SOURCE
    } else {
        FAST_OFF_SOURCE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn missing_and_empty_history_files_read_as_empty_store() {
        let root = temp_dir("codex-ops-usage-mode-history-empty");
        fs::create_dir_all(&root).expect("create root");
        let missing = root.join("missing.json");
        let empty = root.join("empty.json");
        fs::write(&empty, "  \n").expect("write empty");

        assert_eq!(
            read_usage_mode_history_store(&missing).expect("missing store"),
            empty_usage_mode_history_store()
        );
        assert_eq!(
            read_usage_mode_history_store(&empty).expect("empty store"),
            empty_usage_mode_history_store()
        );
        assert!(read_optional_usage_mode_history(&missing)
            .expect("optional missing")
            .is_none());
        assert!(read_optional_usage_mode_history(&empty)
            .expect("optional empty")
            .is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let root = temp_dir("codex-ops-usage-mode-history-version");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join("history.json");
        fs::write(&path, r#"{"version":2,"switches":[]}"#).expect("write history");

        let error = read_usage_mode_history_store(&path).expect_err("unsupported version");

        assert!(error
            .message()
            .contains("Unsupported usage mode history version"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_timestamp_is_rejected() {
        let error = usage_mode_history_from_store(UsageModeHistoryStore {
            version: USAGE_MODE_HISTORY_STORE_VERSION,
            default_mode: None,
            switches: vec![UsageModeSwitchEvent {
                timestamp: "not-a-date".to_string(),
                fast: true,
                source: FAST_ON_SOURCE.to_string(),
            }],
        })
        .expect_err("invalid timestamp");

        assert!(error
            .message()
            .contains("Expected switch.timestamp to be a valid date string"));
    }

    #[test]
    fn invalid_source_is_rejected() {
        let error = usage_mode_history_from_store(UsageModeHistoryStore {
            version: USAGE_MODE_HISTORY_STORE_VERSION,
            default_mode: Some(UsageModeDefault {
                fast: false,
                observed_at: "2026-05-01T00:00:00.000Z".to_string(),
                source: "rollout".to_string(),
            }),
            switches: Vec::new(),
        })
        .expect_err("invalid source");

        assert!(error.message().contains("Expected defaultMode.source"));
    }

    #[test]
    fn source_must_match_fast_value_for_fast_command_entries() {
        let error = usage_mode_history_from_store(UsageModeHistoryStore {
            version: USAGE_MODE_HISTORY_STORE_VERSION,
            default_mode: None,
            switches: vec![UsageModeSwitchEvent {
                timestamp: "2026-05-01T00:00:00.000Z".to_string(),
                fast: false,
                source: FAST_ON_SOURCE.to_string(),
            }],
        })
        .expect_err("mismatched source");

        assert!(error.message().contains("source to match its fast value"));
    }

    #[test]
    fn switches_are_sorted_for_fast_attribution() {
        let store = normalize_usage_mode_history_store(UsageModeHistoryStore {
            version: USAGE_MODE_HISTORY_STORE_VERSION,
            default_mode: Some(default(false, "2026-05-01T00:00:00.000Z")),
            switches: vec![
                switch("2026-05-03T00:00:00.000Z", false),
                switch("2026-05-02T00:00:00.000Z", true),
            ],
        })
        .expect("normalize");
        let usage = usage_mode_history_from_store(store)
            .expect("usage history")
            .expect("non-empty usage history");

        assert_eq!(
            usage.fast_at(parse("2026-05-01T12:00:00.000Z")),
            Some(false)
        );
        assert_eq!(usage.fast_at(parse("2026-05-02T12:00:00.000Z")), Some(true));
        assert_eq!(
            usage.fast_at(parse("2026-05-03T12:00:00.000Z")),
            Some(false)
        );
    }

    #[test]
    fn duplicate_switch_timestamps_keep_input_order_for_last_writer_wins() {
        let store = normalize_usage_mode_history_store(UsageModeHistoryStore {
            version: USAGE_MODE_HISTORY_STORE_VERSION,
            default_mode: Some(default(false, "2026-05-01T00:00:00.000Z")),
            switches: vec![
                switch("2026-05-02T00:00:00.000Z", false),
                switch("2026-05-02T00:00:00.000Z", true),
            ],
        })
        .expect("normalize");
        let usage = usage_mode_history_from_store(store)
            .expect("usage history")
            .expect("non-empty usage history");

        assert_eq!(usage.fast_at(parse("2026-05-02T00:00:00.000Z")), Some(true));
    }

    #[test]
    fn missing_default_mode_returns_none_before_first_switch() {
        let usage = usage_mode_history_from_store(UsageModeHistoryStore {
            version: USAGE_MODE_HISTORY_STORE_VERSION,
            default_mode: None,
            switches: vec![switch("2026-05-02T00:00:00.000Z", true)],
        })
        .expect("usage history")
        .expect("non-empty usage history");

        assert_eq!(usage.fast_at(parse("2026-05-01T12:00:00.000Z")), None);
        assert_eq!(usage.fast_at(parse("2026-05-02T00:00:00.000Z")), Some(true));
    }

    #[test]
    fn record_usage_mode_switch_appends_normalized_command_source() {
        let store = record_usage_mode_switch(
            empty_usage_mode_history_store(),
            true,
            parse("2026-05-02T00:00:00.000Z"),
        )
        .expect("record switch");

        assert_eq!(
            store.switches,
            vec![switch("2026-05-02T00:00:00.000Z", true)]
        );
    }

    #[test]
    fn write_store_uses_camel_case_json_and_trailing_newline() {
        let root = temp_dir("codex-ops-usage-mode-history-write");
        let path = root.join("codex-ops").join("usage-mode-history.json");
        let store = UsageModeHistoryStore {
            version: USAGE_MODE_HISTORY_STORE_VERSION,
            default_mode: Some(default(false, "2026-05-01T00:00:00.000Z")),
            switches: vec![switch("2026-05-02T00:00:00.000Z", true)],
        };

        write_usage_mode_history_store(&path, &store).expect("write history");
        let content = fs::read_to_string(&path).expect("read written history");

        assert!(content.contains(r#""defaultMode""#));
        assert!(content.contains(r#""observedAt""#));
        assert!(content.ends_with('\n'));

        let _ = fs::remove_dir_all(root);
    }

    fn default(fast: bool, observed_at: &str) -> UsageModeDefault {
        UsageModeDefault {
            fast,
            observed_at: observed_at.to_string(),
            source: MANUAL_FILE_EDIT_SOURCE.to_string(),
        }
    }

    fn switch(timestamp: &str, fast: bool) -> UsageModeSwitchEvent {
        UsageModeSwitchEvent {
            timestamp: timestamp.to_string(),
            fast,
            source: source_for_fast_command(fast).to_string(),
        }
    }

    fn parse(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("timestamp")
            .with_timezone(&Utc)
    }
}
