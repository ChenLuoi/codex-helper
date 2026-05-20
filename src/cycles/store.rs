use super::accounts::{resolve_weekly_cycle_account, WeeklyCycleAccountSource};
use super::cli::{resolve_cycle_file, CycleCommandOptions};
use super::time::{
    assert_iso_timestamp, iso_string, parse_weekly_cycle_anchor_time, weekly_cycle_anchor_id,
};
use super::{normalize_required_id, WEEKLY_CYCLE_PERIOD_HOURS, WEEKLY_CYCLE_STORE_VERSION};
use crate::error::AppError;
use crate::storage::{path_to_string, write_sensitive_file};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyCycleAnchor {
    pub id: String,
    pub at: String,
    pub input: String,
    pub time_zone: String,
    pub source: String,
    #[serde(default)]
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleAccountEntry {
    weekly: WeeklyCycleWeeklyEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleWeeklyEntry {
    period_hours: i64,
    anchors: Vec<WeeklyCycleAnchor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WeeklyCycleStore {
    version: u8,
    accounts: BTreeMap<String, WeeklyCycleAccountEntry>,
}

pub(super) struct AnchorMutationReport {
    pub(super) cycle_file: String,
    pub(super) account_id: String,
    pub(super) anchor: WeeklyCycleAnchor,
    pub(super) anchors: Vec<WeeklyCycleAnchor>,
}

pub(super) struct AnchorListReport {
    pub(super) cycle_file: String,
    pub(super) account_id: String,
    pub(super) account_source: WeeklyCycleAccountSource,
    pub(super) anchors: Vec<WeeklyCycleAnchor>,
}

pub(super) fn add_weekly_cycle_anchors_to_file(
    options: &CycleCommandOptions,
    times: &[String],
    now: DateTime<Utc>,
) -> Result<AnchorMutationReport, AppError> {
    if times.is_empty() {
        return Err(AppError::new(
            "At least one weekly cycle anchor time is required.",
        ));
    }

    let cycle_file = resolve_cycle_file(options);
    let account = resolve_weekly_cycle_account(options, now)?;
    let mut store = read_weekly_cycle_store(&cycle_file)?;
    let mut anchors = Vec::new();
    for at in times {
        let anchor = add_weekly_cycle_anchor(
            &mut store,
            &account.account_id,
            at,
            options.note.as_deref(),
            now,
        )?;
        anchors.push(anchor);
    }
    write_weekly_cycle_store(&cycle_file, &store)?;

    Ok(AnchorMutationReport {
        cycle_file: path_to_string(&cycle_file),
        account_id: account.account_id,
        anchor: anchors
            .first()
            .cloned()
            .ok_or_else(|| AppError::new("No weekly cycle anchor was added."))?,
        anchors,
    })
}

pub(super) fn list_weekly_cycle_anchors_from_file(
    options: &CycleCommandOptions,
    now: DateTime<Utc>,
) -> Result<AnchorListReport, AppError> {
    let cycle_file = resolve_cycle_file(options);
    let account = resolve_weekly_cycle_account(options, now)?;
    let store = read_weekly_cycle_store(&cycle_file)?;
    Ok(AnchorListReport {
        cycle_file: path_to_string(&cycle_file),
        account_id: account.account_id.clone(),
        account_source: account.source,
        anchors: list_weekly_cycle_anchors(&store, &account.account_id),
    })
}

pub(super) fn remove_weekly_cycle_anchor_from_file(
    anchor_id: &str,
    options: &CycleCommandOptions,
    now: DateTime<Utc>,
) -> Result<AnchorMutationReport, AppError> {
    let cycle_file = resolve_cycle_file(options);
    let account = resolve_weekly_cycle_account(options, now)?;
    let mut store = read_weekly_cycle_store(&cycle_file)?;
    let removed = remove_weekly_cycle_anchor(&mut store, &account.account_id, anchor_id)?;
    write_weekly_cycle_store(&cycle_file, &store)?;

    Ok(AnchorMutationReport {
        cycle_file: path_to_string(&cycle_file),
        account_id: account.account_id,
        anchor: removed,
        anchors: Vec::new(),
    })
}

fn create_empty_weekly_cycle_store() -> WeeklyCycleStore {
    WeeklyCycleStore {
        version: WEEKLY_CYCLE_STORE_VERSION,
        accounts: BTreeMap::new(),
    }
}

fn add_weekly_cycle_anchor(
    store: &mut WeeklyCycleStore,
    account_id: &str,
    at: &str,
    note: Option<&str>,
    now: DateTime<Utc>,
) -> Result<WeeklyCycleAnchor, AppError> {
    let account_id = normalize_required_id(account_id, "account id")?;
    let parsed = parse_weekly_cycle_anchor_time(at)?;
    normalize_weekly_cycle_store(store)?;
    let entry = store
        .accounts
        .entry(account_id.clone())
        .or_insert_with(create_weekly_cycle_account_entry);

    if entry
        .weekly
        .anchors
        .iter()
        .any(|anchor| anchor.at == parsed.at_iso)
    {
        return Err(AppError::new(format!(
            "Weekly cycle anchor already exists for account {account_id} at {}.",
            parsed.at_iso
        )));
    }

    let anchor = WeeklyCycleAnchor {
        id: weekly_cycle_anchor_id(parsed.at),
        at: parsed.at_iso,
        input: parsed.input,
        time_zone: parsed.time_zone,
        source: "manual".to_string(),
        note: note.unwrap_or("").to_string(),
        created_at: iso_string(now),
    };
    entry.weekly.anchors.push(anchor.clone());
    sort_weekly_cycle_anchors(&mut entry.weekly.anchors);
    Ok(anchor)
}

fn list_weekly_cycle_anchors(store: &WeeklyCycleStore, account_id: &str) -> Vec<WeeklyCycleAnchor> {
    let mut anchors = store
        .accounts
        .get(account_id)
        .map(|entry| entry.weekly.anchors.clone())
        .unwrap_or_default();
    sort_weekly_cycle_anchors(&mut anchors);
    anchors
}

fn remove_weekly_cycle_anchor(
    store: &mut WeeklyCycleStore,
    account_id: &str,
    anchor_id: &str,
) -> Result<WeeklyCycleAnchor, AppError> {
    let account_id = normalize_required_id(account_id, "account id")?;
    let anchor_id = normalize_required_id(anchor_id, "anchor id")?;
    normalize_weekly_cycle_store(store)?;
    let entry = store
        .accounts
        .entry(account_id.clone())
        .or_insert_with(create_weekly_cycle_account_entry);
    let index = entry
        .weekly
        .anchors
        .iter()
        .position(|anchor| anchor.id == anchor_id)
        .ok_or_else(|| {
            AppError::new(format!(
                "No weekly cycle anchor found for account {account_id}: {anchor_id}."
            ))
        })?;

    Ok(entry.weekly.anchors.remove(index))
}

fn read_weekly_cycle_store(cycle_file: &Path) -> Result<WeeklyCycleStore, AppError> {
    let content = match fs::read_to_string(cycle_file) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(create_empty_weekly_cycle_store());
        }
        Err(error) => return Err(AppError::new(error.to_string())),
    };
    let mut store: WeeklyCycleStore = serde_json::from_str(&content).map_err(|error| {
        AppError::new(format!(
            "Failed to parse {}: {}",
            path_to_string(cycle_file),
            error
        ))
    })?;
    normalize_weekly_cycle_store(&mut store)?;
    Ok(store)
}

fn write_weekly_cycle_store(cycle_file: &Path, store: &WeeklyCycleStore) -> Result<(), AppError> {
    let content =
        serde_json::to_string_pretty(store).map_err(|error| AppError::new(error.to_string()))?;
    write_sensitive_file(cycle_file, &format!("{content}\n"))
        .map_err(|error| AppError::new(error.to_string()))
}

fn normalize_weekly_cycle_store(store: &mut WeeklyCycleStore) -> Result<(), AppError> {
    if store.version != WEEKLY_CYCLE_STORE_VERSION {
        return Err(AppError::new(format!(
            "Unsupported weekly cycle store version: {}.",
            store.version
        )));
    }

    for (account_id, entry) in &mut store.accounts {
        if entry.weekly.period_hours != WEEKLY_CYCLE_PERIOD_HOURS {
            return Err(AppError::new(format!(
                "Expected weekly periodHours for account {account_id} to be {WEEKLY_CYCLE_PERIOD_HOURS}."
            )));
        }
        for anchor in &entry.weekly.anchors {
            if anchor.source != "manual" {
                return Err(AppError::new(
                    "Expected weekly cycle anchor source to be manual.",
                ));
            }
            assert_iso_timestamp(&anchor.at, "anchor.at")?;
            assert_iso_timestamp(&anchor.created_at, "anchor.createdAt")?;
        }
        sort_weekly_cycle_anchors(&mut entry.weekly.anchors);
    }
    Ok(())
}

fn create_weekly_cycle_account_entry() -> WeeklyCycleAccountEntry {
    WeeklyCycleAccountEntry {
        weekly: WeeklyCycleWeeklyEntry {
            period_hours: WEEKLY_CYCLE_PERIOD_HOURS,
            anchors: Vec::new(),
        },
    }
}

fn sort_weekly_cycle_anchors(anchors: &mut [WeeklyCycleAnchor]) {
    anchors.sort_by(|left, right| left.at.cmp(&right.at).then_with(|| left.id.cmp(&right.id)));
}
