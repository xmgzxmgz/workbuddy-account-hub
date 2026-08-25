//! WorkBuddy 自定义模型 / 官方渠道 / 当前模型 管理
//!
//! 关键事实（2026-08-19 逆向确认）：
//!   - WorkBuddy 桌面端真实读取的模型配置 = `~/.workbuddy/models.json`（纯数组，每项一个模型）
//!     （`~/.codebuddy/models.json` 是 CLI/CodeBuddy 的配置，WorkBuddy 不读）
//!   - 官方渠道模型（deepseek-v4-flash / gemini / claude / gpt-5 / GLM / qwen 等）由 WorkBuddy
//!     内置/服务端提供，本地无文件可改，仅能展示
//!   - 「当前使用模型」存在 WorkBuddy 的 localStorage（session/Local Storage/leveldb），无官方
//!     配置入口；这里用二进制探测「尽力而为」地识别，只读不写
//!   - 「切换 API」= 写入 ~/.workbuddy/models.json（WorkBuddy 官方读取）+ 重启 WorkBuddy 生效

use serde_json::{json, Value};
use std::path::PathBuf;

/// WorkBuddy 产品模型配置文件路径（兼容 WORKBUDDY_CONFIG_DIR）
pub fn models_path() -> PathBuf {
    if let Ok(d) = std::env::var("WORKBUDDY_CONFIG_DIR") {
        let p = PathBuf::from(d.trim());
        if !d.trim().is_empty() && p.exists() {
            return p.join("models.json");
        }
    }
    if let Ok(d) = std::env::var("CODEBUDDY_CONFIG_DIR") {
        let p = PathBuf::from(d.trim());
        if !d.trim().is_empty() && p.exists() {
            return p.join("models.json");
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".workbuddy").join("models.json")
}

/// 读 WorkBuddy 自定义模型列表（数组），apiKey 掩码后返回
pub fn list_custom() -> Value {
    let path = models_path();
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            return json!({
                "ok": false, "error": format!("读取失败: {} ({})", path.display(), e),
                "path": path.display().to_string(), "models": []
            });
        }
    };
    let v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return json!({
                "ok": false, "error": format!("JSON 解析失败: {}", e),
                "path": path.display().to_string(), "models": []
            });
        }
    };
    let arr = match v.as_array() {
        Some(a) => a,
        None => {
            return json!({
                "ok": false, "error": "格式异常：期望 JSON 数组",
                "path": path.display().to_string(), "models": []
            });
        }
    };
    let masked: Vec<Value> = arr.iter().map(mask_model).collect();
    json!({
        "ok": true, "path": path.display().to_string(), "models": masked,
        "count": masked.len()
    })
}

/// 掩码 apiKey（只留前 6 后 4）。
/// ⚠️ 不再返回 apiKeyFull 明文——本地 IPC 也按「掩码即掩码」处理，明文 key 仅存于
/// ~/.workbuddy/models.json 源文件，前端展示一律用掩码（旧版返回明文字段与掩码设计矛盾）。
fn mask_model(m: &Value) -> Value {
    let mut out = m.clone();
    let key = out
        .get("apiKey")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    if !key.is_empty() {
        out["apiKey"] = json!(mask_key(&key));
        out["_hasKey"] = json!(true);
    } else {
        out["_hasKey"] = json!(false);
    }
    out
}

pub fn mask_key(k: &str) -> String {
    if k.len() <= 10 {
        return "****".to_string();
    }
    format!("{}…{}", &k[..6], &k[k.len() - 4..])
}

/// 保存自定义模型列表（整体写回）。带时间戳备份 + 原子写（临时文件 + rename）。
/// 注意：models 参数必须是未掩码的完整模型对象。
pub fn save_custom(models: &[Value]) -> Result<Value, String> {
    let path = models_path();
    // 备份
    let ts = chrono_like_ts();
    let bak = path.with_file_name(format!(
        "models.json.bak-{}-{}",
        ts, std::process::id()
    ));
    if path.exists() {
        std::fs::copy(&path, &bak).map_err(|e| format!("备份失败: {}", e))?;
    }
    // 原子写
    let tmp = path.with_extension("json.tmp");
    let payload = serde_json::to_string_pretty(models)
        .map_err(|e| format!("序列化失败: {}", e))?;
    std::fs::write(&tmp, payload).map_err(|e| format!("写入失败: {}", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换失败: {}", e))?;
    Ok(json!({
        "ok": true, "path": path.display().to_string(),
        "count": models.len(), "backup": bak.display().to_string()
    }))
}

/// 读原始模型数组（未掩码）
fn read_raw() -> Result<Vec<Value>, String> {
    let path = models_path();
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {} ({})", path.display(), e))?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("JSON 解析失败: {}", e))?;
    v.as_array()
        .cloned()
        .ok_or_else(|| "格式异常：期望 JSON 数组".to_string())
}

/// 新增自定义模型（id 唯一校验）
pub fn add_model(m: Value) -> Result<Value, String> {
    let mut list = read_raw()?;
    let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if id.is_empty() {
        return Err("模型 ID 不能为空".into());
    }
    if list.iter().any(|x| x.get("id").and_then(|v| v.as_str()) == Some(id.as_str())) {
        return Err(format!("已存在同 ID 的模型: {}", id));
    }
    list.push(m);
    save_custom(&list)
}

/// 更新自定义模型（按 id；patch 中 apiKey 为空/掩码值则保留原 key）
pub fn update_model(id: &str, patch: Value) -> Result<Value, String> {
    let mut list = read_raw()?;
    let idx = list
        .iter()
        .position(|x| x.get("id").and_then(|v| v.as_str()) == Some(id))
        .ok_or_else(|| format!("未找到模型: {}", id))?;
    let old = list[idx].clone();
    let mut new = old;
    if let Some(obj) = patch.as_object() {
        for (k, v) in obj {
            if k == "id" {
                // id 不允许通过 patch 修改（用它定位）；如确实要改走「删除+新增」
                continue;
            }
            if k == "apiKey" {
                let s = v.as_str().unwrap_or("");
                // 空、或仍是掩码形态 → 保留原 key
                if s.is_empty() || s.contains('…') || s == "****" {
                    continue;
                }
                new["apiKey"] = v.clone();
            } else {
                new[k] = v.clone();
            }
        }
    }
    list[idx] = new;
    save_custom(&list)
}

/// 删除自定义模型（按 id）
pub fn delete_model(id: &str) -> Result<Value, String> {
    let mut list = read_raw()?;
    let before = list.len();
    list.retain(|x| x.get("id").and_then(|v| v.as_str()) != Some(id));
    if list.len() == before {
        return Err(format!("未找到模型: {}", id));
    }
    save_custom(&list)
}

/// 简单时间戳（YYYYMMDD-HHMMSS），避免引入 chrono
fn chrono_like_ts() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 粗略本地时间（UTC+8）
    let secs = secs + 8 * 3600;
    let days = secs / 86400;
    let rem = secs % 86400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 测试模型连通性：POST /chat/completions（短超时、max_tokens=8）
pub fn test_model(m: &Value) -> Value {
    let url = m.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut key = m.get("apiKey").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    // ⚠️ 修复：列表「测试」传入的是掩码后的模型（apiKey 为 "sk-xxx…yyyy"），直接用它请求必 401。
    // 若 key 缺失或仍是掩码形态，按 id 从 models.json 读完整 key 再测（表单测试传的是完整 key，不受影响）。
    if !id.is_empty() && (key.is_empty() || key.contains('…') || key == "****") {
        if let Ok(list) = read_raw() {
            if let Some(full) = list.iter().find(|x| x.get("id").and_then(|v| v.as_str()) == Some(id.as_str())) {
                if let Some(k) = full.get("apiKey").and_then(|v| v.as_str()) {
                    key = k.to_string();
                }
            }
        }
    }
    if url.is_empty() || key.is_empty() {
        return json!({"ok": false, "error": "缺少 url 或 apiKey（该模型未保存完整 key，请在编辑表单中填写后再测试）"});
    }
    let t = std::time::Instant::now();
    // 复用 lib.rs 的统一客户端（含 15s 超时 + HTTPS_PROXY/HTTP_PROXY 代理），
    // 与其它官方接口请求行为一致——旧版在此自建裸 client，外网模型在代理环境下测不通。
    let client = super::make_client();
    // url 可能是完整 chat/completions 或 base（如 /v1）
    let endpoint = if url.ends_with("/chat/completions") {
        url.clone()
    } else {
        format!("{}/chat/completions", url.trim_end_matches('/'))
    };
    let body = json!({
        "model": id,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 8
    });
    let resp = client
        .post(&endpoint)
        .header("Authorization", format!("Bearer {}", key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send();
    match resp {
        Ok(r) => {
            let status = r.status().as_u16();
            let elapsed = t.elapsed().as_millis();
            let text = r.text().unwrap_or_default();
            if status == 200 {
                if let Ok(j) = serde_json::from_str::<Value>(&text) {
                    let model = j.get("model").cloned().unwrap_or(Value::Null);
                    return json!({
                        "ok": true, "status": status, "ms": elapsed,
                        "model": model, "note": "chat/completions 200"
                    });
                }
                json!({"ok": true, "status": status, "ms": elapsed, "note": "HTTP 200"})
            } else {
                json!({
                    "ok": false, "status": status, "ms": elapsed,
                    "error": text.chars().take(200).collect::<String>()
                })
            }
        }
        Err(e) => json!({"ok": false, "error": format!("请求失败: {}", e)}),
    }
}

/// WorkBuddy 官方渠道模型清单（内置展示，WorkBuddy 服务端提供、本地不可编辑）。
/// ⚠️ 此清单为**内置静态列表**（旧注释误称"从 localStorage 探测"，实际为硬编码，
/// 探测仅用于 current_model 判断"当前使用的是哪个"）。
pub fn official_list() -> Value {
    json!([
        {"id":"deepseek-v4-flash","name":"DeepSeek V4 Flash","vendor":"官方 · direct","builtin":true},
        {"id":"gemini-2.5-pro","name":"Gemini 2.5 Pro","vendor":"Google · 官方","builtin":true},
        {"id":"claude-opus-4-8","name":"Claude Opus 4.8","vendor":"Anthropic · 官方","builtin":true},
        {"id":"gpt-5.6","name":"GPT-5.6","vendor":"OpenAI · 官方","builtin":true},
        {"id":"glm-5.2.1","name":"GLM-5.2.1","vendor":"智谱 · 官方","builtin":true},
        {"id":"qwen-max","name":"Qwen Max","vendor":"通义 · 官方","builtin":true},
        {"id":"qwen-plus","name":"Qwen Plus","vendor":"通义 · 官方","builtin":true}
    ])
}

/// 探测「当前使用模型」：扫描 WorkBuddy localStorage leveldb 二进制，返回命中的官方模型 id
/// （只读，尽力而为；找不到返回 null）。跨平台候选目录：
///   - macOS:   ~/.workbuddy/app/session/Local Storage/leveldb
///   - Windows: %APPDATA%/WorkBuddy/app/session/Local Storage/leveldb 等
pub fn current_model() -> Value {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(&home).join(".workbuddy").join("app").join("session"));
    }
    if cfg!(target_os = "windows") {
        for env in ["APPDATA", "LOCALAPPDATA"] {
            if let Ok(d) = std::env::var(env) {
                let base = PathBuf::from(&d);
                dirs.push(base.join("WorkBuddy").join("app").join("session"));
                dirs.push(base.join("CodeBuddyExtension").join("app").join("session"));
            }
        }
    }
    let officials: Vec<&str> = vec![
        "deepseek-v4-flash", "gemini-2.5-pro", "claude-opus-4-8", "gpt-5.6",
        "glm-5.2.1", "qwen-max", "qwen-plus",
    ];
    let mut hit: Option<String> = None;
    'outer: for base in dirs {
        let dir = base.join("Local Storage").join("leveldb");
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                if !name.ends_with(".ldb") && !name.ends_with(".log") {
                    continue;
                }
                let Ok(buf) = std::fs::read(ent.path()) else { continue };
                for o in &officials {
                    if buf.windows(o.len()).any(|w| w == o.as_bytes()) {
                        hit = Some(o.to_string());
                        break;
                    }
                }
                if hit.is_some() { break 'outer; }
            }
        }
    }
    match hit {
        Some(id) => json!({"ok": true, "model_id": id, "source": "localStorage 探测"}),
        None => json!({"ok": false, "model_id": null, "source": "未探测到"}),
    }
}
