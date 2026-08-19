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
    api::get_all()
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
fn do_checkin() -> Value {
    api::do_checkin()
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_all,
            get_quota,
            get_checkin,
            do_checkin,
            list_accounts,
            snapshot_current,
            switch_account,
            restart_workbuddy,
            app_running,
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
