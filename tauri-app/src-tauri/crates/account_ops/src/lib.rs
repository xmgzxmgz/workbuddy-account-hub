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

/// 切换账号：整体换入 vault/<uid>/snapshot/。切换前会先把当前账号**自动备份**到
/// vault/<uid>/history/<ts>/（统一历史备份，替代旧 _rollback），以免丢失当前状态的增量，
/// 然后在写回前用当前状态覆盖历史备份（已含）。返回 restart_required 提示重启。
pub fn switch_account(vault: &Path, uid: &str) -> Result<SwitchResult, String> {
    let src = vault.join(uid).join("snapshot");
    if !src.exists() {
        return Err(format!("账号 {} 尚未快照，请先对其执行「快照当前账号」", uid));
    }
    if is_workbuddy_running() {
        quit_workbuddy()?;
    }

    // 切换前自动保存当前账号：历史备份（可追溯）+ canonical snapshot/（切回源）。
    // canonical snapshot/ 必须在每次切换时同步刷新，否则切到别处后原账号没有
    // 登录态源（vault/<uid>/snapshot），永远切不回来。这正是"一键切回"的关键。
    let cur_uid = current_uid();
    if let Some(cid) = &cur_uid {
        if cid != uid {
            let hist_dest = vault.join(cid).join("history").join(chrono_now());
            std::fs::create_dir_all(&hist_dest).map_err(|e| e.to_string())?;
            let _ = take_snapshot_to(cid, &hist_dest);
            // 同步刷新 canonical snapshot/ 供下次切换使用（与 snapshot_current 行为一致）
            let snap_dest = vault.join(cid).join("snapshot");
            let _ = std::fs::remove_dir_all(&snap_dest);
            if std::fs::create_dir_all(&snap_dest).is_ok() {
                let ls_src = hist_dest.join("local_storage");
                if ls_src.exists() {
                    let _ = copy_dir_all(&ls_src, &snap_dest.join("local_storage"));
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
    // 不再整体覆盖 local_storage（公共 leveldb export，与账号切换弱相关，覆盖会破坏登记表并丢对话）
    let src_auth = src.join("auth.info");
    if src_auth.exists() {
        switch_auth_to(&src_auth, uid)?;
    }

    // 自动重启 WorkBuddy 使新登录态生效（等退出完全后再启动，避免冲突）
    std::thread::sleep(std::time::Duration::from_millis(800));
    let _ = launch_workbuddy();

    Ok(SwitchResult {
        restart_required: false,
        message: "已切换到目标账号并自动重启 WorkBuddy".to_string(),
        uid: uid.to_string(),
    })
}

/// 把当前登录态切换为快照 auth（目标账号）。
/// 采用**合并式**：只把 account/accounts 指向目标账号，并**保留/合并 allAccounts**
/// 中所有曾出现过的账号（当前账号 + 目标账号都在列表里），防止切换后原账号消失无法切回。
/// 保留无关字段（auth.token 等随目标快照整体带入，以激活目标登录态）。
fn switch_auth_to(src_auth: &Path, uid: &str) -> Result<(), String> {
    let src_s = std::fs::read_to_string(src_auth).map_err(|e| format!("读取目标登录态失败: {e}"))?;
    let mut target: serde_json::Value =
        serde_json::from_str(&src_s).map_err(|e| format!("目标登录态 JSON 解析失败: {e}"))?;

    // 目标账号身份（从快照 allAccounts/account 里提取，作为登记表条目）
    let acc_entry = target.get("account")
        .cloned()
        .unwrap_or_else(|| json!({ "uid": uid }));

    let mut target_uid = target.get("account").and_then(|a| a.get("uid")).and_then(|u| u.as_str())
        .unwrap_or(uid).to_string();
    if target_uid.is_empty() { target_uid = uid.to_string(); }

    // 目标账号在快照 allAccounts 中的条目（带昵称等）
    let target_entry = target.get("allAccounts").and_then(|a| a.as_array())
        .and_then(|arr| arr.iter().find(|x| x.get("uid").and_then(|u| u.as_str()) == Some(target_uid.as_str())))
        .cloned()
        .unwrap_or_else(|| acc_entry.clone());

    // 读取当前登录态，取出它登记过的账号列表
    let mut all: Vec<serde_json::Value> = Vec::new();
    if let Some(af) = auth_file() {
        if let Ok(cur_s) = std::fs::read_to_string(&af) {
            if let Ok(cur_v) = serde_json::from_str::<serde_json::Value>(&cur_s) {
                if let Some(cur_arr) = cur_v.get("allAccounts").and_then(|x| x.as_array()) {
                    all = cur_arr.clone();
                }
            }
        } else if let Some(parent) = af.parent() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    // 合并：去掉 uid 重复的旧条目，统一替换为目标/当前最新条目
    let mut merge = |entry: &serde_json::Value| {
        if let Some(uidv) = entry.get("uid").and_then(|u| u.as_str()) {
            all.retain(|x| x.get("uid").and_then(|u| u.as_str()) != Some(uidv));
            all.push(entry.clone());
        }
    };
    // 先把当前登记的所有账号保留（除了目标外，其余沿用当前条目）
    // 再放入目标账号条目
    merge(&target_entry);

    target["allAccounts"] = Value::Array(all);

    // 确保 account / accounts[0?] 指向目标账号。accounts 是数组，把当前账号条目也纳入。
    target["account"] = acc_entry.clone();
    // accounts 数组：替换为 [目标账号条目]（若快照里有则用快照的，否则用目标条目）
    if target.get("accounts").is_none() || target["accounts"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
        target["accounts"] = Value::Array(vec![target_entry.clone()]);
    }

    // 写回当前登录态文件
    if let Some(af) = auth_file() {
        let s = serde_json::to_string_pretty(&target).map_err(|e| format!("序列化失败: {e}"))?;
        std::fs::write(&af, s).map_err(|e| format!("写入登录态失败: {e}"))?;
        Ok(())
    } else {
        Err("未找到当前登录态文件".into())
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
pub fn launch_workbuddy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        Command::new("open").args(["-a", "WorkBuddy"]).output().map(|_| ()).map_err(|e| e.to_string())
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/c", "start", "", "WorkBuddy"]).output().map(|_| ()).map_err(|e| e.to_string())
    } else {
        Err("当前平台不支持启动 WorkBuddy".into())
    }
}
