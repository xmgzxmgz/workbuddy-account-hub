//! 诊断：逐个 vault 快照尝试 login_from_file，输出失败账号与原因（默认 ignore）
use std::path::PathBuf;

#[test]
#[ignore]
fn diagnose_vault_snapshots() {
    let home = std::env::var("HOME").unwrap();
    let vault = PathBuf::from(&home).join(".workbuddy-account-hub/vault");
    let key_file = PathBuf::from(&home).join(".workbuddy-account-hub/at-rest-key");
    println!("key_file 存在: {}", key_file.is_file());

    for entry in std::fs::read_dir(&vault).into_iter().flatten().flatten() {
        let uid = entry.file_name().to_string_lossy().into_owned();
        let p = entry.path().join("snapshot").join("auth.info");
        if !p.exists() {
            println!("{uid}: 无 auth.info");
            continue;
        }
        let raw = std::fs::read_to_string(&p).unwrap_or_default();
        let d: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => { println!("{uid}: JSON 解析失败 {e}"); continue; }
        };
        let tok = d.pointer("/auth/accessToken");
        let kind = match tok {
            Some(serde_json::Value::String(s)) => format!("明文 str len={}", s.len()),
            Some(o) if o.get("$wbEncrypted").is_some() => {
                // 解出信封 keyId 对比当前密钥
                let env_raw = o.get("envelope").and_then(|x| x.as_str()).unwrap_or("");
                let kid = base64_json_keyid(env_raw);
                format!("加密信封 keyId={:?}", kid)
            }
            other => format!("其他形态: {:?}", other.map(|x| x.to_string()).map(|s| s.chars().take(40).collect::<String>())),
        };
        let has_uid = d.pointer("/account/uid").and_then(|x| x.as_str()).is_some();
        let login = wb_api::login_from_file(&p);
        // 失败时用 wb_enc 直解 accessToken 打印真实错误
        if login.is_none() {
            if let Some(tokv) = d.pointer("/auth/accessToken") {
                match wb_api::wb_enc::load_at_rest_key() {
                    Ok(key) => match wb_api::wb_enc::auth_field_string(tokv, &key) {
                        Ok(Some(pt)) => println!("  直解成功 len={} head={}", pt.len(), &pt[..pt.len().min(12)]),
                        Ok(None) => println!("  直解：非信封形态"),
                        Err(e) => println!("  直解失败: {e}"),
                    },
                    Err(e) => println!("  密钥加载失败: {e}"),
                }
            }
        }
        println!("{uid}: {} account.uid明文={} login_from_file={}", kind, has_uid, login.is_some());
    }
}

fn base64_json_keyid(env_raw: &str) -> Option<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(env_raw).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("keyId").and_then(|x| x.as_str()).map(|s| s.to_string())
}
