use crate::error::AppError;
use crate::storage::write_sensitive_file;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

pub const ACCOUNT_HISTORY_STORE_VERSION: u8 = 1;
pub const DEFAULT_ACCOUNT_SOURCE: &str = "auth.json";
pub const AUTH_SELECT_SOURCE: &str = "auth select";

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountHistoryAccount {
    pub account_id: String,
    pub observed_at: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
}

impl AccountHistoryAccount {
    pub fn auth_json(
        account_id: String,
        observed_at: DateTime<Utc>,
        name: Option<String>,
        email: Option<String>,
        plan_type: Option<String>,
    ) -> Self {
        Self {
            account_id,
            observed_at: format_account_history_iso(observed_at),
            source: DEFAULT_ACCOUNT_SOURCE.to_string(),
            name,
            email,
            plan_type,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountHistorySwitchEvent {
    pub timestamp: String,
    pub from_account_id: String,
    pub to_account_id: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountHistoryStore {
    pub version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_account: Option<AccountHistoryAccount>,
    pub switches: Vec<AccountHistorySwitchEvent>,
}

#[derive(Debug, Clone)]
pub struct UsageAccountHistory {
    default_account_id: Option<String>,
    switches: Vec<UsageAccountSwitch>,
}

impl UsageAccountHistory {
    pub fn account_id_at(&self, timestamp: DateTime<Utc>) -> Option<String> {
        let mut account_id = self.default_account_id.clone();
        for entry in &self.switches {
            if entry.timestamp > timestamp {
                break;
            }
            account_id = Some(entry.to_account_id.clone());
        }
        account_id
    }
}

#[derive(Debug, Clone)]
struct UsageAccountSwitch {
    timestamp: DateTime<Utc>,
    to_account_id: String,
}

pub fn read_account_history_store(path: &Path) -> Result<AccountHistoryStore, AppError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(empty_account_history_store());
        }
        Err(error) => return Err(AppError::new(error.to_string())),
    };

    parse_account_history_store(&content, path)
}

pub fn write_account_history_store(
    path: &Path,
    store: &AccountHistoryStore,
) -> Result<(), AppError> {
    let normalized = normalize_account_history_store(store.clone())?;
    let content = serde_json::to_string_pretty(&normalized)
        .map_err(|error| AppError::new(error.to_string()))?;
    write_sensitive_file(path, &format!("{content}\n"))
        .map_err(|error| AppError::new(error.to_string()))
}

pub fn ensure_default_account(
    store: AccountHistoryStore,
    account: AccountHistoryAccount,
) -> Result<AccountHistoryStore, AppError> {
    let mut normalized = normalize_account_history_store(store)?;
    if normalized.default_account.is_none() {
        normalized.default_account = Some(account);
    }
    normalize_account_history_store(normalized)
}

pub fn ensure_default_account_in_file(
    path: &Path,
    account: AccountHistoryAccount,
) -> Result<AccountHistoryStore, AppError> {
    let current = read_account_history_store(path)?;
    let ensured = ensure_default_account(current, account)?;
    write_account_history_store(path, &ensured)?;
    Ok(ensured)
}

pub fn record_auth_select_switch(
    store: AccountHistoryStore,
    default_account: AccountHistoryAccount,
    from_account_id: &str,
    to_account_id: &str,
    timestamp: DateTime<Utc>,
) -> Result<AccountHistoryStore, AppError> {
    let mut ensured = ensure_default_account(store, default_account)?;
    ensured.switches.push(AccountHistorySwitchEvent {
        timestamp: format_account_history_iso(timestamp),
        from_account_id: from_account_id.to_string(),
        to_account_id: to_account_id.to_string(),
        source: AUTH_SELECT_SOURCE.to_string(),
    });
    normalize_account_history_store(ensured)
}

pub fn read_optional_usage_account_history(
    path: &Path,
) -> Result<Option<UsageAccountHistory>, AppError> {
    if !path.exists() {
        return Ok(None);
    }

    usage_account_history_from_store(read_account_history_store(path)?)
}

pub fn usage_account_history_from_store(
    store: AccountHistoryStore,
) -> Result<Option<UsageAccountHistory>, AppError> {
    let store = normalize_account_history_store(store)?;
    if store.default_account.is_none() && store.switches.is_empty() {
        return Ok(None);
    }

    let default_account_id = store.default_account.map(|account| account.account_id);
    let switches = store
        .switches
        .into_iter()
        .map(|entry| {
            Ok(UsageAccountSwitch {
                timestamp: parse_timestamp(&entry.timestamp, "switch.timestamp")?,
                to_account_id: entry.to_account_id,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(Some(UsageAccountHistory {
        default_account_id,
        switches,
    }))
}

pub fn read_default_account_label_for(
    path: &Path,
    account_id: &str,
) -> Result<Option<String>, AppError> {
    let store = read_account_history_store(path)?;
    let Some(account) = store.default_account else {
        return Ok(None);
    };
    if account.account_id != account_id {
        return Ok(None);
    }
    let label = account.email.or(account.name);
    Ok(label.map(|label| format!("{label}({account_id})")))
}

pub fn format_account_history_iso(date: DateTime<Utc>) -> String {
    date.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn empty_account_history_store() -> AccountHistoryStore {
    AccountHistoryStore {
        version: ACCOUNT_HISTORY_STORE_VERSION,
        default_account: None,
        switches: Vec::new(),
    }
}

fn parse_account_history_store(
    content: &str,
    path: &Path,
) -> Result<AccountHistoryStore, AppError> {
    if content.trim().is_empty() {
        return Ok(empty_account_history_store());
    }

    let parsed: AccountHistoryStore = serde_json::from_str(content).map_err(|error| {
        AppError::new(format!(
            "Failed to parse {}: {}",
            path_to_string(path),
            error
        ))
    })?;

    if parsed.version != ACCOUNT_HISTORY_STORE_VERSION {
        return Err(AppError::new(format!(
            "Unsupported auth account history version in {}: {}.",
            path_to_string(path),
            parsed.version
        )));
    }

    normalize_account_history_store(parsed)
}

fn normalize_account_history_store(
    mut store: AccountHistoryStore,
) -> Result<AccountHistoryStore, AppError> {
    if let Some(default_account) = &mut store.default_account {
        default_account.account_id =
            normalize_required_account_id(&default_account.account_id, "default account id")?;
        if default_account.source != DEFAULT_ACCOUNT_SOURCE {
            return Err(AppError::new(
                "Expected defaultAccount.source to be auth.json.",
            ));
        }
        parse_timestamp(&default_account.observed_at, "defaultAccount.observedAt")?;
        default_account.name = normalize_optional_string(default_account.name.take());
        default_account.email = normalize_optional_string(default_account.email.take());
        default_account.plan_type = normalize_optional_string(default_account.plan_type.take());
    }

    for entry in &mut store.switches {
        parse_timestamp(&entry.timestamp, "switch.timestamp")?;
        entry.from_account_id =
            normalize_required_account_id(&entry.from_account_id, "switch from account id")?;
        entry.to_account_id =
            normalize_required_account_id(&entry.to_account_id, "switch to account id")?;
        if entry.source != AUTH_SELECT_SOURCE {
            return Err(AppError::new("Expected switch.source to be auth select."));
        }
    }

    store.switches.sort_by(|left, right| {
        parse_timestamp(&left.timestamp, "switch.timestamp")
            .expect("switch timestamp validated")
            .cmp(
                &parse_timestamp(&right.timestamp, "switch.timestamp")
                    .expect("switch timestamp validated"),
            )
            .then_with(|| left.to_account_id.cmp(&right.to_account_id))
            .then_with(|| left.from_account_id.cmp(&right.from_account_id))
    });

    Ok(AccountHistoryStore {
        version: ACCOUNT_HISTORY_STORE_VERSION,
        default_account: store.default_account,
        switches: store.switches,
    })
}

fn parse_timestamp(value: &str, path: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| AppError::new(format!("Expected {path} to be a valid date string.")))
}

fn normalize_required_account_id(value: &str, label: &str) -> Result<String, AppError> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(AppError::new(format!("{label} cannot be empty.")));
    }
    Ok(normalized.to_string())
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn missing_and_empty_history_files_read_as_empty_store() {
        let root = temp_dir("codex-ops-account-history-empty");
        fs::create_dir_all(&root).expect("create root");
        let missing = root.join("missing.json");
        let empty = root.join("empty.json");
        fs::write(&empty, "  \n").expect("write empty");

        assert_eq!(
            read_account_history_store(&missing).expect("missing store"),
            empty_account_history_store()
        );
        assert_eq!(
            read_account_history_store(&empty).expect("empty store"),
            empty_account_history_store()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let root = temp_dir("codex-ops-account-history-version");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join("history.json");
        fs::write(&path, r#"{"version":2,"switches":[]}"#).expect("write history");

        let error = read_account_history_store(&path).expect_err("unsupported version");

        assert!(error
            .message()
            .contains("Unsupported auth account history version"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn switches_are_sorted_for_usage_attribution() {
        let store = normalize_account_history_store(AccountHistoryStore {
            version: ACCOUNT_HISTORY_STORE_VERSION,
            default_account: Some(account("account-a", "2026-05-01T00:00:00.000Z")),
            switches: vec![
                switch("2026-05-03T00:00:00.000Z", "account-b", "account-c"),
                switch("2026-05-02T00:00:00.000Z", "account-a", "account-b"),
            ],
        })
        .expect("normalize");
        let usage = usage_account_history_from_store(store)
            .expect("usage history")
            .expect("non-empty usage history");

        assert_eq!(
            usage.account_id_at(parse("2026-05-01T12:00:00.000Z")),
            Some("account-a".to_string())
        );
        assert_eq!(
            usage.account_id_at(parse("2026-05-02T12:00:00.000Z")),
            Some("account-b".to_string())
        );
        assert_eq!(
            usage.account_id_at(parse("2026-05-03T12:00:00.000Z")),
            Some("account-c".to_string())
        );
    }

    #[test]
    fn ensure_default_account_initializes_and_writes_store() {
        let root = temp_dir("codex-ops-account-history-ensure");
        fs::create_dir_all(&root).expect("create root");
        let path = root.join("history.json");

        let store =
            ensure_default_account_in_file(&path, account("account-a", "2026-05-01T00:00:00.000Z"))
                .expect("ensure default");

        assert_eq!(
            store.default_account.expect("default").account_id,
            "account-a"
        );
        let content = fs::read_to_string(&path).expect("read written history");
        assert!(content.contains(r#""defaultAccount""#));
        assert!(content.ends_with('\n'));

        let _ = fs::remove_dir_all(root);
    }

    fn account(account_id: &str, observed_at: &str) -> AccountHistoryAccount {
        AccountHistoryAccount {
            account_id: account_id.to_string(),
            observed_at: observed_at.to_string(),
            source: DEFAULT_ACCOUNT_SOURCE.to_string(),
            name: None,
            email: None,
            plan_type: None,
        }
    }

    fn switch(timestamp: &str, from: &str, to: &str) -> AccountHistorySwitchEvent {
        AccountHistorySwitchEvent {
            timestamp: timestamp.to_string(),
            from_account_id: from.to_string(),
            to_account_id: to.to_string(),
            source: AUTH_SELECT_SOURCE.to_string(),
        }
    }

    fn parse(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("timestamp")
            .with_timezone(&Utc)
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_millis();
        std::env::temp_dir().join(format!("{prefix}-{millis}-{}", std::process::id()))
    }
}
