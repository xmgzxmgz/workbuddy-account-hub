//! RT 静默续期真实数据测试（默认 ignore）：
//! REFRESH_UID=<uid> cargo test -p account_ops --test refresh_real -- --ignored --nocapture
//!
//! ⚠️ 会真实消耗/轮换目标账号的 refreshToken，且改写 vault 快照（含备份）。
//! 只应对"已废弃或可牺牲"的账号执行；未设置 REFRESH_UID 时直接跳过。
use account_ops::{preflight_accounts, refresh_account, vault_dir};

#[test]
#[ignore]
fn refresh_token_real() {
    let Ok(uid) = std::env::var("REFRESH_UID") else {
        println!("未设置 REFRESH_UID，跳过（这是保护：续期会真实轮换 RT）");
        return;
    };
    let vault = vault_dir();

    // 续期前状态
    let before = preflight_accounts(&vault)
        .into_iter()
        .find(|r| r.get("uid").and_then(|x| x.as_str()) == Some(uid.as_str()))
        .expect("应能找到目标账号");
    println!("续期前: state={} label={} http={}",
        before["state"], before["label"], before["http_status"]);

    match refresh_account(&vault, &uid) {
        Ok(r) => {
            println!("✅ {}", r.message);
            println!("   AT {:.1} → {:.1} 天 · RT 剩 {:.1} 天 · rotated={}",
                r.at_days_before, r.at_days_after, r.rt_days_after, r.rotated);
            println!("   备份: {}", r.backup);
            assert!(r.at_days_after > r.at_days_before, "续期后 AT 剩余天数应增加");
        }
        Err(e) => {
            println!("❌ 续期失败: {e}");
            // 失败不是测试失败：401 说明 RT 也被吊销，这是有效结论
        }
    }

    // 续期后状态（复检三态是否从 revoked 变 ok）
    let after = preflight_accounts(&vault)
        .into_iter()
        .find(|r| r.get("uid").and_then(|x| x.as_str()) == Some(uid.as_str()))
        .expect("应能找到目标账号");
    println!("续期后: state={} label={} http={}",
        after["state"], after["label"], after["http_status"]);
}
