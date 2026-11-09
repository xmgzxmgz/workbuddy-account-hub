//! 账号删除 / 恢复测试（issue：失效账号需要清理）。
//!
//! 全程使用临时 vault，不触碰真实账号数据；但忽略表是全局文件，
//! 测试前后会备份/还原它，避免污染本机配置。
//!
//! cargo test -p account_ops --test delete_account
use account_ops::{
    delete_account, ignored_uids, list_ignored, restore_account, vault_dir,
};
use std::path::PathBuf;

/// 忽略表是全局文件，多个测试并行会互相覆盖它（写完再被下一个测试的 Drop 还原），
/// 所以这里用全局 Mutex 串行化；panic 后 poisoned 也要能继续取锁。
static GLOBAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    GLOBAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// 含 '-' 才会被 vault 扫描认作账号目录
const TEST_UID: &str = "deadbeef-0000-1111-2222-333333333333";

fn setup_tmp(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("hub-del-test-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&base);
    let acc = base.join(TEST_UID).join("snapshot");
    std::fs::create_dir_all(&acc).unwrap();
    std::fs::write(acc.join("auth.info"), r#"{"auth":{"accessToken":"x","domain":"www.workbuddy.cn"},"account":{"uid":"deadbeef-0000-1111-2222-333333333333"}}"#).unwrap();
    // 造一份历史备份，验证是否一起搬走
    let hist = base.join(TEST_UID).join("history").join("1700000000000");
    std::fs::create_dir_all(&hist).unwrap();
    std::fs::write(hist.join("auth.info"), "{}").unwrap();
    base
}

fn ignored_path() -> PathBuf {
    account_ops::hub_home().join("ignored_accounts.json")
}

struct IgnoredGuard(Option<String>);
impl IgnoredGuard {
    fn new() -> Self {
        IgnoredGuard(std::fs::read_to_string(ignored_path()).ok())
    }
}
impl Drop for IgnoredGuard {
    fn drop(&mut self) {
        let p = ignored_path();
        match &self.0 {
            Some(s) => { let _ = std::fs::write(&p, s); }
            None => { let _ = std::fs::remove_file(&p); }
        }
    }
}

#[test]
fn delete_moves_to_trash_hides_from_list_and_can_restore() {
    let _serial = serial();
    let _guard = IgnoredGuard::new();
    let tmp = setup_tmp("trash");

    // 删除前：目录存在
    assert!(tmp.join(TEST_UID).exists());

    let r = delete_account(&tmp, TEST_UID).expect("删除应成功");
    assert!(r.history_moved, "历史备份应一起移入回收站");

    // 1) 原目录没了，但回收站里有（不是永久删除）
    assert!(!tmp.join(TEST_UID).exists(), "原快照目录应被移走");
    assert!(PathBuf::from(&r.moved_to).exists(), "回收站里应存在：{}", r.moved_to);
    assert!(PathBuf::from(&r.moved_to).join("history").exists(), "历史备份应在回收站内");

    // 2) 已被忽略（这是「删了还在列表里」的关键——只删目录不够）
    assert!(ignored_uids().contains(TEST_UID), "uid 应记入忽略表");
    let listed = list_ignored(&tmp);
    assert!(listed.iter().any(|x| x["uid"] == TEST_UID), "应出现在已删除清单里");

    // 3) .trash 不会被当成账号扫回来
    let accs = account_ops::list_accounts(&tmp);
    assert!(
        !accs.iter().any(|a| a.uid == TEST_UID),
        "删除后的账号不应再出现在账号列表"
    );

    // 4) 还原：解除忽略 + 快照移回
    let rr = restore_account(&tmp, TEST_UID).expect("还原应成功");
    assert_eq!(rr["restored"], true, "应真的把快照移回来");
    assert!(tmp.join(TEST_UID).exists(), "快照应回到原位");
    assert!(!ignored_uids().contains(TEST_UID), "应解除忽略");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn deleting_current_account_is_rejected() {
    let _serial = serial();
    let _guard = IgnoredGuard::new();
    let tmp = setup_tmp("cur");
    // 当前登录「形态」必须拒绝删除。注意用 current_account_key 而非 current_uid：
    // 企业版会话下当前键是 <uid>@<eid>，此时删除个人版形态（纯 uid）是合法的——
    // 保护只针对正在使用的那一份登录态（v0.6.27 语义）。
    if let Some(cur) = account_ops::current_account_key() {
        let err = delete_account(&tmp, &cur).expect_err("当前账号键应被拒绝");
        assert!(err.contains("当前登录账号"), "错误信息应说明原因，实际: {err}");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn delete_nonexistent_uid_only_hides() {
    let _serial = serial();
    let _guard = IgnoredGuard::new();
    let tmp = setup_tmp("nosnap");
    // 没有快照的账号（只存在于登记表）也能删 —— 只做隐藏
    let r = delete_account(&tmp, "cafebabe-1111-2222-3333-444444444444").expect("应成功");
    assert_eq!(r.moved_to, "", "无快照时不应产生回收站条目");
    assert!(ignored_uids().contains("cafebabe-1111-2222-3333-444444444444"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
#[ignore = "依赖本机真实 vault，CI runner 上没有账号，会误报失败"]
fn real_vault_still_lists_accounts() {
    // 冒烟：过滤逻辑不能把真实账号误伤（本地手动跑：-- --ignored）
    let accs = account_ops::list_accounts(&vault_dir());
    println!("真实账号数: {}", accs.len());
    assert!(!accs.is_empty(), "真实 vault 应至少有一个账号（过滤不应全部删掉）");
}
