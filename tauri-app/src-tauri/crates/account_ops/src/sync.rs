//! 跨机文件资产同步（Phase 1）——核心机制移植自 iushiyou/workbuddy-sync（MIT License）：
//!
//! - **per-machine 包**：每台机器只写自己的 `from-<机器名>.zip` 到共享目录（网盘/U盘），
//!   永不互相覆盖 —— 从根上避开网盘冲突副本与「一边盖掉另一边」
//! - **pull 合并**：远端包内容写入本机；本机同名文件内容不同时，先把本地版本备份为
//!   `<文件>.conflict-<对端机器>-<时间戳>` 再落新内容，不丢数据
//! - **白名单是数据**：同步范围写在 JSON manifest 里（默认内置，可放
//!   `~/.workbuddy-account-hub/sync-manifest.json` 覆盖），客户端升级新增资产改 JSON 即可
//! - **安全过滤**：never 段/后缀、敏感词文件名、单文件大小上限、zip-slip 防护双端生效
//!
//! Phase 1 只做文件资产（技能/记忆/自动化/对话正文/任务/产物索引等）；
//! workbuddy.db 会话行级合并留待 Phase 2（rusqlite ATTACH + INSERT OR IGNORE）。

use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

// ===== manifest（数据化的同步范围）=====

/// 内置默认 manifest。与 workbuddy-sync v1 对齐；用户可用
/// `~/.workbuddy-account-hub/sync-manifest.json` 整体覆盖（字段缺省时回落内置值）。
const DEFAULT_MANIFEST: &str = r#"{
  "manifest_version": 1,
  "sync_files": [
    "SOUL.md", "IDENTITY.md", "USER.md", "MEMORY.md",
    "settings.json", "models.json", "mcp.json",
    "plugins/installed_plugins.json", "plugins/known_marketplaces.json",
    "*/sidebar-list-snapshot.json"
  ],
  "sync_dirs": [
    "skills", "memory", "projects", "tasks", "storage", "artifact-index",
    "experts", "plugins/marketplaces/my-experts", "assistant-display",
    "todos", "plans", "deliverables", "brain", "project-resources",
    "plugin-account-state", "appearance-resources"
  ],
  "never_segments": [
    "workbuddy.db", "workbuddy.db-shm", "workbuddy.db-wal",
    "connectors", "binaries", "app", "logs", "sessions", "traces", "cache",
    "credentials", "auth", ".credentials.json", ".master.key", "security",
    "local-models", "blobs", "shell-snapshots", "vendor", "wbipc",
    "connectors-marketplace", "plugin-marketplace-state-new", "pending-telemetry",
    "node_modules", ".git", "__pycache__", ".venv", "venv", ".idea", ".vscode",
    "file-tree-manifests", "skill-cloud-sync", "editor-sdk-sandbox-ws",
    "workspace", "audit-log", "file-history", "clipboard-images", "local_storage",
    ".temp", "changes-detail", "changes-index", "keyblob", "user-server-cache"
  ],
  "never_suffix": [
    ".db", ".db-shm", ".db-wal", ".key", ".pem", ".pfx", ".tmp",
    ".log", ".swp", ".env"
  ],
  "sensitive_words": [
    "secret", "credential", "password", "passwd", "apikey", "api-key", "api_key",
    "token", "cookie", "auth-state", "private-key", "privatekey"
  ],
  "extra_globs": [],
  "max_file_mb": 200
}"#;

struct Manifest {
    sync_files: Vec<String>,
    sync_dirs: Vec<String>,
    never_segments: Vec<String>,
    never_suffix: Vec<String>,
    sensitive_words: Vec<String>,
    extra_globs: Vec<String>,
    max_file_mb: u64,
}

fn load_manifest() -> Manifest {
    let v: Value = std::fs::read_to_string(crate::hub_home().join("sync-manifest.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::from_str(DEFAULT_MANIFEST).expect("内置 manifest 必须合法"));
    let arr = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|i| i.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default()
    };
    Manifest {
        sync_files: arr("sync_files"),
        sync_dirs: arr("sync_dirs"),
        never_segments: arr("never_segments"),
        never_suffix: arr("never_suffix"),
        sensitive_words: arr("sensitive_words"),
        extra_globs: arr("extra_globs"),
        max_file_mb: v.get("max_file_mb").and_then(|x| x.as_u64()).unwrap_or(200),
    }
}

impl Manifest {
    /// 排除判定（与 workbuddy-sync is_excluded 同语义）：never 段/后缀、
    /// 备份/冲突遗留文件、extra_globs、文件名敏感词。
    fn is_excluded(&self, rel: &str) -> bool {
        let norm = rel.replace('\\', "/");
        if norm.split('/').any(|p| self.never_segments.iter().any(|n| n == p)) {
            return true;
        }
        let low = norm.to_lowercase();
        if self.never_suffix.iter().any(|s| low.ends_with(s)) {
            return true;
        }
        if low.contains(".bak") || low.contains(".conflict-") || low.ends_with('~') {
            return true;
        }
        if self.extra_globs.iter().any(|g| glob_match(g, &norm)) {
            return true;
        }
        self.sensitive_words.iter().any(|w| low.contains(w))
    }
}

// ===== 极简 glob（'*' / '?'，支持多段路径）=====

fn glob_match(pat: &str, s: &str) -> bool {
    glob_match_bytes(pat.as_bytes(), s.as_bytes())
}

fn glob_match_bytes(p: &[u8], s: &[u8]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    match p[0] {
        b'*' => {
            for i in 0..=s.len() {
                if glob_match_bytes(&p[1..], &s[i..]) {
                    return true;
                }
            }
            false
        }
        b'?' => !s.is_empty() && glob_match_bytes(&p[1..], &s[1..]),
        c => !s.is_empty() && s[0] == c && glob_match_bytes(&p[1..], &s[1..]),
    }
}

/// 把带通配符的白名单条目展开成实际路径（支持多段，如 `*/sidebar-list-snapshot.json`）
fn expand(base: &Path, pattern: &str) -> Vec<PathBuf> {
    let norm = pattern.replace('\\', "/");
    if !norm.contains('*') && !norm.contains('?') {
        return vec![base.join(&norm)];
    }
    glob_walk(base, &norm.split('/').collect::<Vec<_>>())
}

fn glob_walk(base: &Path, segs: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if segs.is_empty() {
        return out;
    }
    let Ok(rd) = std::fs::read_dir(base) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !glob_match(segs[0], &name) {
            continue;
        }
        let p = base.join(&name);
        if segs.len() == 1 {
            out.push(p);
        } else if p.is_dir() {
            out.extend(glob_walk(&p, &segs[1..]));
        }
    }
    out
}

/// 按白名单收集资产（相对路径, 绝对路径），排除规则与单文件大小上限同步生效
fn collect_assets(home: &Path, m: &Manifest) -> (Vec<(String, PathBuf)>, u64, u64) {
    let mut out = Vec::new();
    let mut total: u64 = 0;
    let mut skipped_sensitive = 0u64;
    let cap = m.max_file_mb.saturating_mul(1024 * 1024);

    for rel in &m.sync_files {
        for p in expand(home, rel) {
            if !p.is_file() {
                continue;
            }
            let r = match p.strip_prefix(home) {
                Ok(x) => x.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if m.is_excluded(&r) {
                skipped_sensitive += 1;
                continue;
            }
            if let Ok(md) = p.metadata() {
                if md.len() > cap {
                    continue;
                }
                total += md.len();
            }
            out.push((r, p));
        }
    }
    for top in &m.sync_dirs {
        for d in expand(home, top) {
            if !d.is_dir() {
                continue;
            }
            collect_dir_recursive(&d, home, m, cap, &mut out, &mut total, &mut skipped_sensitive);
        }
    }
    (out, total, skipped_sensitive)
}

fn collect_dir_recursive(
    dir: &Path,
    home: &Path,
    m: &Manifest,
    cap: u64,
    out: &mut Vec<(String, PathBuf)>,
    total: &mut u64,
    skipped_sensitive: &mut u64,
) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let rel = match p.strip_prefix(home) {
            Ok(x) => x.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        // 命中 never 段的整棵子树直接剪枝，避免深入 connector/cache 等大目录
        if rel.split('/').any(|seg| m.never_segments.iter().any(|n| n == seg)) {
            continue;
        }
        if p.is_dir() {
            collect_dir_recursive(&p, home, m, cap, out, total, skipped_sensitive);
            continue;
        }
        if !p.is_file() || m.is_excluded(&rel) {
            if p.is_file() && m.is_excluded(&rel) {
                *skipped_sensitive += 1;
            }
            continue;
        }
        let Ok(md) = p.metadata() else { continue };
        if md.len() > cap {
            continue;
        }
        *total += md.len();
        out.push((rel, p));
    }
}

// ===== 配置（共享目录 + 上次 push/pull 时间）=====

fn config_path() -> PathBuf {
    crate::hub_home().join("sync.json")
}

fn load_config() -> Value {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "share_dir": "", "last_push": 0, "last_pull": 0 }))
}

fn save_config(cfg: &Value) {
    let p = config_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(p, s);
    }
}

/// 默认共享目录建议：macOS 用 iCloud Drive，Windows 用 OneDrive（都不存在则空）
pub fn default_share_dir() -> String {
    let home = wb_home();
    if cfg!(target_os = "macos") {
        let p = home
            .join("Library")
            .join("Mobile Documents")
            .join("com~apple~CloudDocs")
            .join("wb-sync-hub");
        if p.parent().map(|d| d.exists()).unwrap_or(false) {
            return p.to_string_lossy().into_owned();
        }
        return String::new();
    }
    // Windows：OneDrive 根目录（env 由系统提供）
    if let Ok(od) = std::env::var("OneDrive") {
        return PathBuf::from(od).join("wb-sync-hub").to_string_lossy().into_owned();
    }
    String::new()
}

fn share_dir(cfg: &Value) -> Option<PathBuf> {
    let s = cfg.get("share_dir").and_then(|x| x.as_str()).unwrap_or("");
    if s.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn state_dir(share: &Path) -> PathBuf {
    share.join("hub-sync").join("state")
}

/// 机器标识（包名 from-<机器名>.zip 的来源），仅保留文件名安全字符。
/// 环境变量 `WBHUB_MACHINE_ID` 可覆盖（双机同名/测试场景用）。
pub fn machine_id() -> String {
    let raw = std::env::var("WBHUB_MACHINE_ID").unwrap_or_default();
    let raw = if raw.trim().is_empty() {
        if cfg!(target_os = "windows") {
            std::env::var("COMPUTERNAME").unwrap_or_default()
        } else {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        }
    } else {
        raw
    };
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() {
        if cfg!(target_os = "windows") { "win-pc".into() } else { "mac".into() }
    } else {
        clean
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fmt_ts(t: u64) -> String {
    if t == 0 {
        return "—".into();
    }
    // 本地时间字符串（不引入 chrono，粗粒度展示足够）
    std::process::Command::new("date")
        .arg("-r")
        .arg(t.to_string())
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| t.to_string())
}

// ===== push =====

/// 收集白名单资产 → 打 from-<机器名>.zip → 原子落盘到 <共享目录>/hub-sync/state/
pub fn sync_push() -> Result<Value, String> {
    let mut cfg = load_config();
    let share = share_dir(&cfg).ok_or("未设置共享同步目录，请先在设置里保存")?;
    let st = state_dir(&share);
    std::fs::create_dir_all(&st).map_err(|e| format!("创建同步目录失败: {e}"))?;

    let m = load_manifest();
    let home = wb_home();
    let (assets, total, skipped) = collect_assets(&home, &m);

    let machine = machine_id();
    let final_p = st.join(format!("from-{machine}.zip"));
    let tmp = st.join(format!("from-{machine}.zip.tmp"));

    let f = std::fs::File::create(&tmp).map_err(|e| format!("创建临时包失败: {e}"))?;
    let mut zw = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);

    let meta = json!({
        "machine": machine,
        "pushed_at": now_unix(),
        "manifest_version": 1,
        "file_count": assets.len(),
        "tool": "workbuddy-account-hub",
    });
    zw.start_file("meta.json", opts)
        .map_err(|e| format!("写入 meta 失败: {e}"))?;
    std::io::Write::write_all(&mut zw, serde_json::to_vec(&meta).unwrap_or_default().as_slice())
        .map_err(|e| format!("写入 meta 失败: {e}"))?;

    let mut failed: Vec<String> = Vec::new();
    for (rel, path) in &assets {
        let name = format!("files/{rel}");
        if zw.start_file(&name, opts).is_err() {
            failed.push(rel.clone());
            continue;
        }
        match std::fs::File::open(path).and_then(|mut f| std::io::copy(&mut f, &mut zw)) {
            Ok(_) => {}
            Err(_) => failed.push(rel.clone()),
        }
    }
    zw.finish().map_err(|e| format!("收尾 zip 失败: {e}"))?;
    std::fs::rename(&tmp, &final_p).map_err(|e| format!("落盘失败: {e}"))?;

    let ts = now_unix();
    cfg["last_push"] = json!(ts);
    save_config(&cfg);

    Ok(json!({
        "ok": true,
        "machine": machine,
        "package": final_p.to_string_lossy(),
        "files": assets.len(),
        "size_bytes": total,
        "size_human": du_human(total),
        "skipped_sensitive": skipped,
        "failed": failed,
        "pushed_at": ts,
    }))
}

// ===== pull =====

/// 扫描共享目录里其它机器的 from-*.zip，把远端资产合并进本机：
/// 新文件直接落；同名内容不同 → 本地版本先备份 `.conflict-<对端机器>-<ts>` 再写新值。
pub fn sync_pull() -> Result<Value, String> {
    let mut cfg = load_config();
    let share = share_dir(&cfg).ok_or("未设置共享同步目录，请先在设置里保存")?;
    let st = state_dir(&share);
    let m = load_manifest();
    let home = wb_home();
    let me = machine_id();
    let ts = now_unix();

    let rd = std::fs::read_dir(&st).map_err(|e| format!("读取同步目录失败: {e}"))?;
    let mut pkgs: Vec<PathBuf> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("from-") && name.ends_with(".zip") {
            let owner = name.trim_start_matches("from-").trim_end_matches(".zip");
            if owner != me {
                pkgs.push(e.path());
            }
        }
    }
    pkgs.sort();

    let mut per_machine: Vec<Value> = Vec::new();
    for pkg in &pkgs {
        let report = merge_package(pkg, &home, &m, ts);
        per_machine.push(report);
    }

    cfg["last_pull"] = json!(ts);
    save_config(&cfg);

    Ok(json!({ "ok": true, "pulled_at": ts, "machines": per_machine }))
}

fn merge_package(pkg: &Path, home: &Path, m: &Manifest, ts: u64) -> Value {
    let mut added = 0u64;
    let mut updated = 0u64;
    let mut conflicts = 0u64;
    let mut skipped_excluded = 0u64;
    let mut errors: Vec<String> = Vec::new();
    let mut machine = pkg
        .file_name()
        .map(|n| n.to_string_lossy().trim_start_matches("from-").trim_end_matches(".zip").to_string())
        .unwrap_or_default();

    let f = match std::fs::File::open(pkg) {
        Ok(f) => f,
        Err(e) => {
            return json!({ "machine": machine, "error": format!("打开包失败: {e}") });
        }
    };
    let mut archive = match zip::ZipArchive::new(f) {
        Ok(a) => a,
        Err(e) => return json!({ "machine": machine, "error": format!("解析 zip 失败: {e}") }),
    };

    // 先读 meta（拿到对端机器名，用于冲突备份命名）
    if let Ok(mut mf) = archive.by_name("meta.json") {
        let mut buf = Vec::new();
        if mf.read_to_end(&mut buf).is_ok() {
            if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                if let Some(mc) = v.get("machine").and_then(|x| x.as_str()) {
                    machine = mc.to_string();
                }
            }
        }
    }

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let raw_name = entry.name().to_string();
        let Some(rel) = raw_name.strip_prefix("files/") else { continue };
        if rel.is_empty() || rel.ends_with('/') {
            continue;
        }
        // zip-slip 防护：拒绝 .. 段、空段、绝对路径与盘符路径
        let norm = rel.replace('\\', "/");
        let unsafe_path = norm.starts_with('/')
            || norm.as_bytes().get(1) == Some(&b':')
            || norm.split('/').any(|seg| seg.is_empty() || seg == "..");
        if unsafe_path {
            errors.push(format!("{rel}: 非法路径"));
            continue;
        }
        if m.is_excluded(&norm) {
            skipped_excluded += 1;
            continue;
        }
        if entry.size() > m.max_file_mb.saturating_mul(1024 * 1024) {
            skipped_excluded += 1;
            continue;
        }
        // 远端包里被敏感词/never 规则挡下的内容不入本机（对端 manifest 可能更宽）
        let target = home.join(&norm);
        let Some(parent) = target.parent() else { continue };
        if std::fs::create_dir_all(parent).is_err() {
            errors.push(format!("{rel}: 创建目录失败"));
            continue;
        }

        let mut incoming = Vec::with_capacity(entry.size() as usize);
        if entry.read_to_end(&mut incoming).is_err() {
            errors.push(format!("{rel}: 读取失败"));
            continue;
        }

        match std::fs::read(&target) {
            Ok(local) if local == incoming => { /* 内容一致，幂等跳过 */ }
            Ok(local) => {
                // 内容不同：本地版本备份为 .conflict-<对端机器>-<ts>，再落新值
                let fname = target
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let bak = parent.join(format!("{fname}.conflict-{machine}-{ts}"));
                if std::fs::write(&bak, &local).is_ok() {
                    conflicts += 1;
                }
                if std::fs::write(&target, &incoming).is_ok() {
                    updated += 1;
                } else {
                    errors.push(format!("{rel}: 写入失败"));
                }
            }
            Err(_) => {
                if std::fs::write(&target, &incoming).is_ok() {
                    added += 1;
                } else {
                    errors.push(format!("{rel}: 写入失败"));
                }
            }
        }
    }

    json!({
        "machine": machine,
        "added": added,
        "updated": updated,
        "conflicts": conflicts,
        "skipped_excluded": skipped_excluded,
        "errors": errors,
    })
}

// ===== status =====

/// 同步自检：共享目录/本机包/对端包清单/资产规模
pub fn sync_status() -> Value {
    let cfg = load_config();
    let me = machine_id();
    let share = share_dir(&cfg);
    let m = load_manifest();
    let home = wb_home();
    let (assets, total, skipped) = collect_assets(&home, &m);

    let mut peers: Vec<Value> = Vec::new();
    let mut own: Value = json!(null);
    let last_pull = cfg.get("last_pull").and_then(|x| x.as_u64()).unwrap_or(0);

    if let Some(share) = &share {
        let st = state_dir(share);
        if let Ok(rd) = std::fs::read_dir(&st) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !(name.starts_with("from-") && name.ends_with(".zip")) {
                    continue;
                }
                let owner = name.trim_start_matches("from-").trim_end_matches(".zip").to_string();
                let md = e.metadata().ok();
                let mtime = md
                    .as_ref()
                    .and_then(|x| x.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let info = json!({
                    "machine": owner,
                    "size_bytes": md.as_ref().map(|x| x.len()).unwrap_or(0),
                    "size_human": du_human(md.as_ref().map(|x| x.len()).unwrap_or(0)),
                    "mtime": mtime,
                    "mtime_human": fmt_ts(mtime),
                    "is_new": owner != me && mtime > last_pull,
                });
                if owner == me {
                    own = info;
                } else {
                    peers.push(info);
                }
            }
        }
    }

    json!({
        "machine": me,
        "share_dir": share.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        "share_dir_set": share.is_some(),
        "default_share_dir": default_share_dir(),
        "last_push": cfg.get("last_push").and_then(|x| x.as_u64()).unwrap_or(0),
        "last_push_human": fmt_ts(cfg.get("last_push").and_then(|x| x.as_u64()).unwrap_or(0)),
        "last_pull": last_pull,
        "last_pull_human": fmt_ts(last_pull),
        "own": own,
        "peers": peers,
        "assets": {
            "files": assets.len(),
            "size_bytes": total,
            "size_human": du_human(total),
            "skipped_sensitive": skipped,
        },
    })
}

/// 设置共享同步目录（空串 = 清除）
pub fn set_share_dir(dir: &str) -> Result<Value, String> {
    let mut cfg = load_config();
    let d = dir.trim();
    if d.is_empty() {
        cfg["share_dir"] = json!("");
        save_config(&cfg);
        return Ok(json!({ "ok": true, "share_dir": "" }));
    }
    let p = PathBuf::from(d);
    if !p.is_dir() {
        std::fs::create_dir_all(&p).map_err(|e| format!("目录不存在且创建失败: {e}"))?;
    }
    cfg["share_dir"] = json!(p.to_string_lossy().into_owned());
    save_config(&cfg);
    Ok(json!({ "ok": true, "share_dir": p.to_string_lossy() }))
}

/// 人类可读容量（与 lib.rs du_human 语义一致，独立实现避免耦合内部结构）
fn du_human(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", units[u])
    }
}

/// WorkBuddy 数据根（manifest 白名单的基准目录）：~/.workbuddy
fn wb_home() -> PathBuf {
    crate::home().join(".workbuddy")
}
