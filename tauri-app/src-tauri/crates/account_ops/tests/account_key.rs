//! 账号键（个人版 / 企业版）逻辑测试 —— v0.6.24
//!
//! 背景：同一个 OneID 账号可同时拥有个人版与企业版身份，**uid 完全相同**，
//! 只差 account.type（personal / ultimate）与 account.enterpriseId。
//! 此前 vault 用 uid 作目录名，两形态共用一个槽位 → 切一次覆盖一份。
//! 现在目录名改用 accountKey：`<uid>` 或 `<uid>@<enterpriseId>`。
//!
//! cargo test -p account_ops --test account_key
use account_ops::{account_key, migrate_legacy_snapshots, parse_key};
use serde_json::json;
use std::path::PathBuf;

const UID: &str = "4c6af085-6a28-4c45-8025-099590dac28a";
const EID: &str = "g2qqhhq47u2o";

#[test]
fn key_roundtrip_personal_and_enterprise() {
    // 个人版：无 enterpriseId
    let k1 = account_key(UID, None);
    assert_eq!(k1, UID);
    assert_eq!(parse_key(&k1), (UID, None));

    // 企业版：带 enterpriseId
    let k2 = account_key(UID, Some(EID));
    assert_eq!(k2, format!("{UID}@{EID}"));
    assert_eq!(parse_key(&k2), (UID, Some(EID)));
}

#[test]
fn empty_or_blank_enterprise_id_means_personal() {
    // 客户端的 enterpriseId 常以空串出现（而非缺失），必须当作个人版，
    // 否则会生出一个 "<uid>@" 的畸形目录键
    assert_eq!(account_key(UID, Some("")), UID);
    assert_eq!(account_key(UID, Some("   ")), UID);
    assert_eq!(account_key(UID, None), UID);
}

#[test]
fn key_of_account_reads_enterprise_id_from_account_json() {
    let personal = json!({"uid": UID, "type": "personal"});
    let ent = json!({"uid": UID, "type": "ultimate", "enterpriseId": EID});
    let ent_blank = json!({"uid": UID, "type": "ultimate", "enterpriseId": ""});
    assert_eq!(account_ops::key_of_account(&personal).unwrap(), UID);
    assert_eq!(account_ops::key_of_account(&ent).unwrap(), format!("{UID}@{EID}"));
    assert_eq!(account_ops::key_of_account(&ent_blank).unwrap(), UID);
    assert_eq!(account_ops::key_of_account(&json!({})), None);
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("hub-key-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn write_snap(vault: &PathBuf, dir: &str, acc: serde_json::Value) {
    let d = vault.join(dir).join("snapshot");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("auth.info"), json!({"auth": {}, "account": acc}).to_string()).unwrap();
}

#[test]
fn legacy_enterprise_snapshot_is_renamed_to_key_dir() {
    let vault = tmp("mig");
    // v0.6.23 及以前：企业版快照也躺在裸 uid 目录里
    write_snap(&vault, UID, json!({"uid": UID, "type": "ultimate", "enterpriseId": EID}));
    // 个人版账号不该被动
    let other = "3913c7dd-6a14-4095-8a29-17c29f00b6d3";
    write_snap(&vault, other, json!({"uid": other, "type": "personal"}));

    migrate_legacy_snapshots(&vault);

    assert!(!vault.join(UID).exists(), "企业版快照应已从裸 uid 目录移走");
    assert!(vault.join(format!("{UID}@{EID}")).exists(), "应改名为 <uid>@<eid>");
    assert!(vault.join(other).exists(), "个人版目录不应被移动");
    let _ = std::fs::remove_dir_all(&vault);
}

#[test]
fn migration_is_idempotent_and_never_overwrites_existing() {
    let vault = tmp("mig2");
    // 目标目录已存在（已有企业版快照）→ 保守跳过，不覆盖
    write_snap(&vault, UID, json!({"uid": UID, "type": "ultimate", "enterpriseId": EID}));
    write_snap(&vault, &format!("{UID}@{EID}"), json!({"uid": UID, "type": "ultimate", "enterpriseId": EID}));
    let before = std::fs::metadata(vault.join(&format!("{UID}@{EID}")).join("snapshot").join("auth.info"))
        .map(|m| m.len()).unwrap_or(0);
    migrate_legacy_snapshots(&vault);
    assert!(vault.join(UID).exists(), "目标已存在时应保留原样，不覆盖");
    let after = std::fs::metadata(vault.join(&format!("{UID}@{EID}")).join("snapshot").join("auth.info"))
        .map(|m| m.len()).unwrap_or(0);
    assert_eq!(before, after, "既有企业版快照不应被改动");

    // 再跑一次仍然幂等
    migrate_legacy_snapshots(&vault);
    assert!(vault.join(UID).exists());
    let _ = std::fs::remove_dir_all(&vault);
}
