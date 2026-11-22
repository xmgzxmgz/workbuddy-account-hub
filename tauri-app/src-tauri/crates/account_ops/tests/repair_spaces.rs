//! issue #6「分组空间」修复逻辑测试 —— 官方 5.11.2 只归并 sessions 不归并 spaces。
//! 用临时 SQLite 复现 issue 里描述的真实数据形态，验证三段修复。
//!
//! cargo test -p account_ops --test repair_spaces
use account_ops::repair_spaces;
use rusqlite::Connection;

const TARGET: &str = "aaaaaaaa-1111-1111-1111-111111111111";
const OLD_A: &str = "bbbbbbbb-2222-2222-2222-222222222222";
const OLD_B: &str = "cccccccc-3333-3333-3333-333333333333";

fn setup(tag: &str) -> (std::path::PathBuf, Connection) {
    let dir = std::env::temp_dir().join(format!("hub-spaces-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("workbuddy.db");
    let c = Connection::open(&db).unwrap();
    // 最小 schema：只建修复逻辑会用到的表与列
    c.execute_batch(
        "CREATE TABLE spaces (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            user_id TEXT NOT NULL,
            primary_folder_path TEXT,
            create_folder_path TEXT,
            deleted_at INTEGER NOT NULL DEFAULT -1
         );
         CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            space_id TEXT,
            user_id TEXT NOT NULL
         );",
    )
    .unwrap();
    (db, c)
}

#[test]
fn aligns_merges_and_fills_primary_path() {
    let (db, c) = setup("main");
    // 三个同名分组（issue 里「Aromiso」3 份），分属不同 user_id
    c.execute_batch(&format!(
        "INSERT INTO spaces (id,name,user_id,create_folder_path) VALUES
            ('s1','Aromiso','{OLD_A}','/tmp/a'),
            ('s2','Aromiso','{OLD_B}','/tmp/b'),
            ('s3','Aromiso','{TARGET}','/tmp/c'),
            ('s4','Work','{OLD_A}','/tmp/w');"
    ))
    .unwrap();
    // 会话引用分裂到三个 id 上
    c.execute_batch(&format!(
        "INSERT INTO sessions (id,space_id,user_id) VALUES
            ('x1','s1','{OLD_A}'), ('x2','s2','{OLD_A}'), ('x3','s3','{TARGET}'), ('x4','s4','{OLD_A}');"
    ))
    .unwrap();

    let r = repair_spaces(&db, TARGET).unwrap();
    assert_eq!(r["ok"], true, "{r:?}");
    // 归属全部对齐到目标账号
    assert_eq!(r["aligned"].as_u64().unwrap(), 3, "应把 3 个非目标归属改为目标: {r:?}");
    // 同名只留一个未删（优先保留目标账号名下那个 s3）
    assert_eq!(r["dup_merged"].as_u64().unwrap(), 2, "应合并 2 个重名: {r:?}");
    // 引用重定向：s1/s2 的会话都指向保留的 s3
    assert_eq!(r["session_redirected"].as_u64().unwrap(), 2, "{r:?}");
    // 补 primary_folder_path：只补**存活**的分组（去重后剩 2 个）。
    // 顺序有意为之：先合并重名再补路径，避免给即将软删的重复行做无谓写入。
    assert_eq!(r["primary_filled"].as_u64().unwrap(), 2, "{r:?}");

    // ── DB 级断言 ──
    let alive: i64 = c
        .query_row("SELECT COUNT(*) FROM spaces WHERE deleted_at = -1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(alive, 2, "应只剩 Aromiso(1) + Work(1)");

    let keep_id: String = c
        .query_row("SELECT id FROM spaces WHERE name='Aromiso' AND deleted_at = -1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(keep_id, "s3", "应保留目标账号名下的分组");

    let stray: i64 = c
        .query_row(
            &format!("SELECT COUNT(*) FROM sessions WHERE space_id IN ('s1','s2')"),
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stray, 0, "旧引用应全部重定向");

    let null_primary: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM spaces WHERE deleted_at = -1 AND primary_folder_path IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(null_primary, 0, "primary_folder_path 不应再有空值");

    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[test]
fn missing_spaces_table_is_skipped_not_error() {
    // 模拟 5.9.3 这类没有分组空间的旧客户端
    let dir = std::env::temp_dir().join(format!("hub-spaces-old-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("workbuddy.db");
    Connection::open(&db).unwrap().execute_batch(
        "CREATE TABLE sessions (id TEXT PRIMARY KEY, space_id TEXT, user_id TEXT NOT NULL);",
    ).unwrap();

    let r = repair_spaces(&db, TARGET).unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(r["skipped"], true, "旧客户端应跳过而非报错: {r:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn idempotent_second_run_changes_nothing() {
    let (db, c) = setup("idem");
    c.execute_batch(&format!(
        "INSERT INTO spaces (id,name,user_id,create_folder_path) VALUES
            ('s1','Aromiso','{OLD_A}','/tmp/a'), ('s2','Aromiso','{TARGET}','/tmp/c');"
    ))
    .unwrap();
    let _ = repair_spaces(&db, TARGET).unwrap();
    let second = repair_spaces(&db, TARGET).unwrap();
    assert_eq!(second["aligned"].as_u64().unwrap(), 0, "第二次不应再有归属变更");
    assert_eq!(second["dup_merged"].as_u64().unwrap(), 0, "第二次不应再有重名合并");
    assert_eq!(second["session_redirected"].as_u64().unwrap(), 0);
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}
