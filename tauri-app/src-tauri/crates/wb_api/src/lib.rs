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
use std::path::Path;

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

/// 构建带超时与代理的 HTTP 客户端。
/// - timeout: 整体 15s、connect 10s，避免网络挂起导致整个 get_all（含本地昵称）永久卡死 UI。
/// - 代理：尊重 HTTPS_PROXY/HTTP_PROXY 等环境变量（如本机 Clash 127.0.0.1:7897），
///   直连被墙/不通时自动走代理，避免无限等待。
fn make_client() -> reqwest::blocking::Client {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(10));
    // 代理：优先 HTTPS_PROXY，其次 HTTP_PROXY（含小写），任一存在则全局套用
    let proxy_env = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .unwrap_or_default();
    if !proxy_env.is_empty() {
        if let Ok(p) = reqwest::Proxy::all(&proxy_env) {
            builder = builder.proxy(p);
        }
    }
    builder.build().unwrap_or_else(|_| reqwest::blocking::Client::new())
}

/// 统一代理官方接口（指定登录态）；返回 { status, body(Value), login(脱敏) }
pub fn call_api_as(login: &LoginInfo, endpoint: &str, method: &str, body: &str) -> Result<Value, String> {
    let target = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else {
        format!("{}{}", API_BASE, endpoint)
    };
    let client = make_client();
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

/// 统一代理官方接口（使用当前本机登录态）；返回 { status, body(Value), login(脱敏) }
pub fn call_api(endpoint: &str, method: &str, body: &str) -> Result<Value, String> {
    let login = load_login().ok_or("未找到本机 WorkBuddy 登录态，请先登录 WorkBuddy 客户端".to_string())?;
    call_api_as(&login, endpoint, method, body)
}

/// 从指定登录态文件读取登录信息（供多账号批量操作：每个账号从自己的 vault 快照 auth.info 取 token）
pub fn login_from_file(p: &Path) -> Option<LoginInfo> {
    let s = std::fs::read_to_string(p).ok()?;
    let d: Value = serde_json::from_str(&s).ok()?;
    let token = d.get("auth").and_then(|a| a.get("accessToken")).and_then(|t| t.as_str()).unwrap_or("").to_string();
    if token.is_empty() { return None; }
    let uid = d.get("account")
        .and_then(|a| a.get("uid"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .or_else(|| jwt_payload(&token).and_then(|pl| pl.get("sub").and_then(|x| x.as_str()).map(|s| s.to_string())))
        .unwrap_or_default();
    if uid.is_empty() { return None; }
    Some(LoginInfo { uid, token, file: p.to_string_lossy().into_owned(), account: d.get("account").cloned().unwrap_or(Value::Null) })
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

/// 一键拉「本地信息」（不含任何网络请求，瞬时返回）。
/// 昵称/UID/类型/已登记账号/本地目录/环境/JWT 均为本机文件或本地解析，
/// 不再内嵌额度/签到/记忆等网络查询 —— 避免网络慢/不通时把本地昵称一起卡成空白。
/// 网络部分由前端分批独立调用 get_quota / get_checkin / get_memory 加载。
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

    // 本地数据目录 + 环境 + JWT（均为本地，快）
    result["local_accounts"] = local_accounts(&cur);
    result["env"] = app_env();
    result["jwt"] = jwt_info();
    result
}

/// 仅查询 AI 记忆画像（网络部分，独立加载，失败不影响本地信息）
pub fn get_memory() -> Value {
    call_api(&format!("{}/api/memory/profile", API_BASE), "GET", "")
        .unwrap_or_else(|e| json!({ "error": e }))
}

/// 执行今日签到（幂等：已签则跳过）—— 指定登录态
pub fn do_checkin_as(login: &LoginInfo) -> Value {
    let st = call_api_as(login, &format!("{}{}/checkin-activity-status", API_BASE, BILLING_METER), "POST", "{}")
        .unwrap_or_else(|e| json!({ "error": e }));
    if st.get("error").is_some() { return st; }
    let status = st.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
    if status == 401 { return json!({ "error": "登录态失效(401)，请重新登录 WorkBuddy" }); }
    let already = st.get("body").and_then(|b| b.get("data")).and_then(|d| d.get("today_checked_in")).and_then(|x| x.as_bool()).unwrap_or(false);
    if already {
        return json!({ "skipped": true, "message": "今日已签到，无需重复", "data": st.get("body").and_then(|b| b.get("data")).cloned().unwrap_or(Value::Null) });
    }
    let r = call_api_as(login, &format!("{}{}/daily-checkin", API_BASE, V2_METER), "POST", "{}")
        .unwrap_or_else(|e| json!({ "error": e }));
    r
}

/// 执行今日签到（当前本机登录态）
pub fn do_checkin() -> Value {
    match load_login() {
        Some(l) => do_checkin_as(&l),
        None => json!({ "error": "未找到本机 WorkBuddy 登录态，请先登录 WorkBuddy 客户端" }),
    }
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

// ===== 宠物旅行（buddy travel） =====

/// 宠物状态端点（所有 travel 接口共用的路径前缀）
const BUDDY_TRAVEL: &str = "/activity/growth/buddy/travel";

/// 查询宠物当前状态（idle / traveling / arrived）—— 指定登录态
pub fn buddy_status_as(login: &LoginInfo) -> Value {
    call_api_as(login, &format!("{}{}/status", API_BASE, BUDDY_TRAVEL), "GET", "")
        .unwrap_or_else(|e| json!({ "error": e }))
}
/// 查询宠物当前状态（当前本机登录态）
pub fn buddy_status() -> Value {
    match load_login() {
        Some(l) => buddy_status_as(&l),
        None => json!({ "error": "未找到本机 WorkBuddy 登录态" }),
    }
}

/// 查询可派出地点列表 —— 指定登录态
pub fn buddy_config_as(login: &LoginInfo) -> Value {
    call_api_as(login, &format!("{}{}/config", API_BASE, BUDDY_TRAVEL), "GET", "")
        .unwrap_or_else(|e| json!({ "error": e }))
}
/// 查询可派出地点列表（当前本机登录态）
pub fn buddy_config() -> Value {
    match load_login() {
        Some(l) => buddy_config_as(&l),
        None => json!({ "error": "未找到本机 WorkBuddy 登录态" }),
    }
}

/// 派出宠物前往指定地点 —— 指定登录态
pub fn buddy_depart_as(login: &LoginInfo, location_id: &str) -> Value {
    // WorkBuddy 官方 depart 接口要求 location_id 为整数；前端统一传字符串 "1"，
    // 这里智能解析为数字（解析失败则退回原字符串，保持向后兼容）。
    let lid: serde_json::Value = location_id
        .trim()
        .parse::<i64>()
        .map(serde_json::Value::from)
        .unwrap_or_else(|_| serde_json::Value::from(location_id));
    let body = json!({ "location_id": lid }).to_string();
    call_api_as(login, &format!("{}{}/depart", API_BASE, BUDDY_TRAVEL), "POST", &body)
        .unwrap_or_else(|e| json!({ "error": e }))
}
/// 派出宠物（当前本机登录态）
pub fn buddy_depart(location_id: &str) -> Value {
    match load_login() {
        Some(l) => buddy_depart_as(&l, location_id),
        None => json!({ "error": "未找到本机 WorkBuddy 登录态" }),
    }
}

/// 领取已归来宠物的奖励 —— 指定登录态
pub fn buddy_claim_as(login: &LoginInfo) -> Value {
    call_api_as(login, &format!("{}{}/claim", API_BASE, BUDDY_TRAVEL), "POST", "{}")
        .unwrap_or_else(|e| json!({ "error": e }))
}
/// 领取已归来宠物的奖励（当前本机登录态）
pub fn buddy_claim() -> Value {
    match load_login() {
        Some(l) => buddy_claim_as(&l),
        None => json!({ "error": "未找到本机 WorkBuddy 登录态" }),
    }
}

// ===== 本地信息 =====

/// WorkBuddy 本机数据根目录（跨平台）：
///   macOS:   ~/Library/Application Support/CodeBuddyExtension/Data
///   Windows: %APPDATA%\CodeBuddyExtension\Data
fn data_root() -> std::path::PathBuf {
    if cfg!(target_os = "windows") {
        std::path::Path::new(&std::env::var("APPDATA").unwrap_or_default())
            .join("CodeBuddyExtension").join("Data")
    } else {
        std::path::Path::new(&std::env::var("HOME").unwrap_or_default())
            .join("Library").join("Application Support").join("CodeBuddyExtension").join("Data")
    }
}

/// WorkBuddy 安装包路径（跨平台），找不到返回 None（不崩溃）
fn app_bundle_path() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "windows") {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let candidates = [
            format!("{}\\Programs\\WorkBuddy\\WorkBuddy.exe", local),
            "C:\\Program Files\\WorkBuddy\\WorkBuddy.exe".to_string(),
            format!("{}\\WorkBuddy\\WorkBuddy.exe", local),
        ];
        candidates.iter().map(std::path::PathBuf::from).find(|p| p.exists())
    } else {
        let p = std::path::Path::new("/Applications/WorkBuddy.app");
        if p.exists() { Some(p.to_path_buf()) } else { None }
    }
}

fn local_accounts(cur: &str) -> Value {
    let base = data_root();
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
    let bundle = app_bundle_path();
    let app_bundle_str = bundle.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "（未检测到安装路径）".into());
    let mut out = json!({
        "platform": if cfg!(target_os = "windows") { "win32" } else { "darwin" },
        "arch": std::env::consts::ARCH,
        "node": "—", // Tauri 不含 Node
        "appBundle": app_bundle_str,
    });
    if let Some(b) = &bundle {
        if cfg!(target_os = "macos") {
            let plist = format!("{}/Contents/Info.plist", b.display());
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
        }
        // Windows 暂未解析 exe 版本资源，保留平台/架构即可；不崩溃。
        out["appSize"] = json!(du_human(dir_size(b)));
    }
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
