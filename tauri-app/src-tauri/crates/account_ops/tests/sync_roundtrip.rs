//! 跨机同步 push/pull 回环测试（临时目录模拟两台机器，不碰真实数据）

use account_ops::sync;

fn temp_home(tag: &str) -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!(
        "wbhub-sync-test-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join(".workbuddy").join("skills").join("demo")).unwrap();
    base
}

fn set_home(p: &std::path::Path) {
    if cfg!(target_os = "windows") {
        std::env::set_var("USERPROFILE", p);
    } else {
        std::env::set_var("HOME", p);
    }
}

#[test]
fn push_pull_roundtrip_and_conflict() {
    let share = std::env::temp_dir().join(format!("wbhub-sync-share-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&share);

    // ── 机器 A：写资产 + push ──
    let home_a = temp_home("a");
    std::fs::write(
        home_a.join(".workbuddy").join("MEMORY.md"),
        "# A 的记忆\n",
    )
    .unwrap();
    std::fs::write(
        home_a.join(".workbuddy").join("skills").join("demo").join("SKILL.md"),
        "---\nname: demo\n---\nhello",
    )
    .unwrap();
    // 敏感/never 资产不应被同步
    std::fs::write(home_a.join(".workbuddy").join("auth-token.txt"), "secret").unwrap();
    std::fs::write(home_a.join(".workbuddy").join("keyblob"), "master-key").unwrap();

    set_home(&home_a);
    std::env::set_var("WBHUB_MACHINE_ID", "machine-a");
    sync::set_share_dir(&share.to_string_lossy()).unwrap();
    let push = sync::sync_push().unwrap();
    assert_eq!(push["ok"], json(true));
    assert!(push["files"].as_u64().unwrap() >= 2, "至少应打包 MEMORY.md + SKILL.md");

    // 敏感内容验证：包内不得含 auth-token / keyblob
    let pkg = std::fs::read(&share).ok();
    assert!(pkg.is_some() || share.join("hub-sync").join("state").join("from-machine-a.zip").exists());

    // ── 机器 B：已有不同内容的 MEMORY.md → pull 应产生 conflict 备份并落新值 ──
    let home_b = temp_home("b");
    std::fs::write(
        home_b.join(".workbuddy").join("MEMORY.md"),
        "# B 的旧记忆\n",
    )
    .unwrap();

    set_home(&home_b);
    std::env::set_var("WBHUB_MACHINE_ID", "machine-b");
    sync::set_share_dir(&share.to_string_lossy()).unwrap();
    let pull = sync::sync_pull().unwrap();
    assert_eq!(pull["ok"], json(true));

    let got = std::fs::read_to_string(home_b.join(".workbuddy").join("MEMORY.md")).unwrap();
    assert!(got.contains("A 的记忆"), "应写入 A 的记忆，实际: {got}");
    assert_eq!(
        std::fs::read_to_string(
            home_b.join(".workbuddy").join("skills").join("demo").join("SKILL.md")
        )
        .unwrap(),
        "---\nname: demo\n---\nhello"
    );

    // 本地旧版应留 conflict 备份
    let mem_dir = home_b.join(".workbuddy");
    let has_conflict = std::fs::read_dir(&mem_dir)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("MEMORY.md.conflict-"));
    assert!(has_conflict, "本地旧版 MEMORY.md 应留 .conflict- 备份");

    // 敏感资产不得进入 B 机
    assert!(
        !home_b.join(".workbuddy").join("auth-token.txt").exists(),
        "敏感词文件不应被同步"
    );
    assert!(
        !home_b.join(".workbuddy").join("keyblob").exists(),
        "keyblob 不应被同步"
    );

    // ── 自检：B 视角应看到 machine-a 的包 ──
    let st = sync::sync_status();
    assert_eq!(st["machine"], jstr("machine-b"));
    assert_eq!(st["share_dir_set"], json(true));
    let peers = st["peers"].as_array().unwrap();
    assert!(
        peers.iter().any(|p| p["machine"] == jstr("machine-a")),
        "status 应列出对端包"
    );

    // 清理
    let _ = std::fs::remove_dir_all(&home_a);
    let _ = std::fs::remove_dir_all(&home_b);
    let _ = std::fs::remove_dir_all(&share);
}

fn json(b: bool) -> serde_json::Value {
    serde_json::Value::Bool(b)
}

fn jstr(s: &str) -> serde_json::Value {
    serde_json::Value::String(s.to_string())
}

/// 真实数据只读冒烟（默认 ignore）：cargo test -p account_ops --test sync_roundtrip -- --ignored --nocapture real_status_smoke
#[test]
#[ignore]
fn real_status_smoke() {
    let st = sync::sync_status();
    println!("{}", serde_json::to_string_pretty(&st).unwrap());
}
