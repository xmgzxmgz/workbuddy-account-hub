//! WorkBuddy 官方接口网络层（纯 Rust，替代 Node credits-api.js / debug-server.mjs）
//!
//! 职责：
//!   1. 读取本机登录态文件（workbuddy-desktop.info，含 accessToken JWT + account 档案）
//!   2. 调用官方接口：额度 / 签到状态 / 执行签到 / AI 记忆画像
//!   3. 解析 JWT 声明 + 有效期
//!   4. 读取本机 App 环境（版本 / 安装大小 / 平台）
//!
//! 安全：仅读取本机登录态，凭证不落盘、不回传第三方；所有请求走官方域名。

use serde::Serialize;
use serde_json::{json, Value};

pub mod models;

const API_BASE: &str = "https://copilot.tencent.com";
const BILLING_METER: &str = "/billing/meter";
const V2_METER: &str = "/v2/billing/meter";

#[derive(Serialize)]
pub struct LoginInfo {
    pub uid: String,
    pub token: String,
    pub file: String,
    pub account: Value,
}

/// 定位本机登录态文件（Mac: CodeBuddyExtension/Data/Public/auth；Win: LOCALAPPDATA 同路径）
fn auth_candidates() -> Vec<std::path::PathBuf> {
    let mut cands = Vec::new();
    let home = std::env::var("HOME").unwrap_or_default();
    let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        std::path::Path::new(&home)
            .join("AppData")
            .join("Local")
            .to_string_lossy()
            .into_owned()
    });
    if cfg!(target_os = "windows") {
        cands.push(std::path::Path::new(&local).join("CodeBuddyExtension").join("Data").join("Public").join("auth").join("workbuddy-desktop.info"));
    } else {
        cands.push(std::path::Path::new(&home).join("Library").join("Application Support").join("CodeBuddyExtension").join("Data").join("Public").join("auth").join("workbuddy-desktop.info"));
        cands.push(std::path::Path::new(&home).join("Library").join("Application Support").join("CodeBuddyExtension").join("Data").join("Public").join("auth").join("Tencent-Cloud.coding-copilot.info"));
    }
    cands.retain(|p| p.exists());
    cands
}

fn jwt_payload(token: &str) -> Option<Value> {
    let p = token.split('.').nth(1)?;
    let pad = (4 - (p.len() % 4)) % 4;
    let mut s = p.to_string();
    s.push_str(&"=".repeat(pad));
    let bytes = base64_decode(&s)?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    Some(v)
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    // 标准 base64（PAD 已补齐）
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
        acc = (acc << 6) | (v as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push((acc >> bits) as u8);
        }
    }
    Some(buf)
}

/// 读取本机登录态；返回 null 表示未登录
pub fn load_login() -> Option<LoginInfo> {
    for p in auth_candidates() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(d) = serde_json::from_str::<Value>(&s) {
                let token = d.get("auth").and_then(|a| a.get("accessToken")).and_then(|t| t.as_str()).unwrap_or("").to_string();
                if token.is_empty() { continue; }
                let uid = d.get("account")
                    .and_then(|a| a.get("uid"))
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| jwt_payload(&token).and_then(|pl| pl.get("sub").and_then(|x| x.as_str()).map(|s| s.to_string())))
                    .unwrap_or_default();
                if uid.is_empty() { continue; }
                return Some(LoginInfo {
                    uid,
                    token,
                    file: p.to_string_lossy().into_owned(),
                    account: d.get("account").cloned().unwrap_or(Value::Null),
                });
            }
        }
    }
    None
}

/// 读取整个登录态文件（含 allAccounts 已登记账号列表）
pub fn load_auth_file() -> Option<Value> {
    for p in auth_candidates() {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(d) = serde_json::from_str::<Value>(&s) {
                if d.get("auth").and_then(|a| a.get("accessToken")).is_some() {
                    return Some(d);
                }
            }
        }
    }
    None
}

fn mask(s: &str, n: usize) -> String {
    if s.is_empty() { "(空)".into() } else { format!("{}…({}字符)", &s[..s.len().min(n)], s.len()) }
}

/// 统一代理官方接口；返回 { status, body(Value), login(脱敏) }
pub fn call_api(endpoint: &str, method: &str, body: &str) -> Result<Value, String> {
    let login = load_login().ok_or("未找到本机 WorkBuddy 登录态，请先登录 WorkBuddy 客户端".to_string())?;
    let target = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else {
        format!("{}{}", API_BASE, endpoint)
    };
    let client = reqwest::blocking::Client::new();
    let mut req = match method.to_uppercase().as_str() {
        "GET" => client.get(&target),
        "POST" => client.post(&target),
        _ => return Err(format!("不支持的方法: {}", method)),
    };
    req = req
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {}", login.token))
        .header("X-User-Id", &login.uid);
    if method.to_uppercase() == "POST" {
        req = req.body(body.to_string());
    }
    let resp = req.send().map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status().as_u16();
    let text = resp.text().unwrap_or_default();
    let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Ok(json!({
        "status": status,
        "body": parsed,
        "login": { "uid": login.uid, "file": login.file, "token": mask(&login.token, 8) }
    }))
}

/// 解析 JWT 有效期 + 关键声明
pub fn jwt_info() -> Value {
    let login = match load_login() {
        Some(l) => l,
        None => return json!({ "error": "未找到本机登录态" }),
    };
    let pl = match jwt_payload(&login.token) {
        Some(p) => p,
        None => return json!({ "error": "JWT 解析失败" }),
    };
    let exp = pl.get("exp").and_then(|x| x.as_u64());
    let iat = pl.get("iat").and_then(|x| x.as_u64());
    let auth_time = pl.get("auth_time").and_then(|x| x.as_u64());
    let days = exp.map(|e| {
        let ms = (e as i64) * 1000;
        let now = chrono_now_ms();
        ((ms - now) as f64 / 86_400_000.0).ceil().max(0.0) as i64
    });
    json!({
        "issuer": pl.get("iss").cloned().unwrap_or(Value::Null),
        "subject": pl.get("sub").cloned().unwrap_or(Value::Null),
        "audience": pl.get("aud").cloned().unwrap_or(Value::Null),
        "azp": pl.get("azp").cloned().unwrap_or(Value::Null),
        "scope": pl.get("scope").cloned().unwrap_or(Value::Null),
        "email_verified": pl.get("email_verified").cloned().unwrap_or(Value::Null),
        "preferred_username": pl.get("preferred_username").cloned().unwrap_or(Value::Null),
        "issued_at": iat.map(|t| iso(t)),
        "auth_at": auth_time.map(|t| iso(t)),
        "expires_at": exp.map(|t| iso(t)),
        "remaining_days": days,
        "token_chars": login.token.len(),
        "token_type": pl.get("typ").cloned().unwrap_or(Value::Null),
    })
}

/// 一键拉全部（等价于 web 调试版的 /all）
pub fn get_all() -> Value {
    let login = load_login();
    let auth_file = load_auth_file();
    let cur = login.as_ref().map(|l| l.uid.clone()).unwrap_or_default();

    let mut result = json!({
        "ok": login.is_some(),
        "current_uid": cur,
        "login": Value::Null,
        "registered_accounts": [],
    });

    if let Some(l) = &login {
        let a = &l.account;
        result["login"] = json!({
            "uid": l.uid,
            "nickname": a.get("nickname").cloned().unwrap_or(Value::Null),
            "type": a.get("type").cloned().unwrap_or(Value::Null),
            "uin": a.get("uin").cloned().unwrap_or(Value::Null),
            "phoneNumber": a.get("phoneNumber").cloned().unwrap_or(Value::Null),
            "lastLogin": a.get("lastLogin").cloned().unwrap_or(Value::Null),
            "isAdmin": a.get("isAdmin").cloned().unwrap_or(Value::Null),
            "isCreator": a.get("isCreator").cloned().unwrap_or(Value::Null),
        });
    }
    if let Some(af) = &auth_file {
        if let Some(arr) = af.get("allAccounts").and_then(|x| x.as_array()) {
            result["registered_accounts"] = json!(arr.iter().map(|x| json!({
                "uid": x.get("uid").cloned().unwrap_or(Value::Null),
                "nickname": x.get("nickname").cloned().unwrap_or(Value::Null),
                "type": x.get("type").cloned().unwrap_or(Value::Null),
                "phoneNumber": x.get("phoneNumber").cloned().unwrap_or(Value::Null),
                "lastLogin": x.get("lastLogin").cloned().unwrap_or(Value::Null),
                "isCreator": x.get("isCreator").cloned().unwrap_or(Value::Null),
                "isAdmin": x.get("isAdmin").cloned().unwrap_or(Value::Null),
            })).collect::<Vec<_>>());
        }
    }

    if let Some(l) = &login {
        // 并行查询：额度 / 签到 / 记忆
        let quota = call_api(&format!("{}{}/get-user-resource", API_BASE, BILLING_METER), "POST", "{}")
            .unwrap_or_else(|e| json!({ "error": e }));
        let checkin = call_api(&format!("{}{}/checkin-activity-status", API_BASE, BILLING_METER), "POST", "{}")
            .unwrap_or_else(|e| json!({ "error": e }));
        let memory = call_api(&format!("{}/api/memory/profile", API_BASE), "GET", "")
            .unwrap_or_else(|e| json!({ "error": e }));
        result["quota"] = quota;
        result["checkin"] = checkin;
        result["memory"] = memory;
        result["jwt"] = jwt_info();
    }

    // 本地数据目录
    result["local_accounts"] = local_accounts(&cur);
    result["env"] = app_env();
    result
}

/// 执行今日签到（幂等：已签则跳过）
pub fn do_checkin() -> Value {
    let st = call_api(&format!("{}{}/checkin-activity-status", API_BASE, BILLING_METER), "POST", "{}")
        .unwrap_or_else(|e| json!({ "error": e }));
    if st.get("error").is_some() { return st; }
    let status = st.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
    if status == 401 { return json!({ "error": "登录态失效(401)，请重新登录 WorkBuddy" }); }
    let already = st.get("body").and_then(|b| b.get("data")).and_then(|d| d.get("today_checked_in")).and_then(|x| x.as_bool()).unwrap_or(false);
    if already {
        return json!({ "skipped": true, "message": "今日已签到，无需重复", "data": st.get("body").and_then(|b| b.get("data")).cloned().unwrap_or(Value::Null) });
    }
    let r = call_api(&format!("{}{}/daily-checkin", API_BASE, V2_METER), "POST", "{}")
        .unwrap_or_else(|e| json!({ "error": e }));
    r
}

/// 仅查询额度
pub fn get_quota() -> Value {
    call_api(&format!("{}{}/get-user-resource", API_BASE, BILLING_METER), "POST", "{}")
        .unwrap_or_else(|e| json!({ "error": e }))
}

/// 仅查询签到状态
pub fn get_checkin() -> Value {
    call_api(&format!("{}{}/checkin-activity-status", API_BASE, BILLING_METER), "POST", "{}")
        .unwrap_or_else(|e| json!({ "error": e }))
}

// ===== 本地信息 =====

fn local_accounts(cur: &str) -> Value {
    let home = std::env::var("HOME").unwrap_or_default();
    let base = std::path::Path::new(&home).join("Library").join("Application Support").join("CodeBuddyExtension").join("Data");
    let mut accounts = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&base) {
        for e in rd.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
            let name = e.file_name().to_string_lossy().into_owned();
            let size = dir_size(&e.path());
            let has_login = e.path().join("auth").join("workbuddy-desktop.info").exists()
                || e.path().join("auth").join("Tencent-Cloud.coding-copilot.info").exists();
            let is_current = name == cur || (name == "Public" && !cur.is_empty());
            accounts.push(json!({
                "name": name,
                "size": du_human(size),
                "sizeBytes": size,
                "has_login": has_login,
                "is_current": is_current,
            }));
        }
    }
    json!({ "dataRoot": base.to_string_lossy(), "accounts": accounts })
}

fn dir_size(p: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let fp = e.path();
            if let Ok(meta) = fp.metadata() {
                if meta.is_dir() {
                    total += dir_size(&fp);
                } else {
                    total += meta.len();
                }
            }
        }
    }
    total
}

fn du_human(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, units[i])
}

fn app_env() -> Value {
    let mut out = json!({
        "platform": if cfg!(target_os = "windows") { "win32" } else { "darwin" },
        "arch": std::env::consts::ARCH,
        "node": "—", // Tauri 不含 Node
        "appBundle": "/Applications/WorkBuddy.app",
    });
    let bundle = "/Applications/WorkBuddy.app";
    let plist = format!("{}/Contents/Info.plist", bundle);
    if let Ok(s) = std::fs::read_to_string(&plist) {
        let grab = |k: &str| -> Option<String> {
            let key = format!("<key>{}</key>", k);
            let idx = s.find(&key)?;
            let rest = &s[idx + key.len()..];
            let sidx = rest.find("<string>")? + "<string>".len();
            let eidx = rest[sidx..].find("</string>")?;
            Some(rest[sidx..sidx + eidx].to_string())
        };
        if let Some(v) = grab("CFBundleShortVersionString") { out["version"] = json!(v); }
        if let Some(v) = grab("CFBundleVersion") { out["build"] = json!(v); }
    }
    out["appSize"] = json!(du_human(dir_size(std::path::Path::new(bundle))));
    out
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn iso(secs: u64) -> String {
    simple_iso(secs)
}

fn simple_iso(secs: u64) -> String {
    // 基于 UTC 的粗略格式化（年/月/日/时/分/秒）
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut days = secs / 86400;
    let mut year = 1970;
    loop {
        let leap = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 366 } else { 365 };
        if days < leap as u64 { break; }
        days -= leap as u64;
        year += 1;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let mut month = 1;
    let mut rem = days;
    let days_in = |m: u32, leap: bool| if m == 2 && leap { 29 } else { DAYS[(m - 1) as usize] };
    while month <= 12 {
        let d = days_in(month, leap);
        if rem < d as u64 { break; }
        rem -= d as u64;
        month += 1;
    }
    let day = rem + 1;
    let secs_in_day = secs % 86400;
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z", year, month, day, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn models_smoke() {
        let list = models::list_custom();
        println!("list_custom: ok={} count={} path={}", list["ok"], list["count"], list["path"]);
        let models = list["models"].as_array().unwrap();
        for m in models.iter().take(3) {
            println!("  - id={} name={} key={} url={}", m["id"], m["name"], m["apiKey"], m["url"]);
        }
        let cur = models::current_model();
        println!("current_model: ok={} id={:?} source={}", cur["ok"], cur["model_id"], cur["source"]);
        let off = models::official_list();
        println!("official: count={}", off.as_array().unwrap().len());
    }
    #[test]
    fn models_roundtrip() {
        // 备份+原子写 不实际改文件：仅验证 add/update/delete 的错误分支
        let r = models::update_model("__no_such_model__", json!({}));
        println!("update no-such -> err={:?}", r.is_err());
        let r = models::delete_model("__no_such_model__");
        println!("delete no-such -> err={:?}", r.is_err());
    }
}
