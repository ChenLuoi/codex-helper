use crate::account_history::{
    self, AccountHistoryAccount, AccountHistoryStore, UsageAccountHistory,
};
use crate::error::AppError;
use crate::format::to_pretty_json;
use crate::storage::{
    path_to_string, percent_encode, resolve_storage_paths, write_sensitive_file, StorageOptions,
};
use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const OPENAI_AUTH_CLAIM: &str = "https://api.openai.com/auth";

type JwtJsonObject = Map<String, Value>;

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct AuthCommandOptions {
    pub auth_file: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub store_dir: Option<PathBuf>,
    pub account_history_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthOrganization {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatusSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub audience: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_before: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_auth_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_expired: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds_until_expiry: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
    pub auth_methods: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chatgpt_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chatgpt_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_active_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_active_until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_last_checked: Option<String>,
    pub organizations: Vec<AuthOrganization>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthStatusReport {
    pub auth_file: String,
    pub token_name: String,
    pub header: Value,
    pub claims: Value,
    pub summary: AuthStatusSummary,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthProfileEntry {
    pub source: AuthProfileSource,
    pub account_id: String,
    pub profile_file: Option<String>,
    pub auth_file: Option<String>,
    pub summary: AuthStatusSummary,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AuthProfileSource {
    Current,
    Stored,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthProfileListReport {
    pub auth_file: String,
    pub store_dir: String,
    pub current: Option<AuthProfileEntry>,
    pub stored: Vec<AuthProfileEntry>,
    pub skipped_stored: Vec<AuthProfileReadError>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthProfileReadError {
    pub profile_file: String,
    pub reason: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthProfileSaveReport {
    pub auth_file: String,
    pub store_dir: String,
    pub profile: AuthProfileEntry,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthProfileSwitchReport {
    pub auth_file: String,
    pub store_dir: String,
    pub account_history_file: String,
    pub saved_current: AuthProfileEntry,
    pub activated: AuthProfileEntry,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthProfileRemoveReport {
    pub store_dir: String,
    pub removed: AuthProfileEntry,
}

struct ParsedAuthFile {
    content: String,
    report: AuthStatusReport,
    account_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthStatusJson<'a> {
    auth_file: &'a str,
    token_name: &'a str,
    token_claims_included: bool,
    summary: &'a AuthStatusSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    header: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    claims: Option<&'a Value>,
}

pub fn read_codex_auth_status(
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<AuthStatusReport, AppError> {
    let auth_file = auth_file_path(options);
    let parsed = read_codex_auth_file(&auth_file, now)?;
    Ok(parsed.report)
}

pub fn ensure_usage_account_history(
    account_history_file: &Path,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<UsageAccountHistory, AppError> {
    let mut store = account_history::read_account_history_store(account_history_file)?;
    if store.default_account.is_none() {
        let report = read_codex_auth_status(options, now)?;
        let account_id = report
            .summary
            .chatgpt_account_id
            .clone()
            .or(report.summary.token_account_id.clone())
            .ok_or_else(|| AppError::new("No account id found in auth.json."))?;
        store = account_history::ensure_default_account_in_file(
            account_history_file,
            AccountHistoryAccount::auth_json(
                account_id,
                now,
                report.summary.name.clone(),
                report.summary.email.clone(),
                report.summary.plan_type.clone(),
            ),
        )?;
    }
    account_history::usage_account_history_from_store(store)?
        .ok_or_else(|| AppError::new("No account history default account found."))
}

pub fn save_current_codex_auth_profile(
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<AuthProfileSaveReport, AppError> {
    let auth_file = auth_file_path(options);
    let store_dir = profile_store_dir(options);
    let current = read_codex_auth_file(&auth_file, now)?;
    let profile_file = resolve_profile_file(&store_dir, &current.account_id);

    write_sensitive_file(&profile_file, &current.content)
        .map_err(|error| AppError::new(error.to_string()))?;

    Ok(AuthProfileSaveReport {
        auth_file: path_to_string(&auth_file),
        store_dir: path_to_string(&store_dir),
        profile: to_auth_profile_entry(
            current,
            AuthProfileSource::Stored,
            Some(profile_file),
            None,
        ),
    })
}

pub fn list_codex_auth_profiles(
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<AuthProfileListReport, AppError> {
    let auth_file = auth_file_path(options);
    let store_dir = profile_store_dir(options);
    let current = match read_codex_auth_file(&auth_file, now) {
        Ok(parsed) => Some(to_auth_profile_entry(
            parsed,
            AuthProfileSource::Current,
            None,
            Some(auth_file.clone()),
        )),
        Err(error) if is_not_found_error(error.message()) => None,
        Err(error) => return Err(error),
    };
    let stored = read_stored_codex_auth_profiles(&store_dir, now)?;

    Ok(AuthProfileListReport {
        auth_file: path_to_string(&auth_file),
        store_dir: path_to_string(&store_dir),
        current,
        stored: stored.0,
        skipped_stored: stored.1,
    })
}

pub fn switch_codex_auth_profile(
    account_id: &str,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<AuthProfileSwitchReport, AppError> {
    let auth_file = auth_file_path(options);
    let store_dir = profile_store_dir(options);
    let account_history_file = account_history_file_path(options);
    let profile_file = resolve_profile_file(&store_dir, account_id);
    let selected = read_codex_auth_file(&profile_file, now)?;
    let current = read_codex_auth_file(&auth_file, now)?;

    if selected.account_id != account_id {
        return Err(AppError::new(format!(
            "Stored auth profile {} contains account id {}, expected {account_id}.",
            path_to_string(&profile_file),
            selected.account_id
        )));
    }

    let saved_profile_file = resolve_profile_file(&store_dir, &current.account_id);
    let saved_current = to_auth_profile_entry(
        current,
        AuthProfileSource::Stored,
        Some(saved_profile_file.clone()),
        None,
    );
    let activated = to_auth_profile_entry(
        selected,
        AuthProfileSource::Current,
        None,
        Some(auth_file.clone()),
    );
    let previous_history_content = read_optional_file_content(&account_history_file)?;
    let next_history_store = build_codex_auth_profile_switch_history(
        &account_history_file,
        &saved_current,
        &activated,
        now,
    )?;

    write_sensitive_file(&saved_profile_file, &read_file_content(&auth_file)?)
        .map_err(|error| AppError::new(error.to_string()))?;
    account_history::write_account_history_store(&account_history_file, &next_history_store)?;

    let selected_content = read_file_content(&profile_file)?;
    if let Err(error) = write_sensitive_file(&auth_file, &selected_content) {
        let _ = restore_auth_account_history_file(&account_history_file, previous_history_content);
        return Err(AppError::new(error.to_string()));
    }

    Ok(AuthProfileSwitchReport {
        auth_file: path_to_string(&auth_file),
        store_dir: path_to_string(&store_dir),
        account_history_file: path_to_string(&account_history_file),
        saved_current,
        activated,
    })
}

pub fn remove_codex_auth_profile(
    account_id: &str,
    options: &AuthCommandOptions,
    now: DateTime<Utc>,
) -> Result<AuthProfileRemoveReport, AppError> {
    let store_dir = profile_store_dir(options);
    let profile_file = resolve_profile_file(&store_dir, account_id);
    let selected = read_codex_auth_file(&profile_file, now)?;

    if selected.account_id != account_id {
        return Err(AppError::new(format!(
            "Stored auth profile {} contains account id {}, expected {account_id}.",
            path_to_string(&profile_file),
            selected.account_id
        )));
    }

    fs::remove_file(&profile_file).map_err(|error| AppError::new(error.to_string()))?;

    Ok(AuthProfileRemoveReport {
        store_dir: path_to_string(&store_dir),
        removed: to_auth_profile_entry(
            selected,
            AuthProfileSource::Stored,
            Some(profile_file),
            None,
        ),
    })
}

pub fn format_auth_status(
    report: &AuthStatusReport,
    json: bool,
    include_token_claims: bool,
) -> Result<String, AppError> {
    if json {
        let value = AuthStatusJson {
            auth_file: &report.auth_file,
            token_name: &report.token_name,
            token_claims_included: include_token_claims,
            summary: &report.summary,
            header: include_token_claims.then_some(&report.header),
            claims: include_token_claims.then_some(&report.claims),
        };
        return Ok(format!(
            "{}\n",
            to_pretty_json(&value).map_err(|error| AppError::new(error.to_string()))?
        ));
    }

    let mut lines = vec!["Codex auth".to_string()];
    append_optional_line(
        &mut lines,
        "Account ID",
        report
            .summary
            .chatgpt_account_id
            .as_deref()
            .or(report.summary.token_account_id.as_deref()),
    );
    append_optional_line(&mut lines, "Key ID", report.summary.key_id.as_deref());
    append_optional_line(&mut lines, "Name", report.summary.name.as_deref());
    append_optional_line(&mut lines, "Email", report.summary.email.as_deref());
    append_optional_line(
        &mut lines,
        "User ID",
        report
            .summary
            .user_id
            .as_deref()
            .or(report.summary.chatgpt_user_id.as_deref()),
    );
    append_optional_line(&mut lines, "Plan", report.summary.plan_type.as_deref());

    if !report.summary.organizations.is_empty() {
        lines.push("Organizations:".to_string());
        for organization in &report.summary.organizations {
            lines.push(format!("  {}", format_organization(organization)));
        }
    }

    Ok(format!("{}\n", lines.join("\n")))
}

pub fn format_auth_profile_list(report: &AuthProfileListReport) -> String {
    let mut lines = vec![
        "Codex auth profiles".to_string(),
        format!("Store: {}", report.store_dir),
        String::new(),
    ];

    match &report.current {
        Some(current) => lines.push(format!("Current: {}", format_auth_profile_entry(current))),
        None => lines.push("Current: (missing auth.json)".to_string()),
    }

    lines.push(String::new());

    if report.stored.is_empty() {
        lines.push("Persisted: none".to_string());
    } else {
        lines.push("Persisted:".to_string());
        for (index, entry) in report.stored.iter().enumerate() {
            let marker = if Some(&entry.account_id)
                == report.current.as_ref().map(|entry| &entry.account_id)
            {
                " (current)"
            } else {
                ""
            };
            lines.push(format!(
                "  {}. {}{}",
                index + 1,
                format_auth_profile_entry(entry),
                marker
            ));
        }
    }

    if !report.skipped_stored.is_empty() {
        lines.push(String::new());
        lines.push("Skipped persisted profiles:".to_string());
        for (index, entry) in report.skipped_stored.iter().enumerate() {
            lines.push(format!(
                "  {}. {} - {}",
                index + 1,
                entry.profile_file,
                entry.reason
            ));
        }
    }

    format!("{}\n", lines.join("\n"))
}

pub fn format_auth_profile_entry(entry: &AuthProfileEntry) -> String {
    let label = entry
        .summary
        .email
        .as_deref()
        .or(entry.summary.name.as_deref())
        .or(entry.summary.user_id.as_deref())
        .or(entry.summary.chatgpt_user_id.as_deref())
        .unwrap_or("unknown");
    let plan = entry.summary.plan_type.as_deref().unwrap_or("unknown");
    format!("{label}({}) - {plan}", entry.account_id)
}

fn read_codex_auth_file(file_path: &Path, now: DateTime<Utc>) -> Result<ParsedAuthFile, AppError> {
    let content = fs::read_to_string(file_path).map_err(|error| file_error(error, file_path))?;
    let auth_json = parse_json_object(&content, file_path)?;
    let report = build_codex_auth_status(&auth_json, &path_to_string(file_path), now)?;
    let account_id = get_auth_account_id(&report)?;

    Ok(ParsedAuthFile {
        content,
        report,
        account_id,
    })
}

fn build_codex_auth_status(
    auth_json: &Map<String, Value>,
    auth_file: &str,
    now: DateTime<Utc>,
) -> Result<AuthStatusReport, AppError> {
    let tokens = auth_json
        .get("tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::new("No id_token found in auth.json. Expected auth.json tokens.id_token.")
        })?;
    let id_token = tokens
        .get("id_token")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::new("No id_token found in auth.json. Expected auth.json tokens.id_token.")
        })?;

    if id_token.is_empty() {
        return Err(AppError::new(
            "No id_token found in auth.json. Expected auth.json tokens.id_token.",
        ));
    }

    let (header, claims) = decode_jwt(id_token, "id_token")?;
    let summary = summarize_auth_jwt(auth_json, &header, &claims, now);

    Ok(AuthStatusReport {
        auth_file: auth_file.to_string(),
        token_name: "id_token".to_string(),
        header: Value::Object(header),
        claims: Value::Object(claims),
        summary,
    })
}

fn decode_jwt(token: &str, token_name: &str) -> Result<(JwtJsonObject, JwtJsonObject), AppError> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return Err(AppError::new(format!(
            "{token_name} is not a JWT with header, payload, and signature parts."
        )));
    }

    let header = decode_jwt_json_part(parts[0], token_name, "header")?;
    let claims = decode_jwt_json_part(parts[1], token_name, "payload")?;
    Ok((header, claims))
}

fn decode_jwt_json_part(
    segment: &str,
    token_name: &str,
    part_name: &str,
) -> Result<Map<String, Value>, AppError> {
    let decoded = base64url_decode(segment).map_err(|_| {
        AppError::new(format!(
            "{token_name} {part_name} is not valid base64url JSON."
        ))
    })?;
    let value: Value = serde_json::from_slice(&decoded).map_err(|_| {
        AppError::new(format!(
            "{token_name} {part_name} is not valid base64url JSON."
        ))
    })?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::new(format!("{token_name} {part_name} must be a JSON object.")))
}

fn summarize_auth_jwt(
    auth_json: &Map<String, Value>,
    header: &Map<String, Value>,
    claims: &Map<String, Value>,
    now: DateTime<Utc>,
) -> AuthStatusSummary {
    let expires_at = read_numeric_date_claim(claims, "exp");
    let openai_auth = claims.get(OPENAI_AUTH_CLAIM).and_then(Value::as_object);
    let tokens = auth_json.get("tokens").and_then(Value::as_object);
    let seconds_until_expiry = expires_at
        .as_ref()
        .map(|expires_at| expires_at.signed_duration_since(now).num_seconds());

    AuthStatusSummary {
        auth_mode: get_string_value(auth_json.get("auth_mode")),
        token_account_id: tokens.and_then(|tokens| get_string_value(tokens.get("account_id"))),
        last_refresh: read_date_value(auth_json.get("last_refresh")),
        token_type: get_string_claim(header, "typ"),
        algorithm: get_string_claim(header, "alg"),
        key_id: get_string_claim(header, "kid"),
        issuer: get_string_claim(claims, "iss"),
        subject: get_string_claim(claims, "sub"),
        audience: get_string_array_claim(claims, "aud"),
        jwt_id: get_string_claim(claims, "jti"),
        issued_at: read_numeric_date_claim(claims, "iat")
            .map(account_history::format_account_history_iso),
        expires_at: expires_at.map(account_history::format_account_history_iso),
        not_before: read_numeric_date_claim(claims, "nbf")
            .map(account_history::format_account_history_iso),
        auth_time: read_numeric_date_claim(claims, "auth_time")
            .map(account_history::format_account_history_iso),
        requested_auth_time: read_numeric_date_claim(claims, "rat")
            .map(account_history::format_account_history_iso),
        is_expired: expires_at.map(|expires_at| expires_at <= now),
        seconds_until_expiry,
        name: get_string_claim(claims, "name"),
        email: get_string_claim(claims, "email"),
        email_verified: get_boolean_claim(claims, "email_verified"),
        auth_provider: get_string_claim(claims, "auth_provider"),
        auth_methods: get_string_array_claim(claims, "amr"),
        chatgpt_account_id: openai_auth
            .and_then(|object| get_string_claim(object, "chatgpt_account_id")),
        chatgpt_user_id: openai_auth.and_then(|object| get_string_claim(object, "chatgpt_user_id")),
        user_id: openai_auth.and_then(|object| get_string_claim(object, "user_id")),
        plan_type: openai_auth.and_then(|object| get_string_claim(object, "chatgpt_plan_type")),
        subscription_active_start: openai_auth
            .and_then(|object| read_date_value(object.get("chatgpt_subscription_active_start"))),
        subscription_active_until: openai_auth
            .and_then(|object| read_date_value(object.get("chatgpt_subscription_active_until"))),
        subscription_last_checked: openai_auth
            .and_then(|object| read_date_value(object.get("chatgpt_subscription_last_checked"))),
        organizations: get_organizations(openai_auth),
        scopes: get_scope_claims(claims),
    }
}

fn read_stored_codex_auth_profiles(
    store_dir: &Path,
    now: DateTime<Utc>,
) -> Result<(Vec<AuthProfileEntry>, Vec<AuthProfileReadError>), AppError> {
    let entries = match fs::read_dir(store_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()))
        }
        Err(error) => return Err(AppError::new(error.to_string())),
    };

    let mut filenames = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| AppError::new(error.to_string()))?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".json"))
        {
            filenames.push(path);
        }
    }
    filenames.sort();

    let mut profiles = Vec::new();
    let mut skipped = Vec::new();
    for profile_file in filenames {
        match read_codex_auth_file(&profile_file, now) {
            Ok(parsed) => profiles.push(to_auth_profile_entry(
                parsed,
                AuthProfileSource::Stored,
                Some(profile_file),
                None,
            )),
            Err(error) => skipped.push(AuthProfileReadError {
                profile_file: path_to_string(&profile_file),
                reason: error.message().to_string(),
            }),
        }
    }

    profiles.sort_by(|left, right| left.account_id.cmp(&right.account_id));
    Ok((profiles, skipped))
}

fn build_codex_auth_profile_switch_history(
    account_history_file: &Path,
    saved_current: &AuthProfileEntry,
    activated: &AuthProfileEntry,
    now: DateTime<Utc>,
) -> Result<AccountHistoryStore, AppError> {
    let current_store = account_history::read_account_history_store(account_history_file)?;
    account_history::record_auth_select_switch(
        current_store,
        auth_profile_entry_to_history_account(saved_current, now),
        &saved_current.account_id,
        &activated.account_id,
        now,
    )
}

fn auth_profile_entry_to_history_account(
    entry: &AuthProfileEntry,
    now: DateTime<Utc>,
) -> AccountHistoryAccount {
    AccountHistoryAccount::auth_json(
        entry.account_id.clone(),
        now,
        entry.summary.name.clone(),
        entry.summary.email.clone(),
        entry.summary.plan_type.clone(),
    )
}

fn to_auth_profile_entry(
    parsed: ParsedAuthFile,
    source: AuthProfileSource,
    profile_file: Option<PathBuf>,
    auth_file: Option<PathBuf>,
) -> AuthProfileEntry {
    AuthProfileEntry {
        source,
        account_id: parsed.account_id,
        profile_file: profile_file.as_ref().map(path_to_string),
        auth_file: auth_file.as_ref().map(path_to_string),
        summary: parsed.report.summary,
    }
}

fn get_auth_account_id(report: &AuthStatusReport) -> Result<String, AppError> {
    let account_id = report
        .summary
        .chatgpt_account_id
        .as_deref()
        .or(report.summary.token_account_id.as_deref())
        .unwrap_or_default();

    if account_id.is_empty() {
        return Err(AppError::new("No account id found in auth.json."));
    }

    Ok(account_id.to_string())
}

fn auth_file_path(options: &AuthCommandOptions) -> PathBuf {
    resolve_storage_paths(&storage_options(options)).auth_file
}

fn profile_store_dir(options: &AuthCommandOptions) -> PathBuf {
    resolve_storage_paths(&storage_options(options)).profile_store_dir
}

fn account_history_file_path(options: &AuthCommandOptions) -> PathBuf {
    resolve_storage_paths(&storage_options(options)).account_history_file
}

fn storage_options(options: &AuthCommandOptions) -> StorageOptions {
    StorageOptions {
        codex_home: options.codex_home.clone(),
        auth_file: options.auth_file.clone(),
        profile_store_dir: options.store_dir.clone(),
        account_history_file: options.account_history_file.clone(),
        usage_mode_history_file: None,
        sessions_dir: None,
    }
}

fn resolve_profile_file(store_dir: &Path, account_id: &str) -> PathBuf {
    store_dir.join(format!("{}.json", percent_encode(account_id)))
}

fn read_file_content(path: &Path) -> Result<String, AppError> {
    fs::read_to_string(path).map_err(|error| file_error(error, path))
}

fn read_optional_file_content(path: &Path) -> Result<Option<String>, AppError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::new(error.to_string())),
    }
}

fn restore_auth_account_history_file(path: &Path, content: Option<String>) -> Result<(), AppError> {
    match content {
        Some(content) => {
            write_sensitive_file(path, &content).map_err(|error| AppError::new(error.to_string()))
        }
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::new(error.to_string())),
        },
    }
}

fn parse_json_object(content: &str, file_path: &Path) -> Result<Map<String, Value>, AppError> {
    let value: Value = serde_json::from_str(content).map_err(|error| {
        AppError::new(format!(
            "Failed to parse {}: {}",
            path_to_string(file_path),
            error
        ))
    })?;

    value.as_object().cloned().ok_or_else(|| {
        AppError::new(format!(
            "Expected {} to contain a JSON object.",
            path_to_string(file_path)
        ))
    })
}

fn get_organizations(openai_auth: Option<&Map<String, Value>>) -> Vec<AuthOrganization> {
    let Some(Value::Array(organizations)) =
        openai_auth.and_then(|object| object.get("organizations"))
    else {
        return Vec::new();
    };

    organizations
        .iter()
        .filter_map(Value::as_object)
        .map(|organization| AuthOrganization {
            id: get_string_claim(organization, "id"),
            title: get_string_claim(organization, "title"),
            role: get_string_claim(organization, "role"),
            is_default: get_boolean_claim(organization, "is_default"),
        })
        .collect()
}

fn get_string_claim(object: &Map<String, Value>, key: &str) -> Option<String> {
    get_string_value(object.get(key))
}

fn get_string_value(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Number(value)) => Some(value.to_string()),
        Some(Value::Bool(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn get_boolean_claim(object: &Map<String, Value>, key: &str) -> Option<bool> {
    match object.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("true") => Some(true),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn get_string_array_claim(object: &Map<String, Value>, key: &str) -> Vec<String> {
    match object.get(key) {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Number(value)) => vec![value.to_string()],
        Some(Value::Bool(value)) => vec![value.to_string()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| get_string_value(Some(value)))
            .collect(),
        _ => Vec::new(),
    }
}

fn get_scope_claims(object: &Map<String, Value>) -> Vec<String> {
    let mut scopes = BTreeSet::new();
    for value in get_space_separated_claim(object, "scope")
        .into_iter()
        .chain(get_space_separated_claim(object, "scp"))
        .chain(get_string_array_claim(object, "scopes"))
    {
        scopes.insert(value);
    }
    scopes.into_iter().collect()
}

fn get_space_separated_claim(object: &Map<String, Value>, key: &str) -> Vec<String> {
    match object.get(key) {
        Some(Value::String(value)) => value
            .split_whitespace()
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect(),
        _ => get_string_array_claim(object, key),
    }
}

fn read_numeric_date_claim(object: &Map<String, Value>, key: &str) -> Option<DateTime<Utc>> {
    let timestamp = match object.get(key) {
        Some(Value::Number(value)) => value.as_f64()?,
        Some(Value::String(value)) => value.parse::<f64>().ok()?,
        _ => return None,
    };

    if !timestamp.is_finite() {
        return None;
    }

    Utc.timestamp_millis_opt((timestamp * 1000.0) as i64)
        .single()
}

fn read_date_value(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    if text.is_empty() {
        return None;
    }

    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|date| account_history::format_account_history_iso(date.with_timezone(&Utc)))
}

fn append_optional_line(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        if !value.is_empty() {
            lines.push(format!("{label}: {value}"));
        }
    }
}

fn format_organization(organization: &AuthOrganization) -> String {
    let mut parts = Vec::new();
    if let Some(title) = organization
        .title
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(title.to_string());
    }
    if let Some(id) = organization.id.as_deref().filter(|value| !value.is_empty()) {
        parts.push(id.to_string());
    }
    if let Some(role) = organization
        .role
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("role={role}"));
    }
    if organization.is_default == Some(true) {
        parts.push("default".to_string());
    }

    if parts.is_empty() {
        "(unknown organization)".to_string()
    } else {
        parts.join(", ")
    }
}

fn base64url_decode(value: &str) -> Result<Vec<u8>, ()> {
    let mut output = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u8;

    for byte in value.bytes() {
        if byte == b'=' {
            break;
        }

        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(()),
        } as u32;

        buffer = (buffer << 6) | value;
        bits += 6;

        while bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }

    Ok(output)
}

fn file_error(error: io::Error, path: &Path) -> AppError {
    if error.kind() == io::ErrorKind::NotFound {
        return AppError::new(format!(
            "ENOENT: no such file or directory, open '{}'",
            path_to_string(path)
        ));
    }
    AppError::new(error.to_string())
}

fn is_not_found_error(message: &str) -> bool {
    message.starts_with("ENOENT:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn decodes_status_without_leaking_the_token() {
        let token = jwt(
            r#"{"alg":"RS256","typ":"JWT","kid":"key-1"}"#,
            r#"{"sub":"user_123","exp":1778649000,"email":"user@example.test","https://api.openai.com/auth":{"chatgpt_account_id":"account_123","chatgpt_plan_type":"pro"}}"#,
        );
        let mut auth_json = Map::new();
        auth_json.insert(
            "tokens".to_string(),
            serde_json::json!({ "id_token": token, "account_id": "account_123" }),
        );
        let report = build_codex_auth_status(
            &auth_json,
            "/tmp/auth.json",
            DateTime::parse_from_rfc3339("2026-05-12T00:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();

        assert_eq!(
            report.summary.chatgpt_account_id.as_deref(),
            Some("account_123")
        );
        let text = format_auth_status(&report, false, false).unwrap();
        assert!(text.contains("Account ID: account_123"));
        assert!(!text.contains("id_token"));
    }

    #[test]
    fn json_claims_are_opt_in() {
        let token = jwt(r#"{"alg":"RS256","kid":"key-1"}"#, r#"{"sub":"user_123"}"#);
        let mut auth_json = Map::new();
        auth_json.insert(
            "tokens".to_string(),
            serde_json::json!({ "id_token": token }),
        );
        let report = build_codex_auth_status(&auth_json, "/tmp/auth.json", Utc::now()).unwrap();
        let default_json: Value =
            serde_json::from_str(&format_auth_status(&report, true, false).unwrap()).unwrap();
        let claims_json: Value =
            serde_json::from_str(&format_auth_status(&report, true, true).unwrap()).unwrap();

        assert!(default_json.get("claims").is_none());
        assert_eq!(claims_json["claims"]["sub"], "user_123");
    }

    #[test]
    fn malformed_jwt_errors_are_clear() {
        let mut auth_json = Map::new();
        auth_json.insert(
            "tokens".to_string(),
            serde_json::json!({ "id_token": "not-a-jwt" }),
        );

        let error = build_codex_auth_status(&auth_json, "/tmp/auth.json", Utc::now()).unwrap_err();

        assert!(error.message().contains("id_token is not a JWT"));
    }

    #[test]
    fn profile_switch_preserves_files_and_writes_history() {
        let temp_dir = temp_dir("codex-ops-auth-switch");
        let auth_file = temp_dir.join("auth.json");
        let store_dir = temp_dir.join("auth-profiles");
        let history_file = temp_dir.join("auth-account-history.json");
        let now = DateTime::parse_from_rfc3339("2026-05-13T00:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let current_content = auth_content("account-a", "a@example.test", "plus");
        let selected_content = auth_content("account-b", "b@example.test", "pro");

        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(&auth_file, &selected_content).unwrap();
        save_current_codex_auth_profile(
            &AuthCommandOptions {
                auth_file: Some(auth_file.clone()),
                store_dir: Some(store_dir.clone()),
                ..AuthCommandOptions::default()
            },
            now,
        )
        .unwrap();
        fs::write(&auth_file, &current_content).unwrap();

        let report = switch_codex_auth_profile(
            "account-b",
            &AuthCommandOptions {
                auth_file: Some(auth_file.clone()),
                store_dir: Some(store_dir.clone()),
                account_history_file: Some(history_file.clone()),
                ..AuthCommandOptions::default()
            },
            now,
        )
        .unwrap();

        assert_eq!(report.saved_current.account_id, "account-a");
        assert_eq!(report.activated.account_id, "account-b");
        assert_eq!(fs::read_to_string(&auth_file).unwrap(), selected_content);
        assert_eq!(
            fs::read_to_string(store_dir.join("account-a.json")).unwrap(),
            current_content
        );
        let history: Value =
            serde_json::from_str(&fs::read_to_string(&history_file).unwrap()).unwrap();
        assert_eq!(history["defaultAccount"]["accountId"], "account-a");
        assert_eq!(history["switches"][0]["fromAccountId"], "account-a");
        assert_eq!(history["switches"][0]["toAccountId"], "account-b");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
