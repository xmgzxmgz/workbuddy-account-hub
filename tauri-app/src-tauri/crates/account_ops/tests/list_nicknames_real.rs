//! list_accounts 昵称解析真实数据测试（默认 ignore）：
//! cargo test -p account_ops --test list_nicknames_real -- --ignored --nocapture
//!
//! 验证 v0.6.14 修复：vault 快照为 $wbEncrypted 加密格式时，列表昵称
//! 应通过批量内存解密解析出来（绝不回写快照文件）。
use account_ops::list_accounts;

#[test]
#[ignore]
fn list_accounts_real_nicknames() {
    let vault = account_ops::vault_dir();
    let accounts = list_accounts(&vault);
    assert!(!accounts.is_empty(), "应至少枚举出一个账号");
    let mut with_nick = 0;
    for a in &accounts {
        let nick = a.nickname.as_deref().unwrap_or("");
        let has = !nick.is_empty();
        if has { with_nick += 1; }
        println!(
            "{}: nick={} current={} snapshot={}",
            a.uid,
            if has { nick } else { "(无昵称)" },
            a.current,
            a.has_snapshot
        );
    }
    assert!(with_nick > 0, "至少应有一个账号解析出昵称（明文或解密）");
}
