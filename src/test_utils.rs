#![cfg(test)]

pub(crate) fn temp_dir(prefix: &str) -> std::path::PathBuf {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_millis();
    std::env::temp_dir().join(format!("{prefix}-{millis}-{}", std::process::id()))
}

pub(crate) fn jwt(header: &str, payload: &str) -> String {
    format!(
        "{}.{}.signature",
        encode_base64url(header),
        encode_base64url(payload)
    )
}

pub(crate) fn auth_content(account_id: &str, email: &str, plan: &str) -> String {
    let payload = serde_json::json!({
        "sub": format!("auth0|{account_id}"),
        "email": email,
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account_id,
            "chatgpt_plan_type": plan,
            "chatgpt_user_id": format!("user-{account_id}"),
            "user_id": format!("user-{account_id}")
        }
    });
    let token = jwt(r#"{"alg":"RS256","kid":"key-1"}"#, &payload.to_string());
    serde_json::to_string_pretty(&serde_json::json!({
        "auth_mode": "chatgpt",
        "tokens": {
            "id_token": token,
            "refresh_token": "synthetic-refresh-token",
            "account_id": account_id
        },
        "last_refresh": "2026-05-12T05:32:41.917677755Z"
    }))
    .expect("serialize auth content")
}

pub(crate) fn encode_base64url(value: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = value.as_bytes();
    let mut output = String::new();
    let mut index = 0;

    while index < bytes.len() {
        let b0 = bytes[index];
        let b1 = *bytes.get(index + 1).unwrap_or(&0);
        let b2 = *bytes.get(index + 2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if index + 1 < bytes.len() {
            output.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        }
        if index + 2 < bytes.len() {
            output.push(TABLE[(b2 & 0x3f) as usize] as char);
        }
        index += 3;
    }

    output
}
