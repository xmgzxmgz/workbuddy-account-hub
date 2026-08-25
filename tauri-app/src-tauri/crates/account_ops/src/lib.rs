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
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use rusqlite::Connection;

// 渲染层会话置顶状态存储在 Electron localStorage（leveldb）里，键名形如
// `wb:conversation-list:expanded-state:u:<uid>`，值 JSON 含 `{"pinned":true,...}`。
// 切换账号时需把 `u:<旧uid>` 键复制为 `u:<新uid>`（见 migrate_renderer_pin_state）。
use leveldb::database::Database;
use leveldb::iterator::{Iterable, LevelDBIterator};
use leveldb::kv::KV;
use leveldb::options::{Options, ReadOptions, WriteOptions};

#[derive(Serialize)]
pub struct AccountInfo {
    pub uid: String,
    pub nickname: Option<String>,
    pub current: bool,
    pub has_snapshot: bool,
}

#[derive(Serialize)]
pub struct SwitchResult {
    pub restart_required: bool,
    pub restart_workbuddy: bool,
    pub aggregate_id: String,
    pub message: String,
    pub uid: String,
}

/// 一份历史备份的元数据（历史备份页展示用）
#[derive(Serialize)]
pub struct BackupMeta {
    pub uid: String,
    pub ts: String,
    pub file_count: usize,
    pub auth_included: bool,
    pub bytes: u64,
    pub local_files: Vec<String>,
    pub auth_file_name: Option<String>,
    pub is_latest: bool,
}

/// 用户主目录。Windows 桌面 GUI 进程通常不设置 `$HOME`，故 Windows 优先用 `$USERPROFILE`
/// （指向 C:\Users\<user>），否则会 fallback 成进程当前工作目录「.」，导致 vault / local_storage
/// 被写到错误位置、快照与枚举全部失效。macOS 用 `$HOME`。
fn home() -> PathBuf {
    if cfg!(target_os = "windows") {
        PathBuf::from(
            std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into()),
        )
    } else {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
    }
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
///
/// 路径按平台对齐官方客户端落盘位置：
///   - macOS:   ~/Library/Application Support/CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info
///   - Windows: %LOCALAPPDATA%/CodeBuddyExtension/Data/Public/auth/workbuddy-desktop.info
/// 旧版/个别环境会直放 LOCALAPPDATA 根目录，故 Windows 额外兜底该位置。
pub fn auth_file() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if cfg!(target_os = "windows") {
        let local = PathBuf::from(
            std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
                std::env::var("USERPROFILE")
                    .map(|u| PathBuf::from(u).join("AppData").join("Local").to_string_lossy().into_owned())
                    .unwrap_or_else(|_| ".".into())
            }),
        );
        vec![
            local.join("CodeBuddyExtension").join("Data").join("Public").join("auth").join("workbuddy-desktop.info"),
            local.join("workbuddy-desktop.info"),
        ]
    } else {
        vec![home()
            .join("Library")
            .join("Application Support")
            .join("CodeBuddyExtension")
            .join("Data")
            .join("Public")
            .join("auth")
            .join("workbuddy-desktop.info")]
    };
    candidates.into_iter().find(|p| p.exists())
}

/// 保险库根目录（按 uid 分桶）
pub fn vault_dir() -> PathBuf {
    home().join(".workbuddy-account-hub").join("vault")
}

/// ⚠️ 历史警示（已彻底移除实现）：
/// 曾有一个 `rewrite_sessions_to_aggregate` 函数会把 workbuddy.db 的 sessions.user_id
/// 改写成某个 uid，用于「多账号会话合并」。该做法在 Windows 上会导致：
///   1) 会话归属信息被永久混淆、无法还原「原属哪个账号」；
///   2) 每次切换都把全部对话搬给目标账号，造成对话被改乱。
/// WorkBuddy 在 Windows 上以登录态 token 的 `sub` 隔离会话，两个账号本就该各看各的对话。
/// **切换 = 只换登录态（真实 token + 真实 uid），绝不改写会话库。** 故此处不再提供实现。


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

/// 在 local_storage 中收集账号登记表中的全部账号（JSON 数组 [{userId, data}, ...]）。
///
/// 关键修复：不再「返回第一个能解析的登记表」，而是遍历所有 `.info` 登记表，
/// 把里面出现的所有 `userId` 合并成并集（去重）。原因：WorkBuddy 官方客户端
/// 会在账号切换/刷新时把某个 entry_xxx.info 压缩成「仅当前账号」，
/// 若只取第一个命中就会丢账号 —— 合并所有登记表 + 下面 list_accounts 的
/// vault 枚举兜底，才能保证侧边栏永远显示全部已保存账号、可切换。
fn find_registry() -> Vec<(String, Option<String>)> {
    let ls = local_storage_dir();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let rd = std::fs::read_dir(&ls).ok();
    for e in rd.into_iter().flat_map(|r| r.flatten()) {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("info") { continue; }
        let Ok(s) = std::fs::read_to_string(&p) else { continue; };
        let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&s) else { continue; };
        for item in arr {
            let Some(uid) = item.get("userId").and_then(|x| x.as_str()) else { continue; };
            let uid = uid.to_string();
            if seen.insert(uid.clone()) {
                let nick = item
                    .get("data")
                    .and_then(|d| d.get("nickname"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                out.push((uid, nick));
            }
        }
    }
    out
}

/// 从当前登录态文件(workbuddy-desktop.info)的顶层 `allAccounts` 提取本机「所有登录过的账号」。
/// 这是最权威的来源：只要某账号在此机登录过，就会出现在 allAccounts。
/// 之前的枚举只依赖 local_storage 登记表 + vault 快照，当官方把登记表压缩成「仅当前账号」、
/// 且该账号尚未快照时就会丢账号（表现为侧边栏「不显示所有账号」）。补上 allAccounts 来源可根治。
fn auth_accounts() -> Vec<(String, Option<String>)> {
    let Some(f) = auth_file() else { return Vec::new(); };
    let Ok(s) = std::fs::read_to_string(&f) else { return Vec::new(); };
    let Ok(v) = serde_json::from_str::<Value>(&s) else { return Vec::new(); };
    let mut out = Vec::new();
    if let Some(arr) = v.get("allAccounts").and_then(|x| x.as_array()) {
        for item in arr {
            let Some(uid) = item.get("uid").and_then(|u| u.as_str()) else { continue; };
            let nick = item.get("nickname").and_then(|x| x.as_str()).map(|s| s.to_string());
            out.push((uid.to_string(), nick));
        }
    }
    // allAccounts 为空时兜底：至少保留当前 account
    if out.is_empty() {
        if let Some(uid) = v.get("account").and_then(|a| a.get("uid")).and_then(|u| u.as_str()) {
            let nick = v.get("account").and_then(|a| a.get("nickname")).and_then(|x| x.as_str()).map(|s| s.to_string());
            out.push((uid.to_string(), nick));
        }
    }
    out
}

/// 从保险库(vault)目录枚举所有已保存账号的 uid（只要有 snapshot/history 目录就算）。
/// 兜底来源：即使官方登记表被压缩丢失账号，只要之前快照过，这里就能找回并可切换。
fn vault_account_uids(vault: &Path) -> Vec<String> {
    let mut uids = Vec::new();
    let Ok(rd) = std::fs::read_dir(vault) else { return uids; };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() { continue; }
        // 仅保留像 uid 的目录（含 '-'）+ 内部有 snapshot 或 history 的有效账号
        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            if name.contains('-') && (p.join("snapshot").exists() || p.join("history").exists()) {
                uids.push(name.to_string());
            }
        }
    }
    uids
}

/// 读取某个账号快照中的昵称（vault/<uid>/snapshot/local_storage 登记表或 auth.info 兜底）
fn nickname_from_vault(vault: &Path, uid: &str) -> Option<String> {
    // 1) 从该账号快照的 local_storage 登记表找昵称
    let snap_ls = vault.join(uid).join("snapshot").join("local_storage");
    if let Ok(rd) = std::fs::read_dir(&snap_ls) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("info") { continue; }
            let Ok(s) = std::fs::read_to_string(&p) else { continue; };
            let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&s) else { continue; };
            for item in arr {
                if item.get("userId").and_then(|x| x.as_str()) == Some(uid) {
                    let nick = item.get("data").and_then(|d| d.get("nickname")).and_then(|x| x.as_str()).map(|s| s.to_string());
                    if nick.is_some() { return nick; }
                }
            }
        }
    }
    // 2) 兜底：auth.info 里的 nickname / uid
    let auth_p = vault.join(uid).join("snapshot").join("auth.info");
    let Ok(s) = std::fs::read_to_string(&auth_p) else { return None; };
    let Ok(v) = serde_json::from_str::<Value>(&s) else { return None; };
    if let Some(nick) = v.get("account").and_then(|a| a.get("nickname")).and_then(|x| x.as_str()) {
        return Some(nick.to_string());
    }
    None
}

fn snapshot_exists(vault: &Path, uid: &str) -> bool {
    vault.join(uid).join("snapshot").exists()
}

/// 枚举本机账号 + 当前账号 + 是否已快照
///
/// 账号来源合并三处，保证不丢：
///   1. local_storage 所有登记表并集（find_registry）
///   2. vault 目录下所有已有 snapshot/history 的账号（兜底，防登记表被官方压缩丢账号）
///   3. 当前登录账号（永远展示）
/// 昵称优先级：登记表 > vault 快照兜底。
pub fn list_accounts(vault: &Path) -> Vec<AccountInfo> {
    let cur = current_uid();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    // 0) 当前登录态文件里的 allAccounts（最权威：本机所有登录过的账号，含尚未快照的）
    for (uid, nick) in auth_accounts() {
        if seen.insert(uid.clone()) {
            out.push(AccountInfo {
                uid: uid.clone(),
                nickname: nick.or_else(|| nickname_from_vault(vault, &uid)),
                current: cur.as_deref() == Some(uid.as_str()),
                has_snapshot: snapshot_exists(vault, &uid),
            });
        }
    }

    // 1) 合并所有登记表
    for (uid, nick) in find_registry() {
        if seen.insert(uid.clone()) {
            out.push(AccountInfo {
                uid: uid.clone(),
                nickname: nick.or_else(|| nickname_from_vault(vault, &uid)),
                current: cur.as_deref() == Some(uid.as_str()),
                has_snapshot: snapshot_exists(vault, &uid),
            });
        }
    }

    // 2) vault 兜底：登记表里没有的已快照账号也补进来（可切换）
    for uid in vault_account_uids(vault) {
        if seen.insert(uid.clone()) {
            out.push(AccountInfo {
                uid: uid.clone(),
                nickname: nickname_from_vault(vault, &uid),
                current: cur.as_deref() == Some(uid.as_str()),
                has_snapshot: true,
            });
        }
    }

    // 3) 确保当前账号始终出现
    if let Some(c) = &cur {
        if seen.insert(c.clone()) {
            out.push(AccountInfo {
                uid: c.clone(),
                nickname: nickname_from_vault(vault, c),
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

fn list_local_files(p: &Path) -> Vec<String> {
    if !p.exists() { return Vec::new(); }
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            if let Ok(meta) = e.metadata() {
                if meta.is_file() {
                    v.push(e.file_name().to_string_lossy().to_string());
                }
            } else {
                v.push(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    v.sort();
    v
}

fn dir_size(p: &Path) -> u64 {
    if !p.exists() { return 0; }
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            if let Ok(m) = e.metadata() {
                if m.is_file() {
                    total += m.len();
                } else if m.is_dir() {
                    total += dir_size(&e.path());
                }
            }
        }
    }
    total
}

/// 快照当前账号（local_storage + 登录态文件）到 vault/<uid>/history/<ts>/（多版本），
/// 并同步更新 vault/<uid>/snapshot/ 作为切换源。支持自动清理只保留最近 MAX_BACKUPS 份。
pub const MAX_BACKUPS: usize = 5;

fn take_snapshot_to(uid: &str, dest_root: &Path) -> Result<BackupMeta, String> {
    let ls = local_storage_dir();
    if ls.exists() {
        copy_dir_all(&ls, &dest_root.join("local_storage")).map_err(|e| e.to_string())?;
    }
    // 同时备份会话数据库，切换时才能恢复/迁移目标账号的历史对话
    if let Some(db) = workbuddy_db_path() {
        let _ = std::fs::copy(&db, dest_root.join("workbuddy.db"));
    }
    let mut auth_included = false;
    if let Some(af) = auth_file() {
        if let Some(parent) = dest_root.join("auth.info").parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::copy(&af, dest_root.join("auth.info")).map_err(|e| e.to_string())?;
        auth_included = true;
    }
    let ls_files = list_local_files(&dest_root.join("local_storage"));
    let auth_file_name = if auth_included { Some("auth.info".to_string()) } else { None };
    Ok(BackupMeta {
        uid: uid.to_string(),
        ts: chrono_now(),
        file_count: ls_files.len(),
        local_files: ls_files,
        auth_included,
        bytes: dir_size(dest_root),
        auth_file_name,
        is_latest: false,
    })
}

/// 对 history/<ts>/ 目录做自动清理：仅保留最新的 MAX_BACKUPS 份
fn cleanup_history(vault: &Path, uid: &str) {
    let hist = vault.join(uid).join("history");
    if !hist.exists() { return; }
    let mut ts: Vec<String> = std::fs::read_dir(&hist)
        .ok()
        .map(|rd| rd.flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .collect::<Vec<_>>())
        .unwrap_or_default();
    // timestamp 是 unix 秒，倒序保留最新
    ts.sort_by(|a, b| b.cmp(a));
    if ts.len() > MAX_BACKUPS {
        for old in ts.iter().skip(MAX_BACKUPS) {
            let _ = std::fs::remove_dir_all(hist.join(old));
        }
    }
}

/// 快照当前账号为一份可追溯的历史备份（多版本）。同时更新 canonical snapshot/ 供切换源。
pub fn snapshot_current(vault: &Path) -> Result<BackupMeta, String> {
    let uid = current_uid().ok_or("未找到当前登录态，请先登录 WorkBuddy")?;
    // 1) 历史多版本备份
    let ts = chrono_now();
    let hist_dest = vault.join(&uid).join("history").join(&ts);
    std::fs::create_dir_all(&hist_dest).map_err(|e| e.to_string())?;
    let mut meta = take_snapshot_to(&uid, &hist_dest)?;
    meta.ts = ts.clone();
    meta.is_latest = true;

    // 2) 同步 canonical snapshot/ 作为切换源（保持与 history 一致）
    let snap_dest = vault.join(&uid).join("snapshot");
    let _ = std::fs::remove_dir_all(&snap_dest);
    std::fs::create_dir_all(&snap_dest).map_err(|e| e.to_string())?;
    let ls_src = hist_dest.join("local_storage");
    if ls_src.exists() {
        copy_dir_all(&ls_src, &snap_dest.join("local_storage")).map_err(|e| e.to_string())?;
    }
    let db_src = hist_dest.join("workbuddy.db");
    if db_src.exists() {
        let _ = std::fs::copy(&db_src, snap_dest.join("workbuddy.db"));
    }
    let auth_src = hist_dest.join("auth.info");
    if auth_src.exists() {
        std::fs::copy(&auth_src, snap_dest.join("auth.info")).map_err(|e| e.to_string())?;
    }

    cleanup_history(vault, &uid);
    Ok(meta)
}

/// 「登录态默认自动保存」：只要当前账号有登录信息，就保证 vault/<uid>/snapshot/ 存在。
/// 已存在则直接返回（不做重复写盘）；不存在则立即补建（历史备份 + canonical snapshot）。
#[derive(Serialize)]
pub struct EnsureSnapshotResult {
    pub uid: String,
    pub existed: bool,
    pub file_count: usize,
    pub auth_included: bool,
}

pub fn ensure_snapshot(vault: &Path) -> Result<EnsureSnapshotResult, String> {
    let uid = current_uid().ok_or("未找到当前登录态，请先登录 WorkBuddy")?;
    if snapshot_exists(vault, &uid) {
        let snap = vault.join(&uid).join("snapshot");
        let fc = count_files(&snap.join("local_storage"));
        let ai = snap.join("auth.info").exists();
        return Ok(EnsureSnapshotResult { uid, existed: true, file_count: fc, auth_included: ai });
    }
    let m = snapshot_current(vault)?;
    Ok(EnsureSnapshotResult { uid, existed: false, file_count: m.file_count, auth_included: m.auth_included })
}

/// 遍历本机所有已登记账号做一次备份：
///   - 当前登录账号：全新备份（local_storage + 最新登录态），并刷新切换源；
///   - 其他已保存账号：把其 canonical snapshot 归档为一份历史时间点（内容不动）；
///   - 没保存过登录态的账号：跳过并说明原因。
/// 返回每个账号的备份结果。
#[derive(Serialize)]
pub struct BackupAllResult {
    pub uid: String,
    pub ok: bool,
    pub message: String,
    pub file_count: usize,
    pub auth_included: bool,
    pub is_current: bool,
}

pub fn backup_all(vault: &Path) -> Result<Vec<BackupAllResult>, String> {
    let cur = current_uid();
    let accs = list_accounts(vault);
    let mut out = Vec::new();
    for a in accs {
        let uid = a.uid.clone();
        let is_cur = cur.as_deref() == Some(uid.as_str());
        let r = if is_cur {
            match snapshot_current(vault) {
                Ok(m) => BackupAllResult {
                    uid: uid.clone(), ok: true, message: "已备份（含最新登录态）".into(),
                    file_count: m.file_count, auth_included: m.auth_included, is_current: true,
                },
                Err(e) => BackupAllResult {
                    uid, ok: false, message: e, file_count: 0, auth_included: false, is_current: true,
                },
            }
        } else if a.has_snapshot {
            match archive_snapshot(vault, &uid) {
                Ok((fc, ai)) => BackupAllResult {
                    uid: uid.clone(), ok: true, message: "已归档历史快照".into(),
                    file_count: fc, auth_included: ai, is_current: false,
                },
                Err(e) => BackupAllResult {
                    uid, ok: false, message: e, file_count: 0, auth_included: false, is_current: false,
                },
            }
        } else {
            BackupAllResult {
                uid: uid.clone(), ok: false, message: "该账号未保存过登录态，无法备份".into(),
                file_count: 0, auth_included: false, is_current: false,
            }
        };
        out.push(r);
    }
    if out.is_empty() { return Err("本机未发现任何已登记账号".into()); }
    Ok(out)
}

/// 把某账号已有的 canonical snapshot 归档为一份历史时间点备份（只是复制留存，不改动内容）
fn archive_snapshot(vault: &Path, uid: &str) -> Result<(usize, bool), String> {
    let snap = vault.join(uid).join("snapshot");
    if !snap.is_dir() { return Err("无快照可归档".into()); }
    let ts = chrono_now();
    let hist_dest = vault.join(uid).join("history").join(&ts);
    std::fs::create_dir_all(&hist_dest).map_err(|e| e.to_string())?;
    if snap.join("local_storage").exists() {
        copy_dir_all(&snap.join("local_storage"), &hist_dest.join("local_storage")).map_err(|e| e.to_string())?;
    }
    let mut auth_included = false;
    if snap.join("auth.info").exists() {
        std::fs::copy(snap.join("auth.info"), hist_dest.join("auth.info")).map_err(|e| e.to_string())?;
        auth_included = true;
    }
    cleanup_history(vault, uid);
    Ok((count_files(&hist_dest.join("local_storage")), auth_included))
}

/// 列出某个账号（或全部账号）的历史备份元数据。uid=None 时返回全部。
pub fn list_backups(vault: &Path, uid: Option<&str>) -> Vec<BackupMeta> {
    let mut out = Vec::new();
    let user_histories: Vec<PathBuf> = if let Some(u) = uid {
        vec![vault.join(u).join("history")]
    } else {
        match std::fs::read_dir(vault) {
            Ok(rd) => rd.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path().join("history"))
                .collect(),
            Err(_) => Vec::new(),
        }
    };
    for hist in user_histories {
        let u = hist.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or("?").to_string();
        if !hist.exists() { continue; }
        let mut ts: Vec<String> = std::fs::read_dir(&hist).ok().map(|rd| rd.flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .collect()).unwrap_or_default();
        ts.sort_by(|a, b| b.cmp(a)); // 最新的在前
        for (i, t) in ts.iter().enumerate() {
            let d = hist.join(t);
            let meta = BackupMeta {
                uid: u.clone(),
                ts: t.clone(),
                file_count: count_files(&d.join("local_storage")),
                local_files: list_local_files(&d.join("local_storage")),
                auth_included: d.join("auth.info").exists(),
                bytes: dir_size(&d),
                auth_file_name: if d.join("auth.info").exists() { Some("auth.info".to_string()) } else { None },
                is_latest: i == 0,
            };
            out.push(meta);
        }
    }
    out
}

/// 单份备份的详细内容（目录树 + 文件清单）。返回原始快照目录里的结构。
pub fn backup_detail(vault: &Path, uid: &str, ts: &str) -> Result<BackupMeta, String> {
    let d = vault.join(uid).join("history").join(ts);
    if !d.is_dir() {
        return Err(format!("备份不存在: {}/history/{}", uid, ts));
    }
    let all: Vec<BackupMeta> = list_backups(vault, Some(uid));
    let mut meta = BackupMeta {
        uid: uid.to_string(),
        ts: ts.to_string(),
        file_count: count_files(&d.join("local_storage")),
        local_files: list_local_files(&d.join("local_storage")),
        auth_included: d.join("auth.info").exists(),
        bytes: dir_size(&d),
        auth_file_name: if d.join("auth.info").exists() { Some("auth.info".to_string()) } else { None },
        is_latest: all.first().map(|m| m.ts == ts).unwrap_or(false),
    };
    // 附上完整文件树（每文件一行 相对路径）
    let mut tree = Vec::new();
    collect_tree(&d, "", &mut tree);
    meta.local_files = tree;
    Ok(meta)
}

fn collect_tree(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let rel = if prefix.is_empty() { name.clone() } else { format!("{}/{}", prefix, name) };
            if let Ok(m) = e.metadata() {
                if m.is_dir() {
                    out.push(format!("[DIR] {}", rel));
                    collect_tree(&e.path(), &rel, out);
                } else {
                    out.push(format!("{} ({}B)", rel, m.len()));
                }
            }
        }
    }
}

fn chrono_now() -> String {
    // 毫秒级时间戳，避免同一秒内多次备份/归档目录名撞车
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    ms.to_string()
}

/// WorkBuddy 会话库路径（macOS / Linux 通用）：~/.workbuddy/workbuddy.db
fn workbuddy_db_path() -> Option<PathBuf> {
    let p = home().join(".workbuddy").join("workbuddy.db");
    if p.exists() { Some(p) } else { None }
}

/// 为「已在登录态 allAccounts 中、但尚无 vault 快照」的账号即时生成一份**占位快照**，
/// 用于账号列表展示（昵称/uid）。⚠️ 生成的 auth.info **不含 auth.accessToken**——
/// 实测官方登录态文件的 token 只存在顶层 `auth` 字段（当前登录账号），allAccounts 条目
/// 无 token。因此占位快照**不能用于切换**（切换必须有该账号的真实 token 快照，
/// 见 switch_auth_to 的校验）。切换前若 vault/history 里存有该账号真实登录态，会优先恢复。
///
/// ⚠️ 关键修复（防止昵称串号）：生成快照时**只用官方 allAccounts 里匹配 uid 的真实条目**，
/// 绝不回退到当前 `account`（那是另一个账号，用它造目标账号快照会把昵称也写成别的账号的）。
/// 若 allAccounts 里找不到该 uid（官方压缩场景），仅保留 uid、昵称留空，由 list_accounts
/// 用其他来源兜底，绝不编造错误昵称。
fn materialize_snapshot_for(vault: &Path, uid: &str) -> Result<(), String> {
    let Some(af) = auth_file() else {
        return Err("未找到当前登录态文件，无法生成快照".into());
    };
    let s = std::fs::read_to_string(&af).map_err(|e| format!("读取登录态失败: {e}"))?;
    let v: Value = serde_json::from_str(&s).map_err(|e| format!("登录态解析失败: {e}"))?;

    // 优先取官方 allAccounts 里匹配 uid 的真实条目（含真实昵称）
    let entry = v.get("allAccounts").and_then(|a| a.as_array())
        .and_then(|arr| arr.iter().find(|x| x.get("uid").and_then(|u| u.as_str()) == Some(uid)))
        .cloned();

    // 仅当 allAccounts 里确实没有该 uid 时，才用最小占位（uid 真实，昵称留空，不编造假昵称）
    let entry = match entry {
        Some(e) => e,
        None => {
            // 再试 account（仅当 account.uid 正好等于目标 uid）
            if v.get("account").and_then(|a| a.get("uid")).and_then(|u| u.as_str()) == Some(uid) {
                v.get("account").cloned().unwrap_or_else(|| json!({ "uid": uid }))
            } else {
                json!({ "uid": uid })
            }
        }
    };

    let snap = vault.join(uid).join("snapshot");
    std::fs::create_dir_all(&snap).map_err(|e| e.to_string())?;
    // 保留官方 allAccounts 里所有真实账号条目（带真实昵称），不丢、不串
    let all_accounts = v.get("allAccounts").and_then(|a| a.as_array()).cloned()
        .unwrap_or_else(|| vec![entry.clone()]);
    let full = json!({
        "account": entry.clone(),
        "accounts": [entry.clone()],
        "allAccounts": all_accounts,
    });
    std::fs::write(snap.join("auth.info"), serde_json::to_string_pretty(&full).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let ls = local_storage_dir();
    if ls.exists() {
        copy_dir_all(&ls, &snap.join("local_storage")).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 从某账号的历史备份（vault/<uid>/history/<ts>/）中找最新一份含 auth.info 的备份，
/// 用于把「仅 materialize 的占位快照」升级为「含真实 token 的登录态快照」。
/// 历史备份由 snapshot_current / switch_account 在账号曾是当前账号时留存（真实 auth 拷贝）。
fn latest_hist_auth(vault: &Path, uid: &str) -> Option<PathBuf> {
    let hist = vault.join(uid).join("history");
    let mut ts: Vec<String> = std::fs::read_dir(&hist).ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect();
    // 13 位毫秒时间戳，字符串倒序即最新在前
    ts.sort_by(|a, b| b.cmp(a));
    for t in ts {
        let p = hist.join(t).join("auth.info");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// 切换账号：切换前先【完整备份】当前账号，再换入目标登录态并合并目标登记表。
///
/// 四个动作（用户核心诉求：切账号时，原账号的会话/自动化跟着搬到目标账号名下，
/// 在目标登录态下能直接看到并继续原对话，用目标账号的额度/积分；且切换前自动备份）：
///   1. 切换前自动完整备份当前账号（local_storage 整目录 + 登录态 + workbuddy.db）到
///      vault/<cid>/history/<ts>/（多版本，可一键回滚），并刷新 canonical snapshot/ 作切回源；
///   2. 合并式切换登录态（改 workbuddy-desktop.info 的 account/accounts/allAccounts 指向目标，
///      保留所有账号登记），使额度/积分随目标 uid 走；
///   3. 合并目标账号登记表(entry_*.info)进当前 local_storage（仅目标 uid 那条，不破坏其他账号）；
///   4. 将会话库 workbuddy.db 里源账号的会话/自动化归属改写为目标 uid（"搬"语义，源视角下不可见，
///      回滚用 history/<ts>/workbuddy.db.bak）。
///
/// ⚠️ 与旧设计相反：旧版"按 uid 隔离、绝不碰会话库"会导致切账号后看不到原对话；
/// 现按用户需求主动搬迁会话归属，使"切账号会话跟着过去"成为现实。
pub fn switch_account(vault: &Path, uid: &str) -> Result<SwitchResult, String> {
    // 切换前若 WorkBuddy 在运行，先优雅退出（避免退出时回写覆盖 local_storage/登录态）
    if is_workbuddy_running() {
        quit_workbuddy()?;
    }

    // 目标账号快照缺失：先即时生成占位快照（materialize），若 history 里有该账号
    // 真实登录态备份，则用它覆盖占位快照的 auth.info（含真实 token，切换才有效）。
    let src = vault.join(uid).join("snapshot");
    if !src.exists() {
        materialize_snapshot_for(vault, uid)?;
        if let Some(hist_auth) = latest_hist_auth(vault, uid) {
            let _ = std::fs::copy(&hist_auth, src.join("auth.info"));
        }
    }

    // 切换前自动【完整备份】当前账号：local_storage 整目录 + 登录态文件，
    // 写入 vault/<cid>/history/<ts>/（多版本，可一键回滚），并同步刷新 canonical snapshot/
    // 作为"切回源"。用户明确要求切换前完整备份，故此处用 take_snapshot_to 而非轻量版。
    // canonical snapshot/ 必须在每次切换时同步刷新，否则切到别处后原账号没有
    // 登录态源（vault/<uid>/snapshot），永远切不回来。这正是"一键切回"的关键。
    let cur_uid = current_uid();
    if let Some(cid) = &cur_uid {
        if cid != uid {
            let ts = chrono_now();
            let hist_dest = vault.join(cid).join("history").join(&ts);
            std::fs::create_dir_all(&hist_dest).map_err(|e| e.to_string())?;
            let _ = take_snapshot_to(cid, &hist_dest);
            // 额外备份会话库 workbuddy.db（搬迁前兜底，便于一键回滚会话归属）。
            // 备份失败必须中止——接下来要改写该库，没有备份就没有回滚点。
            if let Some(db) = workbuddy_db_path() {
                std::fs::copy(&db, hist_dest.join("workbuddy.db.bak"))
                    .map_err(|e| format!("备份会话库失败（已中止切换，未改动任何数据）: {e}"))?;
            }
            // 同步刷新 canonical snapshot/（含完整 local_storage + workbuddy.db + auth.info）
            let snap_dest = vault.join(cid).join("snapshot");
            let _ = std::fs::remove_dir_all(&snap_dest);
            if std::fs::create_dir_all(&snap_dest).is_ok() {
                let ls_src = hist_dest.join("local_storage");
                if ls_src.exists() {
                    let _ = copy_dir_all(&ls_src, &snap_dest.join("local_storage"));
                }
                let db_src = hist_dest.join("workbuddy.db");
                if db_src.exists() {
                    let _ = std::fs::copy(&db_src, snap_dest.join("workbuddy.db"));
                }
                let auth_src = hist_dest.join("auth.info");
                if auth_src.exists() {
                    let _ = std::fs::copy(&auth_src, snap_dest.join("auth.info"));
                }
            }
            cleanup_history(vault, cid);
        }
    }

    // 写入目标登录态：合并式切换（保留 allAccounts 所有账号登记，只把 account 指向目标账号）
    let src_auth = src.join("auth.info");
    if !src_auth.exists() {
        return Err("目标账号没有可用的登录态快照（auth.info 缺失），已中止切换。请先在官方客户端登录该账号，再点「保存当前登录态」".into());
    }
    switch_auth_to(&src_auth, uid)?;

    // 合并目标账号的登记表(entry_*.info)进当前 local_storage：让"当前账号身份"真正换成目标账号
    // （用它的积分/额度），同时保留其他账号的登记表条目（合并而非覆盖）。
    if let Some(cid) = &cur_uid {
        if cid != uid {
            merge_local_storage_entries(&src.join("local_storage"), uid);
        }
    }

    // ===== 会话归属搬迁（用户核心诉求：切账号，会话跟着过去）=====
    // WorkBuddy 以登录态 token 的 `sub`(=uid) 查询 workbuddy.db 的 sessions.user_id 来显示对话。
    // 切换登录态后，WorkBuddy 只会显示「目标 uid」名下的会话——源账号的对话就看不见了。
    // 因此此处主动把源账号在会话库里的全部归属改写为目标 uid，使源账号的对话/自动化
    // 在目标登录态下直接可见并可持续（用目标账号的额度/积分）。
    //
    // 关键修复：v0.5.0 之前用系统 sqlite3 CLI 执行搬迁，Windows 通常没有 sqlite3.exe 导致
    // "调用 sqlite3 失败: program not found"。现改用 Rust 内嵌 rusqlite（bundled 特征），
    // 零外部依赖，Windows/macOS 均可用。
    //
    // ⚠️ 语义（用户已确认）：这是「搬」而非「复制」——搬迁后源账号视角下这些对话不再可见
    // （切回源账号时也看不到）。回滚方式：从 vault/<cid>/history/<ts>/workbuddy.db.bak 还原。
    let mut migrated_sessions: i64 = 0;
    let mut migrated_autos: i64 = 0;
    if let Some(cid) = &cur_uid {
        if cid != uid {
            match migrate_sessions_user_id(cid, uid) {
                Ok((s, a)) => { migrated_sessions = s; migrated_autos = a; }
                Err(e) => {
                    return Err(format!(
                        "会话搬迁失败（已中止切换，登录态未改动）: {e}"
                    ));
                }
            }
        }
    }

    // 会话置顶（Pinned）状态迁移：把渲染层 localStorage 里 `u:<旧uid>` 键复制为 `u:<新uid>`。
    // best-effort：失败时仅记录警告，不阻断账号切换（置顶丢失只是体验问题，可手动重新置顶）。
    let mut pin_warn: Option<String> = None;
    if let Some(cid) = &cur_uid {
        if cid != uid {
            if let Err(e) = migrate_renderer_pin_state(cid, uid) {
                pin_warn = Some(e);
            }
        }
    }

    // 切换后由前端在确认 WorkBuddy 无任务运行后，调用 restart_workbuddy 重启 WorkBuddy 生效
    // （不是重启中枢——中枢重启会丢当前界面状态，且 WorkBuddy 自身需以新登录态重启）。
    Ok(SwitchResult {
        restart_required: true,
        restart_workbuddy: true,
        aggregate_id: String::new(),
        message: {
            let mut m = format!(
                "已切换到目标账号（{}）：搬迁会话 {} 条、自动化 {} 条，准备重启 WorkBuddy 生效",
                uid, migrated_sessions, migrated_autos
            );
            if let Some(w) = &pin_warn {
                m.push_str(&format!("（⚠️ 会话置顶状态迁移未完成: {w}）"));
            }
            m
        },
        uid: uid.to_string(),
    })
}

/// 用 Rust 内嵌 SQLite（rusqlite，bundled 特征）把 workbuddy.db 中属于 from_uid 的
/// 会话/自动化归属改写到 to_uid。避免 WorkBuddy 在 Windows 上自行调用系统 `sqlite3` CLI
/// 导致"program not found"失败（这正是 v0.5.0 用户报告的根因）。
///
/// 涉及表（均按 user_id / owner_user_id 标记归属，无外键、无触发器，可安全改写）：
///   - sessions.user_id                         会话列表（对话）——改写归属
///   - automations.owner_user_id                定时任务归属——改写归属
///   - automation_delivery_outbox.owner_user_id 自动化投递记录归属——改写归属
///   - memory.user_id                           用户记忆库归属——改写归属
///
/// 迁移前会备份原 db 到同目录 `.workbuddy.db.migrate-backup.<from>.<ts>`，失败不破坏原文件。
/// 返回 (搬迁的会话数, 搬迁的自动化数)；出错时返回 Err（由调用方决定是否中止切换）。
pub fn migrate_sessions_user_id(from_uid: &str, to_uid: &str) -> Result<(i64, i64), String> {
    let Some(db) = workbuddy_db_path() else {
        // 本机没有会话库（极少见），视为无需搬迁，不报错中断切换
        return Ok((0, 0));
    };

    // 先备份（带时间戳，避免覆盖）
    let ts = chrono_now();
    let backup = db.with_extension(format!("migrate-backup.{}.{}", from_uid, ts));
    std::fs::copy(&db, &backup).map_err(|e| format!("备份 workbuddy.db 失败: {e}"))?;

    let conn = Connection::open(&db).map_err(|e| format!("打开 workbuddy.db 失败: {e}"))?;

    // 会话归属改写（核心诉求：切账号，会话跟着过去）
    let s_changed = conn
        .execute(
            "UPDATE sessions SET user_id = ?1 WHERE user_id = ?2",
            [to_uid, from_uid],
        )
        .map_err(|e| format!("更新 sessions.user_id 失败: {e}"))?;

    // 自动化归属改写（失败不致命，仍尝试继续）
    let a_changed = conn
        .execute(
            "UPDATE automations SET owner_user_id = ?1 WHERE owner_user_id = ?2",
            [to_uid, from_uid],
        )
        .unwrap_or(0);

    // 自动化投递记录归属改写（不存在该表则忽略）
    let _ = conn.execute(
        "UPDATE automation_delivery_outbox SET owner_user_id = ?1 WHERE owner_user_id = ?2",
        [to_uid, from_uid],
    );

    // 用户记忆库归属改写（不存在该表则忽略）
    let _ = conn.execute(
        "UPDATE memory SET user_id = ?1 WHERE user_id = ?2",
        [to_uid, from_uid],
    );

    Ok((s_changed as i64, a_changed as i64))
}

/// 迁移渲染层「会话置顶」状态（修复 v0.5.1 用户反馈：切账号后置顶会话不再置顶）。
///
/// 背景：WorkBuddy 的会话置顶（置顶/Pinned）状态**不**存在 workbuddy.db（sessions 表没有
/// pin 列），而是存在渲染层 Electron localStorage（leveldb），路径：
///   `~/.workbuddy/app/session/Local Storage/leveldb`
/// 键名形如 `wb:conversation-list:expanded-state:u:<uid>`，值 JSON 含 `{"pinned":true,...}`，
/// 且 `<uid>` 与 `sessions.user_id` 同 ID 空间。
///
/// 切换账号时 `migrate_sessions_user_id` 只改写了 workbuddy.db 的 user_id，没动这个
/// localStorage → 置顶映射仍挂在旧 `u:<from_uid>` 键下，新登录读 `u:<to_uid>`（空）→ 置顶丢失。
///
/// 本函数把**所有**以 `:u:<from_uid>` 结尾的渲染层键复制为 `:u:<to_uid>` 并删除旧键。
/// 必须在 WorkBuddy 已退出（leveldb 锁释放）后调用（switch_account 开头已 quit_workbuddy）。
///
/// ⚠️ 设计为 best-effort：任何错误返回 Err(String) 作为【警告】，由调用方决定是否中止切换
///    （默认不中止——置顶丢失只是体验问题，不应阻断账号切换）。
pub fn migrate_renderer_pin_state(from_uid: &str, to_uid: &str) -> Result<(), String> {
    let suffix_from = format!(":u:{from_uid}");
    let suffix_to = format!(":u:{to_uid}");

    let mut dir = workbuddy_home();
    dir.push("app");
    dir.push("session");
    dir.push("Local Storage");
    dir.push("leveldb");
    if !dir.exists() {
        // 渲染层存储不存在（极少见，如从未启动过会话界面），视为无需迁移
        return Ok(());
    }

    // 打开 leveldb（只读校验 + 可写）。若被占用（理论上不会，因已 quit），给少量重试窗口。
    let db: Database<Vec<u8>> = open_leveldb_with_retry(&dir)?;

    // 遍历全部键，收集需要改名的（键以 :u:<from_uid> 结尾）
    let read_opts = ReadOptions::new();
    let iter = db.iter(&read_opts);
    iter.start();
    let mut to_rename: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (key, val) in iter {
        if let Ok(s) = std::str::from_utf8(&key) {
            if s.ends_with(&suffix_from) {
                // 复制键体，仅替换末尾的 uid 后缀
                let base_len = key.len().saturating_sub(suffix_from.len());
                let mut new_key = key[..base_len].to_vec();
                new_key.extend_from_slice(suffix_to.as_bytes());
                to_rename.push((new_key, val));
            }
        }
    }

    if to_rename.is_empty() {
        return Ok(());
    }

    // 写新键 + 删旧键（best-effort：单条失败记日志但继续）
    let mut done = 0usize;
    for (new_key, val) in &to_rename {
        // 旧键 = 新键去掉 suffix_to 再拼回 suffix_from
        let base_len = new_key.len().saturating_sub(suffix_to.len());
        let mut old_key = new_key[..base_len].to_vec();
        old_key.extend_from_slice(suffix_from.as_bytes());
        let write_opts = WriteOptions::new();
        if db.put(write_opts, new_key, val).is_ok() {
            let del_opts = WriteOptions::new();
            let _ = db.delete(del_opts, &old_key);
            done += 1;
        }
    }

    if done == 0 {
        return Err(format!(
            "渲染层置顶键已找到 {} 个但写入失败（leveldb 可能无法写入）",
            to_rename.len()
        ));
    }
    Ok(())
}

/// 带重试地打开渲染层 leveldb（应对 WorkBuddy 刚退出、文件锁释放的极小时间窗）。
fn open_leveldb_with_retry(dir: &Path) -> Result<Database<Vec<u8>>, String> {
    let mut last_err = String::new();
    for _ in 0..15 {
        let mut opts = Options::new();
        opts.create_if_missing = false;
        match Database::open(dir, opts) {
            Ok(db) => return Ok(db),
            Err(e) => {
                last_err = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }
    Err(format!("打开渲染层 localStorage 失败（已重试）: {last_err}"))
}

/// 把当前登录态切换为快照 auth（目标账号）。
///
/// ⚠️ 关键约束（修复「切换后还是原账号 / 昵称被覆盖」）：
///   登录态文件里的 **uid 必须与 accessToken.sub 完全一致**（WorkBuddy 以 token.sub 认人）。
///   因此本函数【绝不改写 uid】，只把 account/accounts/allAccounts 指向目标账号【真实 uid】，
///   并保留 allAccounts 中其他账号的【真实 uid + 真实昵称】条目（合并而非覆盖），
///   使两个账号在登录态里都真实存在、可互相切回，且昵称不被污染。
fn switch_auth_to(src_auth: &Path, uid: &str) -> Result<(), String> {
    let src_s = std::fs::read_to_string(src_auth).map_err(|e| format!("读取目标登录态失败: {e}"))?;
    let target: serde_json::Value =
        serde_json::from_str(&src_s).map_err(|e| format!("目标登录态 JSON 解析失败: {e}"))?;

    // ⚠️ 关键校验：目标快照必须含真实 auth.accessToken。
    // materialize 生成的占位快照无 token（实测 allAccounts 条目不含 token），
    // 若放行，写回登录态后 WorkBuddy 将以无 token 状态启动 → 登录态失效。
    let has_token = target
        .get("auth")
        .and_then(|a| a.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|t| !t.is_empty())
        .unwrap_or(false);
    if !has_token {
        return Err("目标账号没有保存过登录态 token（accessToken），无法切换。请先在官方客户端登录该账号，再在 Hub 点「保存当前登录态」".into());
    }

    // 目标账号真实身份（快照里的 account 条目，uid 保持真实值，不改写）
    let acc_entry = target.get("account")
        .cloned()
        .unwrap_or_else(|| json!({ "uid": uid }));

    // 目标账号真实 uid（来自快照，不修正、不聚合）
    let target_uid = target.get("account").and_then(|a| a.get("uid")).and_then(|u| u.as_str())
        .unwrap_or(uid).to_string();

    // 目标账号在快照 allAccounts 中的完整条目（带真实昵称等）
    let target_entry = target.get("allAccounts").and_then(|a| a.as_array())
        .and_then(|arr| arr.iter().find(|x| x.get("uid").and_then(|u| u.as_str()) == Some(target_uid.as_str())))
        .cloned()
        .unwrap_or_else(|| acc_entry.clone());

    // 读取【当前登录态】里已经登记过的所有账号条目（保留其他账号的真实 uid + 昵称）
    let mut all: Vec<serde_json::Value> = Vec::new();
    if let Some(af) = auth_file() {
        if let Ok(cur_s) = std::fs::read_to_string(&af) {
            if let Ok(cur_v) = serde_json::from_str::<serde_json::Value>(&cur_s) {
                if let Some(cur_arr) = cur_v.get("allAccounts").and_then(|x| x.as_array()) {
                    all = cur_arr.clone();
                }
            }
        }
    }
    // 合并：按真实 uid 去重，目标账号用快照里的最新真实条目覆盖，其余账号原样保留
    all.retain(|x| x.get("uid").and_then(|u| u.as_str()) != Some(target_uid.as_str()));
    all.push(target_entry.clone());

    // 构造写回的登录态：以目标快照为骨架（含目标真实 token），仅 allAccounts 合并了其他账号
    let mut out = target.clone();
    out["allAccounts"] = Value::Array(all);
    out["account"] = acc_entry.clone();
    // accounts 数组：放目标账号真实条目（单元素；WorkBuddy 多账号切换实际看 allAccounts）
    out["accounts"] = Value::Array(vec![target_entry.clone()]);

    // 写回当前登录态文件（uid 保持真实，token 为目标真实 token）
    if let Some(af) = auth_file() {
        let s = serde_json::to_string_pretty(&out).map_err(|e| format!("序列化失败: {e}"))?;
        std::fs::write(&af, s).map_err(|e| format!("写入登录态失败: {e}"))?;
        Ok(())
    } else {
        Err("未找到当前登录态文件".into())
    }
}

/// 合并目标账号的登记表(entry_*.info)进当前 local_storage。
///
/// 行为（保守合并，绝不破坏其他账号）：
///   - 遍历目标快照 `snapshot/local_storage` 下的 `entry_*.info`；
///   - 仅挑出 `userId == uid`（目标账号）的条目复制进当前 local_storage（覆盖同名 hash 文件，新增则创建）；
///   - 其他账号的 entry 不动；若目标快照里没有该 uid 的 entry（常见，因为快照是共享登记表），
///     则跳过——交给 WorkBuddy 重启后以新登录态自建当前账号 entry。
///
/// 会话/记忆(workbuddy.db、sessions、memory)是全局共享的，本函数不触碰，切换登录态后自然可见，
/// 这正是"用别的账号积分继续原对话"的实现路径（额度随登录态 uid 走，对话随共享库走）。
fn merge_local_storage_entries(src_ls: &Path, uid: &str) {
    let target_uid = uid.to_string();
    let dst = local_storage_dir();
    if !src_ls.exists() { return; }
    if let Ok(rd) = std::fs::read_dir(src_ls) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("info") { continue; }
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            // 只处理 entry_ 开头（账号登记表），跳过 wb_entry_ 等
            if !name.starts_with("entry_") { continue; }
            let Ok(s) = std::fs::read_to_string(&p) else { continue; };
            let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&s) else { continue; };
            let mut is_target = false;
            for item in &arr {
                if item.get("userId").and_then(|x| x.as_str()) == Some(target_uid.as_str()) {
                    is_target = true;
                    break;
                }
            }
            if is_target {
                let _ = std::fs::create_dir_all(&dst);
                let _ = std::fs::copy(&p, dst.join(&name));
            }
        }
    }
}

/// WorkBuddy 主进程可执行文件路径（macOS）。
/// ⚠️ 必须用精确路径匹配，不能用 `pgrep -f "WorkBuddy"`——本工具自身位于
/// `/Applications/WorkBuddy Account Hub.app`，路径含 "WorkBuddy"，模糊匹配会把
/// 自己（Hub）也当成 WorkBuddy 杀掉，导致切换写入登录态/重启流程中断（切换不过去的根因）。
const WB_EXE: &str = "/Applications/WorkBuddy.app/Contents/MacOS/Electron";

/// WorkBuddy 是否正在运行（精确匹配主进程，排除 Hub 自身）
pub fn is_workbuddy_running() -> bool {
    if cfg!(target_os = "macos") {
        Command::new("pgrep").args(["-f", WB_EXE]).output().map(|o| o.status.success()).unwrap_or(false)
    } else if cfg!(target_os = "windows") {
        Command::new("tasklist").args(["/FI", "IMAGENAME eq WorkBuddy.exe"]).output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("WorkBuddy.exe"))
            .unwrap_or(false)
    } else {
        false
    }
}

/// 优雅退出 WorkBuddy 并等待其完全退出（防止退出时回写覆盖登录态）。
/// macOS: 先 osascript 优雅退出，轮询等待退出；超时再按精确路径 pkill 兜底。
pub fn quit_workbuddy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let _ = Command::new("osascript")
            .args(["-e", "tell application \"WorkBuddy\" to quit"])
            .output();
        // 轮询等待完全退出（最多 ~10s）
        for _ in 0..34 {
            if !is_workbuddy_running() { return Ok(()); }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        // 兜底：只按精确路径杀 WorkBuddy 主进程，绝不模糊匹配 Hub
        let _ = Command::new("pkill").args(["-f", WB_EXE]).output();
        std::thread::sleep(std::time::Duration::from_millis(800));
        if !is_workbuddy_running() {
            Ok(())
        } else {
            Err("WorkBuddy 未能完全退出，已中止切换（请手动关闭后重试）".into())
        }
    } else if cfg!(target_os = "windows") {
        let _ = Command::new("taskkill").args(["/IM", "WorkBuddy.exe", "/F"]).output();
        Ok(())
    } else {
        Err("当前平台不支持退出 WorkBuddy".into())
    }
}

/// 启动 WorkBuddy（切换后由用户点击「重启」触发）
/// 定位 WorkBuddy 主程序可执行文件（用于切换后启动）。
///   - macOS:   /Applications/WorkBuddy.app/Contents/MacOS/Electron（精确匹配，避免误杀 Hub 自身）
///   - Windows: %LOCALAPPDATA%/Programs/WorkBuddy/WorkBuddy.exe（本机实际安装位置），
///             候选回退到 ProgramFiles / ProgramFiles(x86) 下的 WorkBuddy/WorkBuddy.exe。
pub fn workbuddy_exe() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let p = PathBuf::from(WB_EXE);
        if p.exists() { Some(p) } else { None }
    } else if cfg!(target_os = "windows") {
        let local = PathBuf::from(
            std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
                std::env::var("USERPROFILE")
                    .map(|u| PathBuf::from(u).join("AppData").join("Local").to_string_lossy().into_owned())
                    .unwrap_or_else(|_| ".".into())
            }),
        );
        let program_files = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        let program_files_x86 =
            std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
        let candidates = vec![
            local.join("Programs").join("WorkBuddy").join("WorkBuddy.exe"),
            PathBuf::from(program_files).join("WorkBuddy").join("WorkBuddy.exe"),
            PathBuf::from(program_files_x86).join("WorkBuddy").join("WorkBuddy.exe"),
        ];
        candidates.into_iter().find(|p| p.exists())
    } else {
        None
    }
}

pub fn launch_workbuddy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        // `open -a` 异步启动，不阻塞
        Command::new("open").args(["-a", "WorkBuddy"]).spawn().map(|_| ()).map_err(|e| e.to_string())
    } else if cfg!(target_os = "windows") {
        // ⚠️ 关键修复：必须用 spawn() 异步启动，绝不能 .output()/.status() —— 后者会
        // 同步等待 WorkBuddy 子进程退出，而 WorkBuddy 是常驻 GUI 不会退出，导致后端
        // 命令卡死、整个中枢 IPC 无响应，表现为「点切换后 WorkBuddy 没启动」。
        //
        // 方案：优先用精确 exe 路径 + `cmd /c start "" "exe"` 启动（start 会立即返回，
        // 真正把 WB 放到独立进程树里，不受本进程退出影响）；找不到路径时退回
        // `cmd /c start WorkBuddy` 走系统关联。
        if let Some(exe) = workbuddy_exe() {
            let exe_str = exe.to_string_lossy().to_string();
            Command::new("cmd")
                .args(["/c", "start", "", &exe_str])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("启动 WorkBuddy 失败（{}）: {}", exe_str, e))
        } else {
            // 兜底：靠系统关联 / PATH 启动
            Command::new("cmd")
                .args(["/c", "start", "", "WorkBuddy"])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("启动 WorkBuddy 失败（未找到安装路径）: {}", e))
        }
    } else {
        Err("当前平台不支持启动 WorkBuddy".into())
    }
}
