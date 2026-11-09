// WorkBuddy 账户中枢 — Tauri 2 桌面端（纯 Rust 实现，无 Node 依赖）
//
// 隐藏 Windows 控制台窗口（GUI 程序默认若不加此属性会弹黑框）
#![cfg_attr(windows, windows_subsystem = "windows")]
//

// 职责：
//   - 账号枚举 / 快照 / 切换：account_ops（保留每个账号登录态，参考 CC Switch）
//   - 仪表盘数据（额度/签到/记忆/JWT/环境/本地目录）：wb_api（直连官方接口）
//
// 切换设计：switch_account 会先退出 WorkBuddy，整体换入目标账号快照，
// 返回 restart_required=true，由前端提示「重启 WorkBuddy 使登录态生效」。

use account_ops as ops;
use serde_json::{json, Value};
use wb_api as api;
#[cfg(windows)]
use std::os::windows::process::CommandExt; // 提供 creation_flags（隐藏子进程控制台窗口）
use std::sync::atomic::{AtomicUsize, Ordering};

// #1 请求级轮转：跨调用 round-robin 起始位（进程内原子计数，重启重置）
static ROTATE_INDEX: AtomicUsize = AtomicUsize::new(0);

// ===== Batch D：单实例锁（#25）+ 签到错误分类冷却（#16） =====

/// 单实例锁路径（与安装目录同级）
fn instance_lock_path() -> std::path::PathBuf {
    if cfg!(windows) {
        std::path::Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default())
            .join("WorkBuddy Account Hub").join(".instance.lock")
    } else {
        std::path::Path::new(&std::env::var("HOME").unwrap_or_default())
            .join("Library").join("Application Support").join("WorkBuddy Account Hub").join(".instance.lock")
    }
}

/// 判断某 PID 是否仍存活（跨平台命令探测；探测失败按「不存活」处理）
fn pid_alive(pid: u32) -> bool {
    if pid == 0 { return false; }
    #[cfg(windows)] {
        let mut c = std::process::Command::new("tasklist");
        c.args(["/FI", &format!("PID eq {}", pid), "/NH"]);
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW：避免从 GUI 父进程 spawn 时弹出控制台窗口
        if let Ok(o) = c.output() {
            let s = String::from_utf8_lossy(&o.stdout).to_lowercase();
            // 同时匹配 PID 与本程序映像名，避免 PID 复用误判为「已在运行」
            return s.contains(&pid.to_string()) && s.contains("workbuddy-account-hub");
        }
        false
    }
    #[cfg(not(windows))] {
        std::process::Command::new("kill").args(["-0", &pid.to_string()])
            .status().map(|s| s.success()).unwrap_or(false)
    }
}

/// 确保单实例：若已被另一存活实例占用则退出；陈旧锁自动清理后占位。
/// 锁文件「新鲜度」阈值：心跳每 10s 重写一次，超过 30s 没更新视为陈旧锁。
/// 这是 v0.6.29 修的回归：只靠 pid + `kill -0` 判活会被 **PID 复用**骗过
/// （旧实例退出后 pid 可能被别的进程占用），导致新实例以为自己"已在运行"而静默退出。
const LOCK_STALE_SECS: u64 = 30;

fn lock_recent(p: &std::path::Path) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() < LOCK_STALE_SECS)
        .unwrap_or(false)
}

fn write_lock(p: &std::path::Path) -> bool {
    std::fs::write(p, std::process::id().to_string()).is_ok()
}

/// 心跳：运行期间每 10s 重写锁文件刷新 mtime（内容仍是本进程 pid）。
/// 进程退出/崩溃后心跳停止，30s 后锁自动失效，不会再卡住下一次启动。
fn spawn_lock_heartbeat(p: std::path::PathBuf) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        let mine = std::fs::read_to_string(&p)
            .ok()
            .map(|s| s.trim() == std::process::id().to_string())
            .unwrap_or(false);
        if mine { let _ = write_lock(&p); } else { break; }
    });
}

fn ensure_single_instance() -> bool {
    let p = instance_lock_path();
    if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); }
    // 原子抢锁（create_new / O_EXCL）避免两实例同时启动的双开竞态
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
        Ok(mut f) => {
            use std::io::Write;
            let _ = f.write_all(std::process::id().to_string().as_bytes());
            drop(f);
            spawn_lock_heartbeat(p);
            true
        }
        Err(_) => {
            // 锁已存在：只有「进程活着 且 心跳新鲜」才算真的在运行
            let alive = std::fs::read_to_string(&p)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .map(pid_alive)
                .unwrap_or(false);
            if alive && lock_recent(&p) { return false; }
            let _ = std::fs::remove_file(&p);
            if write_lock(&p) { spawn_lock_heartbeat(p); true } else { false }
        }
    }
}

/// 签到错误分类 → （类型, 建议冷却秒数）（#16 冷却状态机）
fn classify_checkin_err(msg: &str) -> (String, u64) {
    let m = msg.to_lowercase();
    if m.contains("401") || m.contains("登录") || m.contains("身份过期") || m.contains("12153") {
        ("session_dead".into(), 0)        // 会话失效：需重新登录，禁用该账号
    } else if m.contains("429") || m.contains("频繁") {
        ("rate_limited".into(), 60)       // 限流：60s 后重试
    } else if m.contains("402") || m.contains("额度") {
        ("hard_credit".into(), 43200)     // 额度类：12h
    } else if m.contains("404") || m.contains("not found") {
        ("not_found".into(), 300)         // 未找到：短冷却
    } else {
        ("unknown".into(), 300)
    }
}

/// 当前 Unix 毫秒时间戳（本地计算，避免跨 crate 依赖）
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 把阻塞任务挪到独立线程池执行。
/// 性能关键：Tauri 2 同步命令在主线程执行，网络请求（超时上限 15s/请求）
/// 期间整个窗口会冻结（点击/滚动全部无响应）—— v0.6.2 卡顿根因。
/// async 命令 + spawn_blocking 让 UI 永不为网络等待而卡死。
async fn offload<F>(f: F) -> Value
where F: FnOnce() -> Value + Send + 'static {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .unwrap_or_else(|e| json!({ "ok": false, "error": format!("后台任务异常: {}", e) }))
}

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
    // 当前账号键：企业版是 `<uid>@<eid>`，与 current_uid 不同，前端据此高亮当前行
    if let Some(o) = v.as_object_mut() {
        o.insert("current_key".into(), serde_json::to_value(ops::current_account_key()).unwrap_or(Value::Null));
    }
    v
}

#[tauri::command]
async fn get_quota() -> Value {
    offload(move || {
        api::get_quota()
    }).await
}

// ===== v0.6.13 设置面板：WorkBuddy.exe 自动检测诊断 / 手选兜底 / 原生浏览框 =====

/// 登录态路径 + at-rest 加密能力诊断（issue #3「切换报成功但账号没变」一键取证）。
/// 返回：候选文件清单（路径/大小/mtime/是否含 token/是否精确名）、最终选中的活跃文件、
/// 以及 Hub 是否具备把敏感字段重新封装成 $wbEncrypted 信封的能力。
#[tauri::command]
fn get_auth_path_report() -> Value {
    api::auth_path_report()
}

/// 删除一个已保存的账号（失效账号清理）。
/// 快照会被移到 vault/.trash/ 而非直接删除，可用 restore_account 还原。
#[tauri::command]
fn delete_account(uid: String) -> Result<Value, String> {
    let r = ops::delete_account(&ops::vault_dir(), &uid)?;
    Ok(serde_json::to_value(r).unwrap_or(Value::Null))
}

/// 恢复被删除的账号：解除隐藏 + 从回收站移回最近一份快照。
#[tauri::command]
fn restore_account(uid: String) -> Result<Value, String> {
    ops::restore_account(&ops::vault_dir(), &uid)
}

/// 当前被隐藏/删除的账号清单（含是否还可还原）。
#[tauri::command]
fn list_ignored_accounts() -> Value {
    serde_json::to_value(ops::list_ignored(&ops::vault_dir())).unwrap_or(Value::Null)
}

/// 登录态三态预检（issue #3 问题 B）：只读、无副作用。
/// 每账号返回 state = ok / expiring / revoked / expired / error / no_login。
/// 账号间串行 + 250ms 间隔，避免触发风控。
#[tauri::command]
async fn preflight_accounts() -> Value {
    offload(move || {
        let rows = ops::preflight_accounts(&ops::vault_dir());
        let mut tally = serde_json::Map::new();
        for r in &rows {
            let k = r.get("state").and_then(|x| x.as_str()).unwrap_or("error").to_string();
            let c = tally.get(&k).and_then(|x| x.as_u64()).unwrap_or(0) + 1;
            tally.insert(k, json!(c));
        }
        json!({ "ok": true, "accounts": rows, "summary": tally })
    }).await
}

/// 单账号 RT 静默续期（issue #3 问题 C）。
/// 内部已做备份 / 当前账号保护 / 原子回写；失败返回原因，成功返回前后到期对比。
#[tauri::command]
async fn refresh_account(uid: String) -> Value {
    offload(move || match ops::refresh_account(&ops::vault_dir(), &uid) {
        Ok(r) => json!({ "ok": true, "result": r }),
        Err(e) => json!({ "ok": false, "error": e }),
    }).await
}

/// 批量续期：只处理「即将过期 / 已吊销」的账号（避免无谓消耗 RT 轮换）。
/// 强制跳过当前登录账号（客户端在跑时会和它抢同一个 refreshToken）。
#[tauri::command]
async fn refresh_all_accounts() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let rows = ops::preflight_accounts(&vault);
        let mut results = Vec::new();
        let targets: Vec<_> = rows
            .iter()
            .filter(|r| {
                let st = r.get("state").and_then(|x| x.as_str()).unwrap_or("");
                let cur = r.get("current").and_then(|x| x.as_bool()).unwrap_or(false);
                (st == "expiring" || st == "revoked") && !cur
            })
            .collect();
        let total = targets.len();
        for (i, r) in targets.iter().enumerate() {
            let uid = r.get("uid").and_then(|x| x.as_str()).unwrap_or("").to_string();
            match ops::refresh_account(&vault, &uid) {
                Ok(res) => results.push(json!({
                    "uid": uid, "nickname": r.get("nickname"), "ok": true, "message": res.message,
                })),
                Err(e) => results.push(json!({
                    "uid": uid, "nickname": r.get("nickname"), "ok": false, "error": e,
                })),
            }
            if i + 1 < total {
                // refresh 比签到敏感：间隔拉长到 800ms，避免批量触发风控/IP 限制
                std::thread::sleep(std::time::Duration::from_millis(800));
            }
        }
        json!({ "ok": true, "results": results, "attempted": total })
    }).await
}

#[tauri::command]
fn get_exe_settings() -> Value {
    api::wb_exe_diagnostics()
}

#[tauri::command]
fn save_exe_setting(path: String) -> Value {
    match api::save_manual_exe(&path) {
        Ok(()) => json!({ "ok": true }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

/// 原生「浏览…」文件选择框。rfd 的 AsyncFileDialog 内部处理主线程调度
/// （macOS 上对话框必须在主线程弹，Windows 上走 COM），故用异步 API + tauri 执行器等待。
#[tauri::command]
async fn pick_workbuddy_exe() -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let dlg = rfd::AsyncFileDialog::new().set_title("选择 WorkBuddy 客户端可执行文件");
        #[cfg(windows)]
        let dlg = dlg.add_filter("WorkBuddy.exe", &["exe"]);
        let picked = tauri::async_runtime::block_on(dlg.pick_file());
        match picked {
            Some(f) => json!({ "ok": true, "path": f.path().to_string_lossy().into_owned() }),
            None => json!({ "ok": false, "cancelled": true }),
        }
    }).await.unwrap_or_else(|e| json!({ "ok": false, "error": format!("对话框异常: {}", e) }))
}

#[tauri::command]
async fn get_checkin() -> Value {
    offload(move || {
        api::get_checkin()
    }).await
}

#[tauri::command]
async fn get_memory() -> Value {
    offload(move || {
        api::get_memory()
    }).await
}

#[tauri::command]
async fn do_checkin() -> Value {
    offload(move || {
        api::do_checkin()
    }).await
}

// ---------- 宠物旅行（buddy travel） ----------

#[tauri::command]
async fn buddy_status() -> Value {
    offload(move || {
        // v0.6.30：成长系统仅个人版——当前登录态是企业形态时降级用个人版快照
        let vault = ops::vault_dir();
        match ops::current_account_key().and_then(|k| growth_login_for(&vault, &k)) {
            Some(l) => api::buddy_status_as(&l),
            None => api::buddy_status(),
        }
    }).await
}

#[tauri::command]
async fn buddy_config() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        match ops::current_account_key().and_then(|k| growth_login_for(&vault, &k)) {
            Some(l) => api::buddy_config_as(&l),
            None => api::buddy_config(),
        }
    }).await
}

#[tauri::command]
async fn buddy_depart(location_id: String) -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        match ops::current_account_key().and_then(|k| growth_login_for(&vault, &k)) {
            Some(l) => api::buddy_depart_as(&l, &location_id),
            None => api::buddy_depart(&location_id),
        }
    }).await
}

#[tauri::command]
async fn buddy_claim() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        match ops::current_account_key().and_then(|k| growth_login_for(&vault, &k)) {
            Some(l) => api::buddy_claim_as(&l),
            None => api::buddy_claim(),
        }
    }).await
}

// ---------- 多账号批量操作（每个账号用各自 vault 快照里的登录态 token 发起请求） ----------

/// 从某账号的 vault 快照读取登录态（auth.info 含该账号 accessToken）
/// `key` 是账号键（`<uid>` 或 `<uid>@<enterpriseId>`），不是裸 uid。
fn account_login(vault: &std::path::Path, key: &str) -> Option<wb_api::LoginInfo> {
    let p = vault.join(key).join("snapshot").join("auth.info");
    if !p.exists() { return None; }
    wb_api::login_from_file(&p)
}

/// #26：签到瞬时可重试错误（限流/未知/未找到）做指数退避重试，避免偶发抖动直接判失败；
/// 终态错误（会话失效/额度类）不重试，直接走冷却状态机。
fn do_checkin_retry(login: &wb_api::LoginInfo) -> Value {
    let mut last = api::do_checkin_as(login);
    for attempt in 1..=2u32 {
        if last.get("error").is_none() { return last; }
        let emsg = last.get("error").and_then(|e| e.as_str()).unwrap_or("");
        let kind = classify_checkin_err(emsg).0;
        if kind == "session_dead" || kind == "hard_credit" { return last; }
        let backoff = 2000u64 * (1u64 << (attempt - 1)); // 2s, 4s 指数退避
        std::thread::sleep(std::time::Duration::from_millis(backoff));
        last = api::do_checkin_as(login);
    }
    last
}

/// 一键签到：对所有「已保存登录态」的账号执行今日签到，返回每个账号的结果。
/// 请求级轮转（#1）：round-robin 起始位，避免每次都从同一账号开始。
/// 临近过期（#1）：token JWT exp 在 10m 内 → 标记 needs_refresh 并跳过（无刷新端点，提示重新登录）。
#[tauri::command]
async fn checkin_all() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let accs = ops::list_accounts(&vault);
        let now = now_ms();
        // #1 请求级轮转：round-robin 起始位（进程内原子计数，每次调用 +1）
        let n = accs.len();
        let rotate = if n > 0 { ROTATE_INDEX.fetch_add(1, Ordering::Relaxed) % n } else { 0 };
        let order: Vec<usize> = if n > 0 { (0..n).map(|i| (i + rotate) % n).collect() } else { Vec::new() };
        let mut results = Vec::new();
        let mut ok = 0u32; let mut fail = 0u32; let mut skipped = 0u32;
        // #23：账号列表去重 + 上限保护，避免异常重复触发风暴。
        // v0.6.27：去重键从 uid 改为账号键——同一账号的个人版与企业版是两个独立
        // 登录态（不同 token），应各签一次；用 uid 去重会把其中一个整个吞掉。
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for idx in order {
            let a = &accs[idx];
            if seen.len() >= 100 { break; }              // #23 总量上限
            if !seen.insert(a.key.clone()) { continue; }  // #23 去重
            if !a.has_snapshot {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "error": "无登录态快照"}));
                skipped += 1; continue;
            }
            let Some(login) = account_login(&vault, &a.key) else {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "error": "登录态文件缺失或无效"}));
                skipped += 1; continue;
            };
            // #1 临近过期：token JWT exp 在 10m 内 → 标记 needs_refresh 并跳过
            let near_exp = api::jwt_payload(&login.token)
                .and_then(|pl| pl.get("exp").and_then(|x| x.as_u64()))
                .map(|exp| (exp as i64) * 1000 - (now as i64) < 10 * 60 * 1000)
                .unwrap_or(false);
            if near_exp {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true,
                    "needs_refresh": true, "error": "token 临近过期，请重新登录以刷新"}));
                skipped += 1; continue;
            }
            // ===== 签到前预检（issue #3 问题 B）=====
            // 服务端 2026-10 更换 token 体系后，一批「JWT 还没到期」的旧 token 被统一吊销，
            // 这类账号签到必然 401 且 hub 侧无法自愈。预检把它们提前分流成三态标注：
            //   revoked / expired → 直接跳过并发「需重新登录」信号，不再白发一次签到请求；
            //   ok / expiring     → 继续签到（顺带利用预检拿到的 today_checked_in 省一次请求）；
            //   error（网络等）    → 仍尝试签到，交给 classify_checkin_err 兜底。
            let pf = api::preflight_login(&login);
            let pstate = pf.get("state").and_then(|x| x.as_str()).unwrap_or("error").to_string();
            let plabel = pf.get("label").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let pdetail = pf.get("detail").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if pstate == "revoked" || pstate == "expired" {
                results.push(json!({
                    "uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true,
                    "needs_relogin": true, "kind": "session_dead", "state": pstate,
                    "at_days_left": pf.get("at_days_left").cloned().unwrap_or(Value::Null),
                    "label": plabel,
                    "error": if pdetail.is_empty() { plabel.clone() } else { format!("{plabel}：{pdetail}") },
                }));
                skipped += 1;
                std::thread::sleep(std::time::Duration::from_millis(250));
                continue;
            }
            if pf.get("checked_in_today").and_then(|x| x.as_bool()) == Some(true) {
                results.push(json!({
                    "uid": a.uid, "nickname": a.nickname, "ok": true, "skipped": true,
                    "state": pstate, "label": plabel, "message": "今日已签到",
                }));
                skipped += 1;
                std::thread::sleep(std::time::Duration::from_millis(250));
                continue;
            }
            let r = do_checkin_retry(&login);
            if let Some(err) = r.get("error") {
                let emsg = err.as_str().unwrap_or("");
                let (kind, cooldown) = classify_checkin_err(emsg);
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": false,
                    "error": err, "kind": kind, "cooldown": cooldown}));
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
            // 账号间限速 250ms，避免触发风控（对标 daemon.js:2221-2235 CHECKIN_QUEUE_DELAY_MS）
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        json!({ "ok": true, "results": results, "summary": { "total": results.len(), "ok": ok, "fail": fail, "skipped": skipped } })
    }).await
}

/// 查询所有「已保存登录态」账号的额度，按登记表原始顺序返回。
/// v0.6.2 性能重写：async + spawn_blocking（不冻结 UI）+ 账号间并行请求
/// （原串行 5 账号 × 每账号最多 15s → 现总耗时 = 最慢单账号，通常 1~3s）。
// ===== v0.6.29：Roadmap 功能（webhook 通知 / 自动续期 / 会话复制）=====

/// 通知 webhook：支持 Bark（GET {url}/{title}/{body}）与企业微信群机器人（POST JSON）。
/// 只做「推一条文本」，不重试、不排队——失败由调用方提示，避免影响主流程。
/// 成长任务列表（宠物 tab 展示；企业形态自动降级个人版快照）
#[tauri::command]
fn growth_tasks() -> Value {
    let vault = ops::vault_dir();
    let Some(key) = ops::current_account_key() else { return json!({ "ok": false, "error": "未找到本机登录态" }) };
    match growth_login_for(&vault, &key) {
        Some(l) => api::growth_tasks_as(&l),
        None => json!({ "ok": false, "error": "登录态文件缺失或无效" }),
    }
}

/// 领取单个已完成任务的奖励
#[tauri::command]
fn growth_task_claim(task_id: i64) -> Value {
    let vault = ops::vault_dir();
    let Some(key) = ops::current_account_key() else { return json!({ "ok": false, "error": "未找到本机登录态" }) };
    match growth_login_for(&vault, &key) {
        Some(l) => api::growth_task_claim_as(&l, task_id),
        None => json!({ "ok": false, "error": "登录态文件缺失或无效" }),
    }
}

#[tauri::command]
fn send_webhook(kind: String, url: String, title: String, body: String) -> Value {
    api::send_webhook(&kind, &url, &title, &body)
}

/// 自动续期守护：扫描所有账号快照，把「即将过期 / 已吊销」且非当前登录的账号自动续期。
/// 返回逐账号结果。由前端定时（默认关闭，需在设置里开启）调用。
#[tauri::command]
fn auto_refresh_sweep(days: Option<i64>) -> Value {
    let days = days.unwrap_or(7);
    let vault = ops::vault_dir();
    let cur = ops::current_account_key();
    let accs = ops::list_accounts(&vault);
    let mut out = Vec::new();
    let mut refreshed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    for a in accs {
        if cur.as_deref() == Some(a.key.as_str()) {
            // 当前登录态的 token 由客户端自己续，Hub 不动它（避免与客户端抢 RT 轮换）
            skipped += 1; continue;
        }
        if !a.has_snapshot { skipped += 1; continue; }
        let snap = vault.join(&a.key).join("snapshot").join("auth.info");
        let Ok(txt) = std::fs::read_to_string(&snap) else { skipped += 1; continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) else { skipped += 1; continue };
        let exp = v.get("auth").and_then(|x| x.get("expiresAt")).and_then(|x| x.as_i64()).unwrap_or(0);
        let left_ms = exp - chrono_now_ms();
        let need = exp == 0 || left_ms < days * 24 * 3600 * 1000;
        if !need { skipped += 1; continue; }
        match ops::refresh_account(&vault, &a.key) {
            Ok(r) => {
                refreshed += 1;
                out.push(json!({ "key": a.key, "nickname": a.nickname, "ok": true, "at_days_after": r.at_days_after }));
            }
            Err(e) => {
                failed += 1;
                out.push(json!({ "key": a.key, "nickname": a.nickname, "ok": false, "error": e }));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(400)); // 账号间限速，避免风控
    }
    json!({ "ok": true, "refreshed": refreshed, "failed": failed, "skipped": skipped, "results": out })
}

/// 会话复制（跨账号，源不受影响）：把 from 账号的本地会话**复制**一份给 to 账号。
/// 与切换时的「搬移」不同——这里生成新 id 的副本，源账号照常可见。
/// 注意：只处理本地 workbuddy.db；云端会话需客户端自行同步。
#[tauri::command]
fn copy_sessions(from_key: String, to_key: String) -> Value {
    match ops::copy_sessions(&from_key, &to_key) {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

// ===== OpenAI 兼容网关（v0.6.29，自 workbuddy2api-hub 的思路，仅本机可用）=====
//
// 设计取舍：只监听 127.0.0.1（绝不 0.0.0.0），默认端口 8787。
// 账号选择用请求头 `X-WB-Account`（账号键或 uid 前 8 位），缺省用当前登录态。
// 上游仍是官方 /v2/chat/completions——Hub 只做协议转发，不伪造客户端身份、
// 不做多账号轮询压榨额度（那才是风控高发区）。
use std::sync::atomic::{AtomicBool, AtomicU16};

static GW_RUNNING: AtomicBool = AtomicBool::new(false);
static GW_PORT: AtomicU16 = AtomicU16::new(0);

fn gw_pick_login(sel: &str) -> Option<wb_api::LoginInfo> {
    let vault = ops::vault_dir();
    if sel.trim().is_empty() {
        return ops::current_account_key().and_then(|k| account_login(&vault, &k));
    }
    let accs = ops::list_accounts(&vault);
    let key = accs.iter()
        .find(|a| a.key == sel || a.uid == sel || a.uid.starts_with(sel))
        .map(|a| a.key.clone())?;
    account_login(&vault, &key)
}

async fn gw_models() -> axum::response::Response {
    let vault = ops::vault_dir();
    let accs = ops::list_accounts(&vault);
    let data: Vec<serde_json::Value> = accs.iter().map(|a| json!({
        "id": a.key,
        "object": "model",
        "owned_by": if a.form == "enterprise" { "enterprise" } else { "personal" },
        "name": a.nickname,
    })).collect();
    axum::Json(json!({ "object": "list", "data": data })).into_response()
}

use axum::response::IntoResponse;

async fn gw_chat(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
    let sel = headers.get("x-wb-account").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let Some(login) = gw_pick_login(&sel) else {
        return (axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(json!({ "error": { "message": "找不到可用账号（X-WB-Account 未匹配到已保存登录态）", "type": "invalid_request_error" } })))
            .into_response();
    };
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(300)).build() {
        Ok(c) => c,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({ "error": { "message": format!("client: {e}") } }))).into_response(),
    };
    let url = format!("{}/v2/chat/completions", wb_api::api_base(&login));
    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", login.token))
        .header("Content-Type", "application/json")
        .header("X-User-Id", &login.uid)
        .header("X-Product", "SaaS")
        .body(body.to_vec())
        .send().await;
    match resp {
        Ok(r) => {
            let status = r.status();
            // 流式透传：SSE 必须保持 chunked，不能整体缓冲
            let stream = r.bytes_stream();
            let mut out = axum::response::Response::new(axum::body::Body::from_stream(stream));
            *out.status_mut() = status;
            out.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("text/event-stream"),
            );
            out
        }
        Err(e) => (axum::http::StatusCode::BAD_GATEWAY,
            axum::Json(json!({ "error": { "message": format!("上游请求失败: {e}") } }))).into_response(),
    }
}

#[tauri::command]
async fn start_gateway(port: Option<u16>) -> Value {
    if GW_RUNNING.load(Ordering::Relaxed) {
        return json!({ "ok": true, "already": true, "port": GW_PORT.load(Ordering::Relaxed) });
    }
    let port = port.unwrap_or(8787);
    let app = axum::Router::new()
        .route("/v1/models", axum::routing::get(gw_models))
        .route("/v1/chat/completions", axum::routing::post(gw_chat));
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => return json!({ "ok": false, "error": format!("端口 {port} 绑定失败: {e}") }),
    };
    GW_RUNNING.store(true, Ordering::Relaxed);
    GW_PORT.store(port, Ordering::Relaxed);
    tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, app).await;
        GW_RUNNING.store(false, Ordering::Relaxed);
    });
    json!({ "ok": true, "port": port, "base_url": format!("http://127.0.0.1:{port}/v1") })
}

#[tauri::command]
fn stop_gateway() -> Value {
    // 没有优雅关闭句柄：置位后由前端停止调用；下次启动会重新绑定
    GW_RUNNING.store(false, Ordering::Relaxed);
    json!({ "ok": true, "note": "已停止接收新请求（监听 socket 将在进程退出时释放）" })
}

#[tauri::command]
fn gateway_status() -> Value {
    json!({
        "running": GW_RUNNING.load(Ordering::Relaxed),
        "port": GW_PORT.load(Ordering::Relaxed),
    })
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tauri::command]
fn get_invite() -> Value {
    match ops::current_account_key().and_then(|k| account_login(&ops::vault_dir(), &k)) {
        Some(l) => api::get_invite_as(&l),
        None => json!({ "ok": false, "error": "未找到本机登录态" }),
    }
}

#[tauri::command]
fn enterprise_usage() -> Value {
    match ops::current_account_key().and_then(|k| account_login(&ops::vault_dir(), &k)) {
        Some(l) => api::enterprise_usage_as(&l),
        None => json!({ "error": "未找到本机登录态" }),
    }
}

#[tauri::command]
async fn quota_all() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let accs = ops::list_accounts(&vault);
        // 有快照的账号并行拉取（std::thread::scope 逐账号一线程，结果按登记顺序收集）
        // v0.6.27：限并发——每批最多 3 个线程同时打网关，与签到侧的风控策略对齐。
        // （原实现 N 账号 = N 并发，14 个账号会瞬间打出 14 个请求，容易触发网关限流，
        //  历史上「6/14 账号 401」疑似与此相关。）
        // 元组第一元素统一装账号键 key（企业版是 <uid>@<eid>，用纯 uid 会拿错快照）。
        let snap: Vec<_> = accs.iter().filter(|a| a.has_snapshot).collect();
        let mut fetched: Vec<(String, String, Value)> = Vec::new();
        for chunk in snap.chunks(3) {
            let part: Vec<(String, String, Value)> = std::thread::scope(|s| {
                // vault shadow 成共享引用：多线程各自捕获 &Path（Copy），避免重复 move
                let vault: &std::path::Path = vault.as_path();
                let hs: Vec<_> = chunk
                    .iter()
                    .map(|a| {
                        let key = a.key.clone();
                        let uid_only = a.uid.clone();
                        let form = a.form.clone();
                        let nick = a.nickname.clone().unwrap_or_default();
                        s.spawn(move || {
                            // 成长系统仅个人版（实测 5.9.3）：企业形态自动改用同账号的
                            // 个人版快照 token；没有个人版快照则按登录态缺失跳过。
                            let login = if form == "enterprise" {
                                account_login(&vault, &uid_only)
                            } else {
                                account_login(&vault, &key)
                            };
                            let r = match login {
                                Some(login) => api::get_quota_as(&login),
                                None => Value::Null, // 登录态文件缺失 → 上层记 skipped
                            };
                            (key, nick, r)
                        })
                    })
                    .collect();
                hs.into_iter()
                    .map(|h| h.join().unwrap_or_else(|_| (String::new(), String::new(), Value::Null)))
                    .collect()
            });
            fetched.extend(part);
        }
        let mut results = Vec::new();
        let mut ok = 0u32; let mut fail = 0u32; let mut skipped = 0u32; let mut denied = 0u32;
        let mut fi = 0usize;
        for a in &accs {
            if !a.has_snapshot {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "error": "无登录态快照"}));
                skipped += 1; continue;
            }
            let (uid, nick, r) = &fetched[fi]; fi += 1;
            if r.is_null() {
                results.push(json!({"uid": uid, "nickname": nick, "ok": false, "skipped": true, "error": "登录态文件缺失或无效"}));
                skipped += 1; continue;
            }
            if r.get("permission_denied").and_then(|x| x.as_bool()).unwrap_or(false) {
                denied += 1;
                let cached = r.get("cached").is_some();
                results.push(json!({
                    "uid": uid, "nickname": nick, "ok": false, "permission_denied": true,
                    "permission_msg": r.get("permission_msg").and_then(|x| x.as_str()).unwrap_or(""),
                    "cached": cached,
                    "body": r.get("body").cloned().unwrap_or(Value::Null),
                    "parsed": r.get("parsed").cloned().unwrap_or(Value::Null)
                }));
                continue;
            }
            if let Some(err) = r.get("error") {
                let emsg = err.as_str().unwrap_or("");
                let (kind, cooldown) = classify_checkin_err(emsg);
                results.push(json!({"uid": uid, "nickname": nick, "ok": false, "skipped": false, "error": err, "kind": kind, "cooldown": cooldown}));
                fail += 1; continue;
            }
            let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
            if status == 200 { ok += 1; } else { fail += 1; }
            results.push(json!({
                "uid": uid, "nickname": nick, "ok": status == 200, "skipped": false,
                "status": status,
                "body": r.get("body").cloned().unwrap_or(Value::Null),
                "parsed": r.get("parsed").cloned().unwrap_or(Value::Null)
            }));
        }
        json!({ "ok": true, "results": results, "summary": { "total": results.len(), "ok": ok, "fail": fail, "skipped": skipped, "denied": denied } })
    }).await
}

/// 单个账号签到（用于全部账号额度表格里的逐行操作，幂等：已签则跳过）
#[tauri::command]
async fn checkin_for(uid: String) -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let Some(login) = account_login(&vault, &uid) else {
            return json!({ "uid": uid, "ok": false, "error": "登录态文件缺失或无效" });
        };
        api::do_checkin_as(&login)
    }).await
}

/// 查询所有「已保存登录态」账号的 AI 记忆画像，返回每个账号的画像明细
/// （与 quota_all 同构，覆盖 dashboard 只能看当前登录态单账号的局限）
#[tauri::command]
async fn memory_all() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let accs = ops::list_accounts(&vault);
        let mut results = Vec::new();
        let mut ok = 0u32; let mut fail = 0u32; let mut skipped = 0u32;
        for a in accs {
            if !a.has_snapshot {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "error": "无登录态快照"}));
                skipped += 1; continue;
            }
            let Some(login) = account_login(&vault, &a.key) else {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "error": "登录态文件缺失或无效"}));
                skipped += 1; continue;
            };
            let r = api::get_memory_as(&login);
            if let Some(err) = r.get("error") {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": false, "error": err}));
                fail += 1; continue;
            }
            let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
            if status == 200 { ok += 1; } else { fail += 1; }
            results.push(json!({
                "uid": a.uid, "nickname": a.nickname, "ok": status == 200, "skipped": false,
                "status": status, "body": r.get("body").cloned().unwrap_or(Value::Null)
            }));
        }
        json!({ "ok": true, "results": results, "summary": { "total": results.len(), "ok": ok, "fail": fail, "skipped": skipped } })
    }).await
}

/// 查询所有「已保存登录态」账号的宠物当前状态
/// v0.6.2 性能重写：async + 账号间并行请求（原串行冻结 UI 且慢）
#[tauri::command]
async fn buddy_all_status() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let accs = ops::list_accounts(&vault);
        // 单账号响应 → 最终行 JSON 的解析（供并行线程与本地跳过分支共用）
        let parse_row = |uid: &str, nick: &str, r: &Value| -> Value {
            if let Some(err) = r.get("error") {
                return json!({"uid": uid, "nickname": nick, "has_login": true, "error": err});
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
            json!({
                "uid": uid, "nickname": nick, "has_login": true, "status": status,
                "state": state, "location": location, "reward_credit": reward,
                "arrive_at": arrive_at, "daily_limit_reached": daily
            })
        };
        // v0.6.27：限并发——每批最多 3 个线程同时打网关，与签到侧的风控策略对齐。
        // （原实现 N 账号 = N 并发，14 个账号会瞬间打出 14 个请求，容易触发网关限流，
        //  历史上「6/14 账号 401」疑似与此相关。）
        // 元组第一元素统一装账号键 key（企业版是 <uid>@<eid>，用纯 uid 会拿错快照）。
        let snap: Vec<_> = accs.iter().filter(|a| a.has_snapshot).collect();
        let mut fetched: Vec<(String, String, Value)> = Vec::new();
        for chunk in snap.chunks(3) {
            let part: Vec<(String, String, Value)> = std::thread::scope(|s| {
                // vault shadow 成共享引用：多线程各自捕获 &Path（Copy），避免重复 move
                let vault: &std::path::Path = vault.as_path();
                let hs: Vec<_> = chunk
                    .iter()
                    .map(|a| {
                        let key = a.key.clone();
                        let uid_only = a.uid.clone();
                        let form = a.form.clone();
                        let nick = a.nickname.clone().unwrap_or_default();
                        s.spawn(move || {
                            // 成长系统仅个人版（实测 5.9.3）：企业形态自动改用同账号的
                            // 个人版快照 token；没有个人版快照则按登录态缺失跳过。
                            let login = if form == "enterprise" {
                                account_login(&vault, &uid_only)
                            } else {
                                account_login(&vault, &key)
                            };
                            let r = match login {
                                Some(login) => api::buddy_status_as(&login),
                                None => Value::Null,
                            };
                            (key, nick, r)
                        })
                    })
                    .collect();
                hs.into_iter()
                    .map(|h| h.join().unwrap_or_else(|_| (String::new(), String::new(), Value::Null)))
                    .collect()
            });
            fetched.extend(part);
        }
        let mut accounts = Vec::new();
        let mut fi = 0usize;
        for a in &accs {
            if !a.has_snapshot {
                accounts.push(json!({"uid": a.uid, "nickname": a.nickname, "has_login": false, "error": "无登录态快照"}));
                continue;
            }
            let (uid, nick, r) = &fetched[fi]; fi += 1;
            if r.is_null() {
                accounts.push(json!({"uid": uid, "nickname": nick, "has_login": false, "error": "登录态文件缺失或无效"}));
                continue;
            }
            accounts.push(parse_row(uid, nick, r));
        }
        json!({ "ok": true, "accounts": accounts })
    }).await
}

/// 判断宠物接口返回的 message 是否是「未激活宠物」类业务错误（无法派出）
fn is_no_active_buddy(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("no active buddy") || m.contains("未激活") || m.contains("激活宠物")
        || (m.contains("buddy") && m.contains("active"))
}

/// 判断领取接口返回的 message 是否是「无待领取奖励」类幂等业务错误（忽略即可）
fn is_already_claimed(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("unclaimed") || m.contains("no unclaimed") || m.contains("未领取")
        || m.contains("已领取") || m.contains("no active buddy") || m.contains("未激活")
}

/// 单账号宠物派出（含预检）：
/// 仅当 state==idle 且未达每日上限且有激活宠物时才真正派出；
/// 否则按原因归为「跳过」（非错误），避免把业务不可派出误算成失败。
/// 返回 { uid, nickname, ok, skipped, status?, message?, reason, body? }
fn depart_one(login: &wb_api::LoginInfo, location_id: &str, uid: &str, nickname: &str) -> Value {
    // 预检 1：查该账号宠物当前状态
    let st = api::buddy_status_as(login);
    if let Some(err) = st.get("error") {
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": false, "reason": err, "error": err});
    }
    let st_status = st.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
    if st_status != 200 {
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": false, "reason": format!("状态查询 HTTP {}", st_status), "status": st_status});
    }
    let st_body = st.get("body");
    let code = st_body.and_then(|b| b.get("code")).and_then(|x| x.as_i64());
    let st_msg = st_body.and_then(|b| b.get("msg")).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let st_data = st_body.and_then(|b| b.get("data")).or(st_body);
    // 预检 2：业务码非 0（如未激活宠物）
    if code.map(|c| c != 0).unwrap_or(false) {
        let reason = if is_no_active_buddy(&st_msg) {
            "该账号未在官方激活宠物，请先在 WorkBuddy 客户端激活宠物".to_string()
        } else {
            format!("状态查询失败：{}", if st_msg.is_empty() { "未知业务错误".to_string() } else { st_msg.clone() })
        };
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": true, "reason": reason, "status": st_status});
    }
    let state = st_data.and_then(|d| d.get("state")).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let daily = st_data.and_then(|d| d.get("daily_limit_reached")).and_then(|x| x.as_bool()).unwrap_or(false);
    if state.is_empty() {
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": true, "reason": "无法获取宠物状态（可能未激活宠物）"});
    }
    if state == "traveling" {
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": true, "reason": "宠物正在旅行中，无法派出"});
    }
    if state == "arrived" {
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": true, "reason": "宠物已归来，请先领取奖励"});
    }
    if daily {
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": true, "reason": "今日已派出达每日上限"});
    }
    // 真正派出
    let r = api::buddy_depart_as(login, location_id);
    if let Some(err) = r.get("error") {
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": false, "reason": err, "error": err});
    }
    let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
    let body = r.get("body");
    let d = body.and_then(|b| b.get("data")).or(body);
    let msg = d.and_then(|x| x.get("message")).and_then(|x| x.as_str())
        .or_else(|| body.and_then(|b| b.get("message")).and_then(|x| x.as_str()))
        .unwrap_or("").to_string();
    if status == 200 {
        json!({"uid": uid, "nickname": nickname, "ok": true, "skipped": false, "status": status, "message": msg, "reason": "已派出", "body": body.cloned().unwrap_or(Value::Null)})
    } else if is_no_active_buddy(&msg) {
        json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": true, "reason": "该账号未在官方激活宠物，请先激活", "status": status})
    } else {
        json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": false, "reason": if msg.is_empty() { format!("HTTP {}", status) } else { msg.clone() }, "status": status, "message": msg, "body": body.cloned().unwrap_or(Value::Null)})
    }
}

/// 一键派出所有「已保存登录态」账号的宠物到同一地点（预检 + 三级计数）
#[tauri::command]
async fn buddy_all_depart(location_id: String) -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let accs = ops::list_accounts(&vault);
        let mut results = Vec::new();
        let mut ok = 0u32; let mut fail = 0u32; let mut skipped = 0u32;
        for a in accs {
            if !a.has_snapshot {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "reason": "无登录态快照"}));
                skipped += 1; continue;
            }
            // 成长系统仅个人版：企业形态降级用个人版快照（见 buddy_all_status 同款注释）
            let login = if a.form == "enterprise" { account_login(&vault, &a.uid) } else { account_login(&vault, &a.key) };
            let Some(login) = login else {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "reason": "登录态文件缺失或无效（企业形态且无个人版快照）"}));
                skipped += 1; continue;
            };
            let res = depart_one(&login, &location_id, &a.key, a.nickname.as_deref().unwrap_or(""));
            let okf = res.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            let skf = res.get("skipped").and_then(|x| x.as_bool()).unwrap_or(false);
            if okf { ok += 1; } else if skf { skipped += 1; } else { fail += 1; }
            results.push(res);
        }
        json!({ "ok": true, "results": results, "summary": { "total": results.len(), "ok": ok, "fail": fail, "skipped": skipped } })
    }).await
}

/// 单账号宠物奖励领取（含幂等预检）：HTTP 200 算成功；
/// 400「无待领取奖励 / 未激活宠物」等幂等情形归为「跳过」（非错误）。
fn claim_one(login: &wb_api::LoginInfo, uid: &str, nickname: &str) -> Value {
    let r = api::buddy_claim_as(login);
    if let Some(err) = r.get("error") {
        return json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": false, "reason": err, "error": err});
    }
    let status = r.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
    let body = r.get("body");
    let d = body.and_then(|b| b.get("data")).or(body);
    let msg = d.and_then(|x| x.get("message")).and_then(|x| x.as_str())
        .or_else(|| body.and_then(|b| b.get("message")).and_then(|x| x.as_str()))
        .unwrap_or("").to_string();
    if status == 200 {
        json!({"uid": uid, "nickname": nickname, "ok": true, "skipped": false, "status": status, "message": msg, "reason": "已领取", "body": body.cloned().unwrap_or(Value::Null)})
    } else if is_already_claimed(&msg) {
        json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": true, "reason": "无待领取奖励（已领取或宠物未归来）", "status": status})
    } else {
        json!({"uid": uid, "nickname": nickname, "ok": false, "skipped": false, "reason": if msg.is_empty() { format!("HTTP {}", status) } else { msg.clone() }, "status": status, "message": msg, "body": body.cloned().unwrap_or(Value::Null)})
    }
}

/// 一键领取所有「已保存登录态」账号的宠物奖励（三级计数）
#[tauri::command]
async fn buddy_all_claim() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let accs = ops::list_accounts(&vault);
        let mut results = Vec::new();
        let mut ok = 0u32; let mut fail = 0u32; let mut skipped = 0u32;
        for a in accs {
            if !a.has_snapshot {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "reason": "无登录态快照"}));
                skipped += 1; continue;
            }
            // 成长系统仅个人版：企业形态降级用个人版快照（见 buddy_all_status 同款注释）
            let login = if a.form == "enterprise" { account_login(&vault, &a.uid) } else { account_login(&vault, &a.key) };
            let Some(login) = login else {
                results.push(json!({"uid": a.uid, "nickname": a.nickname, "ok": false, "skipped": true, "reason": "登录态文件缺失或无效（企业形态且无个人版快照）"}));
                skipped += 1; continue;
            };
            let res = claim_one(&login, &a.key, a.nickname.as_deref().unwrap_or(""));
            let okf = res.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            let skf = res.get("skipped").and_then(|x| x.as_bool()).unwrap_or(false);
            if okf { ok += 1; } else if skf { skipped += 1; } else { fail += 1; }
            results.push(res);
        }
        json!({ "ok": true, "results": results, "summary": { "total": results.len(), "ok": ok, "fail": fail, "skipped": skipped } })
    }).await
}

/// 单个账号：派出宠物到指定地点（用于全部账号表格里的逐行操作，含预检）
#[tauri::command]
async fn buddy_depart_for(uid: String, location_id: String) -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let login = growth_login_for(&vault, &uid);
        let Some(login) = login else {
            return json!({"uid": uid, "ok": false, "skipped": false, "error": "登录态文件缺失或无效"});
        };
        depart_one(&login, &location_id, &uid, "")
    }).await
}

/// 单个账号：领取宠物奖励（用于全部账号表格里的逐行操作，含幂等预检）
#[tauri::command]
async fn buddy_claim_for(uid: String) -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let login = growth_login_for(&vault, &uid);
        let Some(login) = login else {
            return json!({"uid": uid, "ok": false, "skipped": false, "error": "登录态文件缺失或无效"});
        };
        claim_one(&login, &uid, "")
    }).await
}

// ---------- 宠物能量 / 盲盒抽奖（growth center） ----------

/// 查询所有「已保存登录态」账号的宠物能量与抽奖额度，返回每个账号的能量是否已满。
/// 与 buddy_all_status / quota_all 同构，覆盖 dashboard 只能看当前账号的局限。
/// 全部账号宠物能量（v0.6.2：async + 账号间并行，原串行会冻结 UI 且慢）
#[tauri::command]
async fn pet_energy_all() -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let accs = ops::list_accounts(&vault);
        // v0.6.27：限并发——每批最多 3 个线程同时打网关，与签到侧的风控策略对齐。
        // （原实现 N 账号 = N 并发，14 个账号会瞬间打出 14 个请求，容易触发网关限流，
        //  历史上「6/14 账号 401」疑似与此相关。）
        // 元组第一元素统一装账号键 key（企业版是 <uid>@<eid>，用纯 uid 会拿错快照）。
        let snap: Vec<_> = accs.iter().filter(|a| a.has_snapshot).collect();
        let mut fetched: Vec<(String, String, Value)> = Vec::new();
        for chunk in snap.chunks(3) {
            let part: Vec<(String, String, Value)> = std::thread::scope(|s| {
                // vault shadow 成共享引用：多线程各自捕获 &Path（Copy），避免重复 move
                let vault: &std::path::Path = vault.as_path();
                let hs: Vec<_> = chunk
                    .iter()
                    .map(|a| {
                        let key = a.key.clone();
                        let uid_only = a.uid.clone();
                        let form = a.form.clone();
                        let nick = a.nickname.clone().unwrap_or_default();
                        s.spawn(move || {
                            // 成长系统仅个人版（实测 5.9.3）：企业形态自动改用同账号的
                            // 个人版快照 token；没有个人版快照则按登录态缺失跳过。
                            let login = if form == "enterprise" {
                                account_login(&vault, &uid_only)
                            } else {
                                account_login(&vault, &key)
                            };
                            let r = match login {
                                Some(login) => api::pet_energy_as(&login),
                                None => Value::Null,
                            };
                            (key, nick, r)
                        })
                    })
                    .collect();
                hs.into_iter()
                    .map(|h| h.join().unwrap_or_else(|_| (String::new(), String::new(), Value::Null)))
                    .collect()
            });
            fetched.extend(part);
        }
        let mut accounts = Vec::new();
        let mut fi = 0usize;
        for a in &accs {
            if !a.has_snapshot {
                accounts.push(json!({"uid": a.uid, "nickname": a.nickname, "has_login": false, "error": "无登录态快照"}));
                continue;
            }
            let (uid, nick, r) = &fetched[fi]; fi += 1;
            if r.is_null() {
                accounts.push(json!({"uid": uid, "nickname": nick, "has_login": false, "error": "登录态文件缺失或无效"}));
                continue;
            }
            if let Some(err) = r.get("error") {
                accounts.push(json!({"uid": uid, "nickname": nick, "has_login": true, "error": err}));
                continue;
            }
            let balance = r.get("energy_balance").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let cost = r.get("cost_per_open").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let affordable = r.get("affordable").and_then(|x| x.as_i64()).unwrap_or(0);
            let max_open = r.get("max_open_count").and_then(|x| x.as_i64()).unwrap_or(0);
            let earned = r.get("energy_earned").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let consumed = r.get("energy_consumed").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let is_full = r.get("is_full").and_then(|x| x.as_bool()).unwrap_or(false);
            accounts.push(json!({
                "uid": uid, "nickname": nick, "has_login": true,
                "energy_balance": balance, "cost_per_open": cost,
                "affordable": affordable, "max_open_count": max_open,
                "energy_earned": earned, "energy_consumed": consumed,
                "is_full": is_full
            }));
        }
        json!({ "ok": true, "accounts": accounts })
    }).await
}

/// 宠物/成长类命令的登录态选择（v0.6.28）：
/// 成长系统仅个人版可用——key 是企业形态时自动降级用同账号的个人版快照。
fn growth_login_for(vault: &std::path::Path, key: &str) -> Option<wb_api::LoginInfo> {
    let (uid_only, eid) = ops::parse_key(key);
    if eid.is_some() {
        account_login(vault, uid_only).or_else(|| account_login(vault, key))
    } else {
        account_login(vault, key)
    }
}

/// 单个账号：抽盲盒（用其 vault 快照登录态 token 发请求）。
/// count 默认抽满当前可抽数量（受能量与每日上限限制）；返回 { ok, status, message?, data?, error? }
#[tauri::command]
async fn pet_draw_for(uid: String, count: Option<u64>) -> Value {
    offload(move || {
        let vault = ops::vault_dir();
        let Some(login) = account_login(&vault, &uid) else {
            return json!({"uid": uid, "ok": false, "error": "登录态文件缺失或无效"});
        };
        let cnt = count.unwrap_or(1);
        api::pet_draw_as(&login, cnt)
    }).await
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
async fn test_custom_model(model: Value) -> Value {
    offload(move || {
        api::models::test_model(&model)
    }).await
}

#[tauri::command]
async fn official_models() -> Value {
    offload(move || {
        api::models::official_list()
    }).await
}

#[tauri::command]
fn current_model() -> Value {
    api::models::current_model()
}

// ---------- 账号管理（account_ops） ----------

#[tauri::command]
fn list_accounts() -> Value {
    let accs = ops::list_accounts(&ops::vault_dir());
    Value::Object({
        let mut m = serde_json::Map::new();
        m.insert("accounts".into(), serde_json::to_value(accs).unwrap_or(Value::Null));
        m.insert("current_uid".into(), serde_json::to_value(ops::current_uid()).unwrap_or(Value::Null));
        m.insert("current_key".into(), serde_json::to_value(ops::current_account_key()).unwrap_or(Value::Null));
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

/// 查看某账号「最近对话消耗」（用量明细，只读）。limit<=0 时用默认 100。
#[tauri::command]
fn usage_summary(uid: String, limit: Option<i64>) -> String {
    ops::usage_summary(&uid, limit.unwrap_or(100))
}

/// 导出某账号「对话历史」（会话清单 + 可选消耗，只读）。返回 JSON 字符串。
#[tauri::command]
fn export_conversation_history(uid: String, include_deleted: Option<bool>, with_usage: Option<bool>) -> String {
    ops::export_conversation_history(&uid, include_deleted.unwrap_or(false), with_usage.unwrap_or(true))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if !ensure_single_instance() {
        eprintln!("WorkBuddy Account Hub 已在运行，本实例退出（单实例锁）");
        std::process::exit(0);
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_all,
            get_auth_path_report,
            delete_account,
            restore_account,
            list_ignored_accounts,
            preflight_accounts,
            refresh_account,
            refresh_all_accounts,
            get_exe_settings,
            save_exe_setting,
            pick_workbuddy_exe,
            get_quota,
            get_invite,
            enterprise_usage,
            send_webhook,
            growth_tasks,
            growth_task_claim,
            auto_refresh_sweep,
            copy_sessions,
            start_gateway,
            stop_gateway,
            gateway_status,
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
            pet_energy_all,
            pet_draw_for,
            list_accounts,
            snapshot_current,
            ensure_snapshot,
            backup_all,
            switch_account,
            restart_workbuddy,
            list_backups,
            backup_detail,
            list_custom_models,
            add_custom_model,
            update_custom_model,
            delete_custom_model,
            test_custom_model,
            official_models,
            current_model,
            quota_all,
            checkin_for,
            memory_all,
            usage_summary,
            export_conversation_history,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WorkBuddy Account Hub");
}

fn main() {
    run();
}
