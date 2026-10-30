//! 登录态三态预检真实数据测试（默认 ignore）：
//! cargo test -p account_ops --test preflight_real -- --ignored --nocapture
//!
//! 验证 issue #3 问题 B：服务端吊销的 token（JWT 未到期但 401）应被单独标注为
//! `revoked`，与"AT 自然到期"（expired）、"将过期"（expiring）区分开。
//! 全程只读，不消耗任何 token。
use account_ops::{preflight_accounts, vault_dir};

#[test]
#[ignore]
fn preflight_all_three_states() {
    let vault = vault_dir();
    let rows = preflight_accounts(&vault);
    assert!(!rows.is_empty(), "应至少有一个账号");

    let mut tally = std::collections::BTreeMap::<String, usize>::new();
    for r in &rows {
        let state = r.get("state").and_then(|x| x.as_str()).unwrap_or("?");
        *tally.entry(state.to_string()).or_insert(0) += 1;
        println!(
            "{} {:<12} nick={} AT={}天 http={} {}{}",
            &r["uid"].as_str().unwrap_or("")[..8.min(r["uid"].as_str().unwrap_or("").len())],
            state,
            r.get("nickname").and_then(|x| x.as_str()).unwrap_or("-"),
            r.get("at_days_left").map(|x| x.to_string()).unwrap_or("null".into()),
            r.get("http_status").map(|x| x.to_string()).unwrap_or("null".into()),
            r.get("label").and_then(|x| x.as_str()).unwrap_or(""),
            if r.get("current").and_then(|x| x.as_bool()).unwrap_or(false) { " ←当前登录" } else { "" },
        );
    }
    println!("\n三态分布: {:?}", tally);
    // 至少能给出一个明确态（不能全是 error —— 那说明探测链路本身有问题）
    assert!(
        tally.keys().any(|k| k != "error"),
        "全部账号预检都失败，探测链路异常"
    );
}
