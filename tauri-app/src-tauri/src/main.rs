// WorkBuddy 账户中枢 — Tauri 2 桌面端（纯 Rust 实现，无 Node 依赖）
//
// 职责：
//   - 账号枚举 / 快照 / 切换：account_ops（保留每个账号登录态，参考 CC Switch）
//   - 仪表盘数据（额度/签到/记忆/JWT/环境/本地目录）：wb_api（直连官方接口）
//
// 切换设计：switch_account 会先退出 WorkBuddy，整体换入目标账号快照，
// 返回 restart_required=true，由前端提示「重启 WorkBuddy 使登录态生效」。

use account_ops as ops;
use serde_json::{json, Value};
use std::process::Command;
use tauri::Manager;
use wb_api as api;

#[tauri::command]
fn get_all() -> Value {
    let mut v = api::get_all();
    // 侧边栏账号列表改为「登记表 + 当前账号」的完整枚举（不随当前登录态 allAccounts 收缩，
    // 避免切换后只剩目标账号、原账号消失在列表里无法切回）。
    let accs = ops::list_accounts(&ops::vault_dir());
    if let Some(arr) = v.get_mut("registered_accounts").and_then(|x| x.as_array_mut()) {
        *arr = serde_json::to_value(accs).unwrap_or(Value::Array(vec![]))
            .as_array().cloned().unwrap_or_default();
    }
    v
}

#[tauri::command]
fn get_quota() -> Value {
    api::get_quota()
}

#[tauri::command]
fn get_checkin() -> Value {
    api::get_checkin()
}

#[tauri::command]
fn get_memory() -> Value {
    api::get_memory()
}

#[tauri::command]
fn do_checkin() -> Value {
    api::do_checkin()
}

// ---------- 宠物旅行（buddy travel） ----------

#[tauri::command]
fn buddy_status() -> Value {
    api::buddy_status()
}

#[tauri::command]
fn buddy_config() -> Value {
    api::buddy_config()
}

#[tauri::command]
fn buddy_depart(location_id: String) -> Value {
    api::buddy_depart(&location_id)
}

#[tauri::command]
fn buddy_claim() -> Value {
    api::buddy_claim()
}

// ---------- 多账号批量操作（每个账号用各自 vault 快照里的登录态 token 发起请求） ----------

/// 从某账号的 vault 快照读取登录态（auth.info 含该账号 accessToken）
fn account_login(vault: &std::path::Path, uid: &str) -> Option<wb_api::LoginInfo> {
    let p = vault.join(uid).join("snapshot").join("auth.info");
    if !p.exists() { return None; }
    wb_api::login_from_file(&p)
}

/// 一键签到：对所有「已保存登录态」的账号执行今日签到，返回每个账号的结果
#[tauri::command]
fn checkin_all() -> Value {
    let vault = ops::vault_dir();
    let accs = ops::list_accounts(&vault);
    let mut results = Vec::new();
    let mut ok = 0u32; let mut fail = 0u32; let mut skipped = 0u32;
    for a in accs {
        if !a.has_snapshot {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": false, "error": "无登录态快照"}));
            fail += 1; continue;
        }
        let Some(login) = account_login(&vault, &a.uid) else {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": false, "error": "登录态文件缺失或无效"}));
            fail += 1; continue;
        };
        let r = api::do_checkin_as(&login);
        if let Some(err) = r.get("error") {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": false, "error": err}));
            fail += 1; continue;
        }
        let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
        let sk = r.get("skipped").and_then(|x| x.as_bool()).unwrap_or(false);
        let body = r.get("body");
        let d = body.and_then(|b| b.get("data")).or(body);
        let msg = d.and_then(|x| x.get("message")).and_then(|x| x.as_str())
            .or_else(|| r.get("body").and_then(|b| b.get("message")).and_then(|x| x.as_str()))
            .unwrap_or("").to_string();
        if status == 200 && !sk { ok += 1; } else if sk { skipped += 1; } else { fail += 1; }
        results.push(json!({
            "uid": a.uid, "nickname": a.nickname, "ok": status == 200, "skipped": sk,
            "status": status, "message": msg
        }));
    }
    json!({ "ok": true, "results": results, "summary": { "total": results.len(), "ok": ok, "fail": fail, "skipped": skipped } })
}

/// 查询所有「已保存登录态」账号的宠物当前状态
#[tauri::command]
fn buddy_all_status() -> Value {
    let vault = ops::vault_dir();
    let accs = ops::list_accounts(&vault);
    let mut accounts = Vec::new();
    for a in accs {
        if !a.has_snapshot {
            accounts.push(json!({"uid": a.uid, "nickname": a.nickname, "has_login": false, "error": "无登录态快照"}));
            continue;
        }
        let Some(login) = account_login(&vault, &a.uid) else {
            accounts.push(json!({"uid": a.uid, "nickname": a.nickname, "has_login": false, "error": "登录态文件缺失或无效"}));
            continue;
        };
        let r = api::buddy_status_as(&login);
        if let Some(err) = r.get("error") {
            accounts.push(json!({"uid": a.uid, "nickname": a.nickname, "has_login": true, "error": err}));
            continue;
        }
        let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
        let body = r.get("body");
        let d = body.and_then(|b| b.get("data")).or(body);
        let state = d.and_then(|x| x.get("state")).and_then(|x| x.as_str()).unwrap_or("unknown").to_string();
        let location = d.and_then(|x| {
            let l = x.get("location");
            if let Some(l) = l {
                if l.is_string() { return l.as_str().map(|s| s.to_string()); }
                if l.is_object() { return l.get("name").and_then(|s| s.as_str()).map(|s| s.to_string()); }
            }
            None
        });
        let reward = d.and_then(|x| x.get("reward_credit")).cloned();
        let arrive_at = d.and_then(|x| x.get("arrive_at")).and_then(|x| x.as_u64()).unwrap_or(0);
        let daily = d.and_then(|x| x.get("daily_limit_reached")).and_then(|x| x.as_bool()).unwrap_or(false);
        accounts.push(json!({
            "uid": a.uid, "nickname": a.nickname, "has_login": true, "status": status,
            "state": state, "location": location, "reward_credit": reward,
            "arrive_at": arrive_at, "daily_limit_reached": daily
        }));
    }
    json!({ "ok": true, "accounts": accounts })
}

/// 一键派出所有「已保存登录态」账号的宠物到同一地点
#[tauri::command]
fn buddy_all_depart(location_id: String) -> Value {
    let vault = ops::vault_dir();
    let accs = ops::list_accounts(&vault);
    let mut results = Vec::new();
    let mut ok = 0u32; let mut fail = 0u32;
    for a in accs {
        if !a.has_snapshot {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "error": "无登录态快照"}));
            fail += 1; continue;
        }
        let Some(login) = account_login(&vault, &a.uid) else {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "error": "登录态文件缺失或无效"}));
            fail += 1; continue;
        };
        let r = api::buddy_depart_as(&login, &location_id);
        if let Some(err) = r.get("error") {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "error": err}));
            fail += 1; continue;
        }
        let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
        let body = r.get("body");
        let d = body.and_then(|b| b.get("data")).or(body);
        let msg = d.and_then(|x| x.get("message")).and_then(|x| x.as_str())
            .or_else(|| r.get("body").and_then(|b| b.get("message")).and_then(|x| x.as_str()))
            .unwrap_or("").to_string();
        let success = status == 200;
        if success { ok += 1; } else { fail += 1; }
        results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": success, "status": status, "message": msg}));
    }
    json!({ "ok": true, "results": results, "summary": { "total": results.len(), "ok": ok, "fail": fail } })
}

/// 一键领取所有「已保存登录态」账号的宠物奖励
#[tauri::command]
fn buddy_all_claim() -> Value {
    let vault = ops::vault_dir();
    let accs = ops::list_accounts(&vault);
    let mut results = Vec::new();
    let mut ok = 0u32; let mut fail = 0u32;
    for a in accs {
        if !a.has_snapshot {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "error": "无登录态快照"}));
            fail += 1; continue;
        }
        let Some(login) = account_login(&vault, &a.uid) else {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "error": "登录态文件缺失或无效"}));
            fail += 1; continue;
        };
        let r = api::buddy_claim_as(&login);
        if let Some(err) = r.get("error") {
            results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "error": err}));
            fail += 1; continue;
        }
        let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
        let body = r.get("body");
        let d = body.and_then(|b| b.get("data")).or(body);
        let msg = d.and_then(|x| x.get("message")).and_then(|x| x.as_str())
            .or_else(|| r.get("body").and_then(|b| b.get("message")).and_then(|x| x.as_str()))
            .unwrap_or("").to_string();
        let success = status == 200;
        if success { ok += 1; } else { fail += 1; }
        results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": success, "status": status, "message": msg}));
    }
    json!({ "ok": true, "results": results, "summary": { "total": results.len(), "ok": ok, "fail": fail } })
}

/// 单个账号：派出宠物到指定地点（用于全部账号表格里的逐行操作）
#[tauri::command]
fn buddy_depart_for(uid: String, location_id: String) -> Value {
    let vault = ops::vault_dir();
    let Some(login) = account_login(&vault, &uid) else {
        return json!({"uid": uid, "ok": false, "error": "登录态文件缺失或无效"});
    };
    let r = api::buddy_depart_as(&login, &location_id);
    if let Some(err) = r.get("error") { return json!({"uid": uid, "ok": false, "error": err}); }
    let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
    let body = r.get("body");
    let d = body.and_then(|b| b.get("data")).or(body);
    let msg = d.and_then(|x| x.get("message")).and_then(|x| x.as_str())
        .or_else(|| r.get("body").and_then(|b| b.get("message")).and_then(|x| x.as_str()))
        .unwrap_or("").to_string();
    json!({"uid": uid, "ok": status == 200, "status": status, "message": msg, "body": r.get("body").cloned().unwrap_or(Value::Null)})
}

/// 单个账号：领取宠物奖励（用于全部账号表格里的逐行操作）
#[tauri::command]
fn buddy_claim_for(uid: String) -> Value {
    let vault = ops::vault_dir();
    let Some(login) = account_login(&vault, &uid) else {
        return json!({"uid": uid, "ok": false, "error": "登录态文件缺失或无效"});
    };
    let r = api::buddy_claim_as(&login);
    if let Some(err) = r.get("error") { return json!({"uid": uid, "ok": false, "error": err}); }
    let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
    let body = r.get("body");
    let d = body.and_then(|b| b.get("data")).or(body);
    let msg = d.and_then(|x| x.get("message")).and_then(|x| x.as_str())
        .or_else(|| r.get("body").and_then(|b| b.get("message")).and_then(|x| x.as_str()))
        .unwrap_or("").to_string();
    json!({"uid": uid, "ok": status == 200, "status": status, "message": msg, "body": r.get("body").cloned().unwrap_or(Value::Null)})
}

// ---------- 模型 / API 管理（wb_api::models） ----------

#[tauri::command]
fn list_custom_models() -> Value {
    api::models::list_custom()
}

#[tauri::command]
fn add_custom_model(model: Value) -> Result<Value, String> {
    api::models::add_model(model)
}

#[tauri::command]
fn update_custom_model(id: String, patch: Value) -> Result<Value, String> {
    api::models::update_model(&id, patch)
}

#[tauri::command]
fn delete_custom_model(id: String) -> Result<Value, String> {
    api::models::delete_model(&id)
}

#[tauri::command]
fn test_custom_model(model: Value) -> Value {
    api::models::test_model(&model)
}

#[tauri::command]
fn official_models() -> Value {
    api::models::official_list()
}

#[tauri::command]
fn current_model() -> Value {
    api::models::current_model()
}

#[tauri::command]
fn models_path() -> Value {
    json!({
        "path": api::models::models_path().display().to_string()
    })
}

// ---------- 账号管理（account_ops） ----------

#[tauri::command]
fn list_accounts() -> Value {
    let accs = ops::list_accounts(&ops::vault_dir());
    Value::Object({
        let mut m = serde_json::Map::new();
        m.insert("accounts".into(), serde_json::to_value(accs).unwrap_or(Value::Null));
        m.insert("current_uid".into(), serde_json::to_value(ops::current_uid()).unwrap_or(Value::Null));
        m.insert("workbuddy_running".into(), Value::Bool(ops::is_workbuddy_running()));
        m
    })
}

#[tauri::command]
fn snapshot_current() -> Result<Value, String> {
    let m = ops::snapshot_current(&ops::vault_dir())?;
    Ok(serde_json::to_value(m).unwrap_or(Value::Null))
}

#[tauri::command]
fn ensure_snapshot() -> Result<Value, String> {
    let m = ops::ensure_snapshot(&ops::vault_dir())?;
    Ok(serde_json::to_value(m).unwrap_or(Value::Null))
}

#[tauri::command]
fn backup_all() -> Result<Value, String> {
    let rows = ops::backup_all(&ops::vault_dir())?;
    Ok(serde_json::to_value(rows).unwrap_or(Value::Null))
}

#[tauri::command]
fn switch_account(uid: String) -> Result<Value, String> {
    let r = ops::switch_account(&ops::vault_dir(), &uid)?;
    Ok(serde_json::to_value(r).unwrap_or(Value::Null))
}

#[tauri::command]
fn restart_workbuddy() -> Result<(), String> {
    ops::launch_workbuddy()
}

#[tauri::command]
fn app_running() -> bool {
    ops::is_workbuddy_running()
}

// 历史备份：list_backups() 返回全部账号的历史备份；list_backups(uid) 只返回某账号。
#[tauri::command]
fn list_backups(uid: Option<String>) -> Value {
    let uid = uid.as_deref();
    let rows = ops::list_backups(&ops::vault_dir(), uid);
    serde_json::to_value(rows).unwrap_or(Value::Null)
}

// 单份备份详情（含完整文件树）
#[tauri::command]
fn backup_detail(uid: String, ts: String) -> Result<Value, String> {
    let m = ops::backup_detail(&ops::vault_dir(), &uid, &ts)?;
    Ok(serde_json::to_value(m).unwrap_or(Value::Null))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_all,
            get_quota,
            get_checkin,
            get_memory,
            do_checkin,
            checkin_all,
            buddy_status,
            buddy_config,
            buddy_depart,
            buddy_claim,
            buddy_all_status,
            buddy_all_depart,
            buddy_all_claim,
            buddy_depart_for,
            buddy_claim_for,
            list_accounts,
            snapshot_current,
            ensure_snapshot,
            backup_all,
            switch_account,
            restart_workbuddy,
            app_running,
            list_backups,
            backup_detail,
            list_custom_models,
            add_custom_model,
            update_custom_model,
            delete_custom_model,
            test_custom_model,
            official_models,
            current_model,
            models_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running WorkBuddy Account Hub");
}

fn main() {
    run();
}
