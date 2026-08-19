//! WorkBuddy 账户中枢 — 纯文件 / 进程操作逻辑
//!
//! 该 crate 不依赖 tauri、不发起网络请求，只负责：
//!   1. 枚举本机 WorkBuddy 账号（读取 local_storage 登记表 + 登录态文件）
//!   2. 快照当前账号状态（local_storage + 登录态文件）
//!   3. 切换账号（整体换入目标账号快照，自动备份可回滚）
//!   4. 退出 / 启动 WorkBuddy
//!
//! 切换设计参考 CC Switch：切换完成后返回 `restart_required=true`，
//! 由前端提示用户"重启 WorkBuddy 使登录态生效"，不自动强杀后硬启。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize)]
pub struct AccountInfo {
    pub uid: String,
    pub nickname: Option<String>,
    pub current: bool,
    pub has_snapshot: bool,
}

#[derive(Serialize)]
pub struct SnapshotMeta {
    pub uid: String,
    pub ts: String,
    pub local_files: usize,
    pub auth_included: bool,
    pub dest: String,
}

#[derive(Serialize)]
pub struct SwitchResult {
    pub restart_required: bool,
    pub message: String,
    pub uid: String,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

/// WorkBuddy 本地数据根
pub fn workbuddy_home() -> PathBuf {
    home().join(".workbuddy")
}

/// 本机 local_storage 目录（账号登记表 + 各账号配置所在）
pub fn local_storage_dir() -> PathBuf {
    workbuddy_home().join("local_storage")
}

/// 登录态文件（v5.3.8+ 明文 JSON，含 accessToken）。切换账号必须连它一起换。
pub fn auth_file() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| home().join("AppData").join("Local").to_string_lossy().into_owned()))
    } else {
        home().join("Library").join("Application Support").join("CodeBuddyExtension").join("Data").join("Public").join("auth")
    };
    let p = base.join("workbuddy-desktop.info");
    if p.exists() { Some(p) } else { None }
}

/// 保险库根目录（按 uid 分桶）
pub fn vault_dir() -> PathBuf {
    home().join(".workbuddy-account-hub").join("vault")
}

/// 读取登录态文件中的当前 uid
pub fn current_uid() -> Option<String> {
    let f = auth_file()?;
    let s = std::fs::read_to_string(&f).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v.get("account")
        .and_then(|a| a.get("uid"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
}

/// 在 local_storage 中定位账号登记表（JSON 数组 [{userId, data}, ...]）
fn find_registry() -> Option<Vec<(String, Option<String>)>> {
    let ls = local_storage_dir();
    let rd = std::fs::read_dir(&ls).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("info") { continue; }
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&s) {
                let mut out = Vec::new();
                for item in arr {
                    if let Some(uid) = item.get("userId").and_then(|x| x.as_str()) {
                        let nick = item
                            .get("data")
                            .and_then(|d| d.get("nickname"))
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string());
                        out.push((uid.to_string(), nick));
                    }
                }
                if !out.is_empty() { return Some(out); }
            }
        }
    }
    None
}

fn snapshot_exists(vault: &Path, uid: &str) -> bool {
    vault.join(uid).join("snapshot").exists()
}

/// 枚举本机账号 + 当前账号 + 是否已快照
pub fn list_accounts(vault: &Path) -> Vec<AccountInfo> {
    let cur = current_uid();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    if let Some(reg) = find_registry() {
        for (uid, nick) in reg {
            if seen.insert(uid.clone()) {
                out.push(AccountInfo {
                    uid: uid.clone(),
                    nickname: nick,
                    current: cur.as_deref() == Some(uid.as_str()),
                    has_snapshot: snapshot_exists(vault, &uid),
                });
            }
        }
    }
    // 确保当前账号始终出现（即使不在登记表中）
    if let Some(c) = &cur {
        if seen.insert(c.clone()) {
            out.push(AccountInfo {
                uid: c.clone(),
                nickname: None,
                current: true,
                has_snapshot: snapshot_exists(vault, c),
            });
        }
    }
    out
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}

fn count_files(p: &Path) -> usize {
    if !p.exists() { return 0; }
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().count())
        .unwrap_or(0)
}

/// 快照当前账号（local_storage + 登录态文件）到 vault/<uid>/snapshot/
pub fn snapshot_current(vault: &Path) -> Result<SnapshotMeta, String> {
    let uid = current_uid().ok_or("未找到当前登录态，请先登录 WorkBuddy")?;
    let dest = vault.join(&uid).join("snapshot");
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    let ls = local_storage_dir();
    if ls.exists() {
        copy_dir_all(&ls, &dest.join("local_storage")).map_err(|e| e.to_string())?;
    }
    let mut auth_included = false;
    if let Some(af) = auth_file() {
        if let Some(parent) = dest.join("auth.info").parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::copy(&af, dest.join("auth.info")).map_err(|e| e.to_string())?;
        auth_included = true;
    }

    Ok(SnapshotMeta {
        uid: uid.clone(),
        ts: chrono_now(),
        local_files: count_files(&dest.join("local_storage")),
        auth_included,
        dest: dest.to_string_lossy().into_owned(),
    })
}

fn chrono_now() -> String {
    // 简单时间戳，避免引入 chrono 依赖
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    secs.to_string()
}

/// 切换账号：整体换入 vault/<uid>/snapshot/。会先退出 WorkBuddy（避免写冲突），
/// 并在写回前自动备份当前状态到 vault/_rollback/<ts>/。返回 restart_required 提示重启。
pub fn switch_account(vault: &Path, uid: &str) -> Result<SwitchResult, String> {
    let src = vault.join(uid).join("snapshot");
    if !src.exists() {
        return Err(format!("账号 {} 尚未快照，请先对其执行「快照当前账号」", uid));
    }
    if is_workbuddy_running() {
        quit_workbuddy()?;
    }

    // 备份当前
    let rb = vault.join("_rollback").join(chrono_now());
    std::fs::create_dir_all(&rb).map_err(|e| e.to_string())?;
    let cur_ls = local_storage_dir();
    if cur_ls.exists() {
        copy_dir_all(&cur_ls, &rb.join("local_storage")).map_err(|e| e.to_string())?;
    }
    if let Some(af) = auth_file() {
        std::fs::copy(&af, rb.join("auth.info")).ok();
    }

    // 写入目标快照
    let src_ls = src.join("local_storage");
    if cur_ls.exists() {
        std::fs::remove_dir_all(&cur_ls).map_err(|e| e.to_string())?;
    }
    if src_ls.exists() {
        copy_dir_all(&src_ls, &cur_ls).map_err(|e| e.to_string())?;
    }
    let src_auth = src.join("auth.info");
    if src_auth.exists() {
        if let Some(af) = auth_file() {
            if let Some(parent) = af.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::copy(&src_auth, &af).map_err(|e| e.to_string())?;
        }
    }

    Ok(SwitchResult {
        restart_required: true,
        message: "切换完成，请重启 WorkBuddy 使登录态生效（参考 CC Switch）".to_string(),
        uid: uid.to_string(),
    })
}

/// WorkBuddy 是否正在运行
pub fn is_workbuddy_running() -> bool {
    if cfg!(target_os = "macos") {
        Command::new("pgrep").args(["-f", "WorkBuddy"]).output().map(|o| o.status.success()).unwrap_or(false)
    } else if cfg!(target_os = "windows") {
        Command::new("tasklist").args(["/FI", "IMAGENAME eq WorkBuddy.exe"]).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("WorkBuddy.exe"))
            .unwrap_or(false)
    } else {
        false
    }
}

/// 优雅退出 WorkBuddy（macOS: osascript；Windows: taskkill）
pub fn quit_workbuddy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let _ = Command::new("osascript").args(["-e", "tell application \"WorkBuddy\" to quit"]).output();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let _ = Command::new("pkill").args(["-f", "WorkBuddy"]).output();
        Ok(())
    } else if cfg!(target_os = "windows") {
        let _ = Command::new("taskkill").args(["/IM", "WorkBuddy.exe", "/F"]).output();
        Ok(())
    } else {
        Err("当前平台不支持退出 WorkBuddy".into())
    }
}

/// 启动 WorkBuddy（切换后由用户点击「重启」触发）
pub fn launch_workbuddy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        Command::new("open").args(["-a", "WorkBuddy"]).output().map(|_| ()).map_err(|e| e.to_string())
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/c", "start", "", "WorkBuddy"]).output().map(|_| ()).map_err(|e| e.to_string())
    } else {
        Err("当前平台不支持启动 WorkBuddy".into())
    }
}
