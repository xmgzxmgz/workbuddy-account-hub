//! 全接口真实探针（ignore）：逐个调用 Hub 使用的所有只读接口，记录 HTTP 状态。
//! 写接口（真签到/派出/领奖/开盲盒）一律不调，避免副作用。
//! cargo test -p wb_api --test probe_all_apis -- --ignored --nocapture
//!
//! 背景（v0.6.28 排查）：客户端升到 5.9.3 后 asar 里 `/activity/growth` 下只剩
//! `buddy/info` 与 `buddy/travel/claim`——旧的 energy / buddy/quota / buddy/open /
//! travel/status / travel/config 都不在了，疑似整个宠物接口族被替换。
use serde_json::json;
use wb_api::{api_base, load_login, region_of_domain};

fn probe(client: &reqwest::blocking::Client, login: &wb_api::LoginInfo, method: &str, path: &str, body: &str) -> (u16, String) {
    let url = format!("{}{}", api_base(login), path);
    let req = if method == "POST" {
        client.post(&url).body(body.to_string())
    } else {
        client.get(&url)
    };
    let resp = req
        .header("Authorization", format!("Bearer {}", login.token))
        .header("X-User-Id", &login.uid)
        .header("X-Product", "SaaS")
        .header("X-IDE-Type", "WorkBuddy")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .send();
    match resp {
        Ok(r) => {
            let code = r.status().as_u16();
            let body = r.text().unwrap_or_default();
            let snip: String = body.chars().take(120).collect();
            (code, snip)
        }
        Err(e) => (0, format!("网络错误: {e}")),
    }
}

#[test]
#[ignore]
fn probe_all_readonly_apis() {
    let login = load_login().expect("读不到本机登录态");
    println!(
        "账号={}… domain={} region={} base={}",
        &login.uid[..8.min(login.uid.len())],
        login.domain,
        region_of_domain(&login.domain),
        api_base(&login)
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();

    // (名称, 方法, 路径, body)
    let cases: Vec<(&str, &str, &str, &str)> = vec![
        // ── 计费（Hub 在用）──
        ("额度 summary(无v2)",  "POST", "/billing/meter/get-user-resource-summary", "{}"),
        ("额度 summary(v2)",     "POST", "/v2/billing/meter/get-user-resource-summary", "{}"),
        ("额度 paid(无v2)",      "POST", "/billing/meter/get-user-resource-paid-packages", "{}"),
        ("额度 free(无v2)",      "POST", "/billing/meter/get-user-resource-free-packages", "{}"),
        ("企业用量",            "POST", "/v2/billing/meter/get-enterprise-user-usage", "{}"),
        // ── 签到（状态查询，无副作用）──
        ("签到活动状态 v2",      "POST", "/v2/billing/meter/checkin-activity-status", "{}"),
        ("签到活动状态 旧",      "POST", "/billing/meter/checkin-activity-status", "{}"),
        // ── 记忆 ──
        ("记忆 profile",        "GET",  "/api/memory/profile", ""),
        // ── 宠物（Hub 旧接口族，asar 5.9.3 已不含）──
        ("宠物能量 energy",     "GET",  "/activity/growth/energy", ""),
        ("宠物配额 buddy/quota", "GET",  "/activity/growth/buddy/quota", ""),
        ("盲盒 buddy/open",     "GET",  "/activity/growth/buddy/open", ""),
        ("旅行状态 travel/status", "GET", "/activity/growth/buddy/travel/status", ""),
        ("旅行配置 travel/config", "GET", "/activity/growth/buddy/travel/config", ""),
        // ── 宠物新接口（5.9.3 asar 里仅存的两个之一）──
        ("宠物聚合 buddy/info", "GET",  "/activity/growth/buddy/info", ""),
        // ── 模型目录 ──
        ("个人模型目录",        "GET",  "/v2/enterprises/personal/models", ""),
    ];

    println!("\n{:<24} {:<6} 状态  响应片段", "接口", "方法");
    println!("{}", "-".repeat(100));
    for (name, method, path, body) in &cases {
        let (code, snip) = probe(&client, &login, method, path, body);
        let mark = if code == 200 { "✓" } else if code == 404 { "✗ 已消失" } else { "?" };
        println!("{:<24} {:<6} {:<4} {} {:<10}", name, method, code, mark, snit(&snip));
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let _ = json!({});
}

fn snit(s: &str) -> String {
    let t: String = s.chars().take(90).collect();
    t.replace('\n', " ")
}

#[test]
#[ignore]
fn probe_growth_with_personal_form() {
    // 用个人版快照的 token 测宠物接口（当前登录态是企业版 → 成长系统 403 属预期）。
    // 个人版快照路径固定为 vault/<uid>/snapshot/auth.info（账号键改造后的约定）。
    let home = std::env::var("HOME").unwrap_or_default();
    let p = std::path::PathBuf::from(&home)
        .join(".workbuddy-account-hub/vault/4c6af085-6a28-4c45-8025-099590dac28a/snapshot/auth.info");
    let Some(login) = wb_api::login_from_file(&p) else {
        println!("个人版快照不存在: {}", p.display());
        return;
    };
    println!("个人版 token: uid={}…", &login.uid[..8]);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();
    for (name, method, path) in [
        ("宠物能量 energy", "GET", "/activity/growth/energy"),
        ("宠物配额 buddy/quota", "GET", "/activity/growth/buddy/quota"),
        ("盲盒(POST-only路径) buddy/open", "GET", "/activity/growth/buddy/open"),
        ("旅行状态 travel/status", "GET", "/activity/growth/buddy/travel/status"),
        ("旅行配置 travel/config", "GET", "/activity/growth/buddy/travel/config"),
        ("宠物聚合 buddy/info", "GET", "/activity/growth/buddy/info"),
        ("额度 summary(个人版)", "POST", "/billing/meter/get-user-resource-summary"),
    ] {
        let (code, snip) = probe(&client, &login, method, path, "{}");
        let mark = if code == 200 { "✓" } else if code == 404 { "✗ 已消失" } else { "?" };
        println!("{:<28} {:<4} {} {:<10}", name, code, mark, snit(&snip));
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[test]
#[ignore]
fn probe_invite_and_usage() {
    // v0.6.28 新功能探针：邀请码 / 企业用量（均在 5.9.3 asar 中出现且后者已实测 200）
    let home = std::env::var("HOME").unwrap_or_default();
    let p = std::path::PathBuf::from(&home)
        .join(".workbuddy-account-hub/vault/4c6af085-6a28-4c45-8025-099590dac28a/snapshot/auth.info");
    let Some(login) = wb_api::login_from_file(&p) else { println!("快照不存在"); return; };
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();
    for (name, method, path, body) in [
        ("邀请码 my-code", "GET", "/activity/workbuddy/invitation/v2/my-code", ""),
        ("邀请记录 invite-records", "GET", "/activity/workbuddy/invitation/v2/invite-records?page=1&size=5", ""),
        ("活动中心 activity/query", "GET", "/activity/query", ""),
    ] {
        let (code, snip) = probe(&client, &login, method, path, body);
        println!("{:<26} {:<4} {}", name, code, snit(&snip));
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}
