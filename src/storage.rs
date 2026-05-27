use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StoragePaths {
    pub codex_home: PathBuf,
    pub helper_dir: PathBuf,
    pub auth_file: PathBuf,
    pub profile_store_dir: PathBuf,
    pub account_history_file: PathBuf,
    pub sessions_dir: PathBuf,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct StorageOptions {
    pub codex_home: Option<PathBuf>,
    pub auth_file: Option<PathBuf>,
    pub profile_store_dir: Option<PathBuf>,
    pub account_history_file: Option<PathBuf>,
    pub sessions_dir: Option<PathBuf>,
}

pub fn default_codex_home() -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    default_codex_home_from(env::var_os("CODEX_HOME").map(PathBuf::from), home)
}

pub fn default_codex_home_from(codex_home_env: Option<PathBuf>, home_dir: PathBuf) -> PathBuf {
    codex_home_env.unwrap_or_else(|| home_dir.join(".codex"))
}

pub fn resolve_codex_home(options: &StorageOptions) -> PathBuf {
    options
        .codex_home
        .clone()
        .unwrap_or_else(default_codex_home)
}

pub fn resolve_codex_ops_dir(codex_home: impl AsRef<Path>) -> PathBuf {
    codex_home.as_ref().join("codex-ops")
}

pub fn resolve_storage_paths(options: &StorageOptions) -> StoragePaths {
    let codex_home = resolve_codex_home(options);
    let helper_dir = resolve_codex_ops_dir(&codex_home);

    StoragePaths {
        auth_file: options
            .auth_file
            .clone()
            .unwrap_or_else(|| codex_home.join("auth.json")),
        profile_store_dir: options
            .profile_store_dir
            .clone()
            .unwrap_or_else(|| helper_dir.join("auth-profiles")),
        account_history_file: options
            .account_history_file
            .clone()
            .unwrap_or_else(|| helper_dir.join("auth-account-history.json")),
        sessions_dir: options
            .sessions_dir
            .clone()
            .unwrap_or_else(|| codex_home.join("sessions")),
        codex_home,
        helper_dir,
    }
}

pub fn write_sensitive_file(file_path: impl AsRef<Path>, content: &str) -> io::Result<()> {
    let file_path = file_path.as_ref();
    let parent = file_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    set_permissions_best_effort(parent, 0o700);

    let temp_file = parent.join(format!(
        ".{}.{}.tmp",
        percent_encode(&temp_seed()),
        percent_encode(
            file_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("codex-ops")
        )
    ));

    let write_result = (|| {
        fs::write(&temp_file, content)?;
        set_permissions_best_effort(&temp_file, 0o600);
        fs::rename(&temp_file, file_path)?;
        set_permissions_best_effort(file_path, 0o600);
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_file);
    }

    write_result
}

pub fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        let char = byte as char;
        if char.is_ascii_alphanumeric()
            || matches!(char, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            output.push(char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

pub fn path_to_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().to_string()
}

pub fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn temp_seed() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("{millis}-{}", std::process::id())
}

#[cfg(unix)]
fn set_permissions_best_effort(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_permissions_best_effort(_path: &Path, _mode: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_codex_home_prefers_env_value() {
        let result = default_codex_home_from(
            Some(PathBuf::from("/tmp/codex")),
            PathBuf::from("/home/user"),
        );

        assert_eq!(result, PathBuf::from("/tmp/codex"));
    }

    #[test]
    fn default_codex_home_falls_back_to_home_dot_codex() {
        let result = default_codex_home_from(None, PathBuf::from("/home/user"));

        assert_eq!(result, PathBuf::from("/home/user/.codex"));
    }

    #[test]
    fn resolves_default_helper_paths_under_codex_home() {
        let paths = resolve_storage_paths(&StorageOptions {
            codex_home: Some(PathBuf::from("/tmp/codex-home")),
            ..StorageOptions::default()
        });

        assert_eq!(paths.auth_file, PathBuf::from("/tmp/codex-home/auth.json"));
        assert_eq!(
            paths.profile_store_dir,
            PathBuf::from("/tmp/codex-home/codex-ops/auth-profiles")
        );
        assert_eq!(
            paths.account_history_file,
            PathBuf::from("/tmp/codex-home/codex-ops/auth-account-history.json")
        );
        assert_eq!(
            paths.sessions_dir,
            PathBuf::from("/tmp/codex-home/sessions")
        );
    }

    #[test]
    fn explicit_files_override_default_paths() {
        let paths = resolve_storage_paths(&StorageOptions {
            codex_home: Some(PathBuf::from("/tmp/codex-home")),
            auth_file: Some(PathBuf::from("/tmp/auth.json")),
            profile_store_dir: Some(PathBuf::from("/tmp/profiles")),
            account_history_file: Some(PathBuf::from("/tmp/history.json")),
            sessions_dir: Some(PathBuf::from("/tmp/sessions")),
        });

        assert_eq!(paths.auth_file, PathBuf::from("/tmp/auth.json"));
        assert_eq!(paths.profile_store_dir, PathBuf::from("/tmp/profiles"));
        assert_eq!(
            paths.account_history_file,
            PathBuf::from("/tmp/history.json")
        );
        assert_eq!(paths.sessions_dir, PathBuf::from("/tmp/sessions"));
    }

    #[test]
    fn percent_encodes_like_encode_uri_component_for_profile_names() {
        assert_eq!(percent_encode("account-a"), "account-a");
        assert_eq!(percent_encode("account a/b"), "account%20a%2Fb");
    }
}
