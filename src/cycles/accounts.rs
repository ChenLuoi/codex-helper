use super::cli::{auth_options, resolve_account_history_file, CycleCommandOptions};
use super::reports::WeeklyCycleReportContext;
use super::{normalize_optional_id, normalize_required_id, DEFAULT_WEEKLY_CYCLE_ACCOUNT_ID};
use crate::account_history;
use crate::auth::{
    list_codex_auth_profiles, read_codex_auth_status, AuthStatusReport, AuthStatusSummary,
};
use crate::error::AppError;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum WeeklyCycleAccountSource {
    Explicit,
    ChatgptAccountId,
    TokenAccountId,
    Default,
}

impl WeeklyCycleAccountSource {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::ChatgptAccountId => "chatgpt_account_id",
            Self::TokenAccountId => "token_account_id",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct WeeklyCycleAccountResolution {
    pub(super) account_id: String,
    pub(super) source: WeeklyCycleAccountSource,
}

pub(super) fn resolve_weekly_cycle_account(
    options: &CycleCommandOptions,
    now: DateTime<Utc>,
) -> Result<WeeklyCycleAccountResolution, AppError> {
    if let Some(account_id) = options.account_id.as_deref() {
        return Ok(WeeklyCycleAccountResolution {
            account_id: normalize_required_id(account_id, "--account-id")?,
            source: WeeklyCycleAccountSource::Explicit,
        });
    }

    if let Some(status) = read_optional_auth_status(options, now) {
        if let Some(account_id) =
            normalize_optional_id(status.summary.chatgpt_account_id.as_deref())
        {
            return Ok(WeeklyCycleAccountResolution {
                account_id,
                source: WeeklyCycleAccountSource::ChatgptAccountId,
            });
        }
        if let Some(account_id) = normalize_optional_id(status.summary.token_account_id.as_deref())
        {
            return Ok(WeeklyCycleAccountResolution {
                account_id,
                source: WeeklyCycleAccountSource::TokenAccountId,
            });
        }
    }

    Ok(WeeklyCycleAccountResolution {
        account_id: DEFAULT_WEEKLY_CYCLE_ACCOUNT_ID.to_string(),
        source: WeeklyCycleAccountSource::Default,
    })
}

pub(super) fn cycle_report_context(
    account_id: &str,
    source: WeeklyCycleAccountSource,
    cycle_file: &str,
    options: &CycleCommandOptions,
    now: DateTime<Utc>,
) -> WeeklyCycleReportContext {
    WeeklyCycleReportContext {
        account_id: Some(account_id.to_string()),
        account_label: resolve_cycle_account_label(account_id, options, now),
        account_source: Some(source.as_str()),
        cycle_file: Some(cycle_file.to_string()),
    }
}

pub(super) fn resolve_cycle_account_label(
    account_id: &str,
    options: &CycleCommandOptions,
    now: DateTime<Utc>,
) -> Option<String> {
    if let Some(status) = read_optional_auth_status(options, now) {
        let auth_account_id = status
            .summary
            .chatgpt_account_id
            .as_deref()
            .or(status.summary.token_account_id.as_deref());
        if auth_account_id == Some(account_id) {
            return format_cycle_account_label(account_id, &status.summary);
        }
    }

    if let Ok(profiles) = list_codex_auth_profiles(&auth_options(options), now) {
        if let Some(current) = profiles.current.as_ref() {
            if current.account_id == account_id {
                return format_cycle_account_label(account_id, &current.summary);
            }
        }
        for profile in &profiles.stored {
            if profile.account_id == account_id {
                return format_cycle_account_label(account_id, &profile.summary);
            }
        }
    }

    read_history_default_cycle_account_label(account_id, options)
}

pub(super) fn format_cycle_account_line(account_id: &str, account_label: Option<&str>) -> String {
    format!("Account: {}", account_label.unwrap_or(account_id))
}

fn read_optional_auth_status(
    options: &CycleCommandOptions,
    now: DateTime<Utc>,
) -> Option<AuthStatusReport> {
    read_codex_auth_status(&auth_options(options), now).ok()
}

fn read_history_default_cycle_account_label(
    account_id: &str,
    options: &CycleCommandOptions,
) -> Option<String> {
    let path = resolve_account_history_file(options);
    account_history::read_default_account_label_for(&path, account_id)
        .ok()
        .flatten()
}

fn format_cycle_account_label(account_id: &str, account: &AuthStatusSummary) -> Option<String> {
    let label = account.email.as_deref().or(account.name.as_deref())?;
    if label.is_empty() {
        None
    } else {
        Some(format!("{label}({account_id})"))
    }
}
