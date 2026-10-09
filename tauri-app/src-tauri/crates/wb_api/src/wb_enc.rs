//! WorkBuddy macOS 登录态字段解密（`$wbEncrypted` / at-rest protection）。
//!
//! 背景：WorkBuddy macOS 客户端 v5.3.8+ 会把登录态文件
//! （`workbuddy-desktop.info`）中的敏感字段（accessToken / refreshToken /
//! nickname / phoneNumber 等）加密存储；Windows 客户端仍为明文。
//! 加密字段形如：`{"$wbEncrypted":1,"envelope":"<base64(JSON)>"}`。
//!
//! 加密链路（逆向自 WorkBuddy 客户端 app.asar / Electron Framework）：
//!   1. `bootstrap.atRestSecretKey`（44 位 canonical base64，解码后 32 字节）
//!      由客户端原生模块 `workbuddyStorage.loggerGet()` 提供，仅存在于
//!      WorkBuddy 进程内部。用 `scripts/extract-mac-at-rest-key.mjs`
//!      一次性提取后保存到 `~/.workbuddy-account-hub/at-rest-key`。
//!   2. protector = SHA-256( utf8(atRestSecretKey 字符串) )
//!      keyId       = SHA-256(protector) 前 16 位 hex
//!                  （等于 `~/.workbuddy/keyblob` 中 slot 的 protectorKeyId，
//!                    可用于校验密钥是否匹配当前设备）
//!   3. 信封 = base64( JSON{suite:1, keyId, nonce, authTag, ciphertext} )
//!      明文  = AES-256-GCM(key=protector, nonce, ciphertext, aad, tag=authTag)
//!   4. AAD = "WB-AAD\0" + 0x01
//!          + len32("WBEV1") + "WBEV1"        // sym-v1 field 帧类型
//!          + len32("sym-v1") + "sym-v1"
//!          + u32be(suite=1)
//!          + len32(keyId) + keyId
//!          + 0x02                             // S.field
//!          + 0x00                             // sequence = undefined
//!          + 0x00                             // final = undefined

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// at-rest 密钥文件（内容 = bootstrap.atRestSecretKey 原始 base64 字符串）。
pub fn key_file_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".workbuddy-account-hub").join("at-rest-key")
}

/// 读取并派生 AES 密钥（protector = SHA-256(utf8(secret 字符串))）。
pub fn load_at_rest_key() -> Result<[u8; 32], String> {
    let p = key_file_path();
    let s = std::fs::read_to_string(&p).map_err(|_| {
        format!(
            "未找到 at-rest 密钥文件（{}）。\nmacOS 版 WorkBuddy 登录态字段已加密，请先运行项目 scripts/extract-mac-at-rest-key.mjs 一次性提取密钥（需 Node ≥ 21）",
            p.display()
        )
    })?;
    let secret = s.trim();
    if secret.is_empty() {
        return Err("at-rest 密钥文件为空，请重新运行提取脚本".into());
    }
    let mut h = Sha256::new();
    h.update(secret.as_bytes());
    let out = h.finalize();
    let mut k = [0u8; 32];
    k.copy_from_slice(&out);
    Ok(k)
}

/// 校验密钥是否匹配当前设备（与 ~/.workbuddy/keyblob 的 protectorKeyId 比对）。
pub fn key_matches_device(key: &[u8; 32]) -> Option<bool> {
    let kb = std::fs::read_to_string(PathBuf::from(std::env::var("HOME").ok()?).join(".workbuddy/keyblob")).ok()?;
    let v: Value = serde_json::from_str(&kb).ok()?;
    let want = v.get("slots")?.as_array()?.first()?.get("protectorKeyId")?.as_str()?.to_string();
    let got: String = Sha256::digest(key).iter().map(|b| format!("{b:02x}")).take(8).collect::<Vec<_>>().join("");
    Some(got == want)
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push((acc >> bits) as u8);
        }
    }
    Some(buf)
}

fn push_len_str(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s);
}

fn build_aad(key_id: &str, framing: &str) -> Vec<u8> {
    let (frame_name, frame_code): (&[u8], u8) = match framing {
        "file" => (b"WBEF1", 1),
        _ => (b"WBEV1", 2),
    };
    let mut a = Vec::new();
    a.extend_from_slice(b"WB-AAD\0");
    a.push(1);
    push_len_str(&mut a, frame_name);
    push_len_str(&mut a, b"sym-v1");
    a.extend_from_slice(&1u32.to_be_bytes());
    push_len_str(&mut a, key_id.as_bytes());
    a.push(frame_code);
    a.push(0);
    a.push(0);
    a
}

/// 解密单个信封（JSON：suite/keyId/nonce/authTag/ciphertext），返回明文字符串。
pub fn decrypt_envelope(env: &Value, key: &[u8; 32]) -> Result<String, String> {
    use aes_gcm::aead::{Aead, Payload};
    use aes_gcm::{Aes256Gcm, KeyInit};

    let key_id = env
        .get("keyId")
        .and_then(|x| x.as_str())
        .ok_or("信封缺少 keyId")?;
    let nonce = b64_decode(env.get("nonce").and_then(|x| x.as_str()).ok_or("信封缺少 nonce")?)
        .ok_or("nonce base64 解码失败")?;
    let mut ct = b64_decode(
        env.get("ciphertext")
            .and_then(|x| x.as_str())
            .ok_or("信封缺少 ciphertext")?,
    )
    .ok_or("ciphertext base64 解码失败")?;
    let tag = b64_decode(env.get("authTag").and_then(|x| x.as_str()).ok_or("信封缺少 authTag")?)
        .ok_or("authTag base64 解码失败")?;

    // 密文可能带 6 字节 WBE 帧前缀（"WBEV1\n" / "WBEF1 " 等）
    if ct.len() > 6 && &ct[0..3] == b"WBE" {
        ct.drain(0..6);
    }

    let cipher = Aes256Gcm::new(key.into());
    let mut last_err = String::new();
    // RustCrypto 的 Aead::decrypt 要求 ciphertext 末尾拼接 16 字节 tag
    let mut buf = ct.clone();
    buf.extend_from_slice(&tag);
    for framing in ["field", "file"] {
        let aad = build_aad(key_id, framing);
        match cipher.decrypt(nonce.as_slice().into(), Payload { msg: &buf, aad: &aad }) {
            Ok(pt) => return String::from_utf8(pt).map_err(|e| format!("明文不是有效 UTF-8: {e}")),
            Err(e) => last_err = format!("{framing}: {e}"),
        }
    }
    Err(format!(
        "AES-GCM 解密失败（密钥不匹配或数据损坏）: {last_err}"
    ))
}

/// 登录态字段值 → 明文字符串。
///
/// 兼容三种形态：
///   - 明文字符串（Windows / 旧版 macOS）→ 原样返回
///   - `{"$wbEncrypted":1,"envelope":".."}`（macOS 加密）→ 解密后返回
///   - 其他（null / 缺失）→ None
pub fn auth_field_string(v: &Value, key: &[u8; 32]) -> Result<Option<String>, String> {
    match v {
        Value::String(s) => Ok(Some(s.clone())),
        Value::Object(o) if o.contains_key("$wbEncrypted") => {
            let env_raw = o
                .get("envelope")
                .and_then(|x| x.as_str())
                .ok_or("加密字段缺少 envelope")?;
            // envelope 可能是 base64(JSON)，也兼容直接存 JSON 字符串的形态
            let env: Value = b64_decode(env_raw)
                .and_then(|b| serde_json::from_slice(&b).ok())
                .or_else(|| serde_json::from_str(env_raw).ok())
                .ok_or("envelope 解析失败（既不是 base64(JSON) 也不是 JSON）")?;
            decrypt_envelope(&env, key).map(Some)
        }
        _ => Ok(None),
    }
}
