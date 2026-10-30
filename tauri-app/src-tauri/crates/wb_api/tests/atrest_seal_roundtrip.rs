//! at-rest 字段信封「加密 → 解密」往返测试（真实本机登录态，默认 ignore）。
//!
//! 目的：证明 Hub 现在能把解密后的明文字段**原样封回**官方客户端的
//! `{"$wbEncrypted":1,"envelope":"..."}` 格式，而不是把明文写进加密登录态文件。
//! 若本测试通过，说明 keyId / nonce / authTag / ciphertext / AAD 帧构造与官方一致
//! （AAD 规则取自官方 app.asar 的 at-rest-crypto chunk）。
//!
//! 跑法：cargo test -p wb_api --test atrest_seal_roundtrip -- --ignored --nocapture
use serde_json::Value;
use wb_api::wb_enc;

/// 纯算法自检：不依赖真实文件，只要 at-rest 密钥可用就应通过。
#[test]
#[ignore]
fn seal_selftest_roundtrip() {
    wb_enc::selftest_roundtrip().expect("加密→解密往返应一致");
    println!("selftest roundtrip OK");
}

/// 真实文件往返：读真实信封 → 解密 → 重新加密 → 再解密，明文必须完全一致，
/// 且新信封的 keyId 必须等于本机 keyblob 的 protectorKeyId。
#[test]
#[ignore]
fn reseal_real_token_roundtrip() {
    let key = wb_enc::load_at_rest_key().expect("at-rest 密钥应可用");
    let expect_key_id = wb_enc::device_key_id(&key);
    println!("derived keyId = {expect_key_id}");
    assert!(
        wb_enc::key_matches_device(&key).unwrap_or(true),
        "派生 keyId 与 keyblob 不匹配，密钥已轮换"
    );

    // 找一个真实登录态文件（明文路径由 wb_api 统一裁决）
    let path = wb_api::active_auth_file().expect("应能找到活跃登录态文件");
    println!("active auth file = {}", path.display());
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let env = doc
        .get("auth")
        .and_then(|a| a.get("accessToken"))
        .expect("应有 auth.accessToken");
    if env.is_string() {
        println!("当前登录态为明文落盘（Windows 旧版形态），跳过信封往返");
        return;
    }

    // 1) 解密真实信封（解出来就是客户端拿到的那串明文；不打印内容，只打印长度）
    let plain = wb_enc::auth_field_string(env, &key)
        .expect("真实信封应能解密")
        .expect("应返回明文");
    println!("decrypted token len = {}", plain.len());

    // 2) 用 Hub 的加密实现重新封装
    let resealed = wb_enc::encrypt_field(&plain, &key).expect("重新封装应成功");
    assert_eq!(resealed.get("$wbEncrypted").and_then(|x| x.as_u64()), Some(1));
    let env_b64 = resealed.get("envelope").and_then(|x| x.as_str()).unwrap();
    use base64::Engine;
    let env_json: Value =
        serde_json::from_slice(&base64::engine::general_purpose::STANDARD.decode(env_b64).unwrap())
            .expect("envelope 应是 base64(JSON)");
    assert_eq!(env_json.get("suite").and_then(|x| x.as_u64()), Some(1));
    assert_eq!(
        env_json.get("keyId").and_then(|x| x.as_str()),
        Some(expect_key_id.as_str()),
        "新信封 keyId 必须与本机 keyblob 一致"
    );

    // 3) 再解回来，明文必须一模一样（证明 GCM 加解密与 AAD 完全自洽）
    let back = wb_enc::auth_field_string(&resealed, &key)
        .expect("Hub 自己封的信封应能解开")
        .expect("应返回明文");
    assert_eq!(back, plain, "往返明文不一致");
    println!("reseal roundtrip OK（keyId / nonce / tag / AAD 全部自洽）");
}

/// 整文档往返：模拟 `switch_auth_to` 的写回链路（解密 → 构造 → seal → 校验），
/// 确认写回后的文档**全程没有任何敏感字段以明文落盘**，且每个字段都能解回原值。
/// 这是「切换不再写坏登录态」的直接证据。
#[test]
#[ignore]
fn whole_doc_seal_is_lossless() {
    let key = wb_enc::load_at_rest_key().expect("at-rest 密钥应可用");
    let path = wb_api::active_auth_file().expect("应能找到活跃登录态文件");
    let raw = std::fs::read_to_string(&path).unwrap();
    let mut doc: Value = serde_json::from_str(&raw).unwrap();
    if doc
        .get("auth")
        .and_then(|a| a.get("accessToken"))
        .map(|t| t.is_string())
        .unwrap_or(false)
    {
        println!("当前为明文落盘（Windows 旧版形态），无需封装，跳过");
        return;
    }

    // 记录解密后的真值
    let tokens = ["accessToken", "refreshToken"];
    let mut expect: Vec<(String, String)> = Vec::new();
    for k in tokens {
        if let Some(env) = doc.get("auth").and_then(|a| a.get(k)).cloned() {
            if let Ok(Some(p)) = wb_enc::auth_field_string(&env, &key) {
                expect.push((format!("auth.{k}"), p));
            }
        }
    }
    for k in ["nickname", "phoneNumber"] {
        if let Some(env) = doc.get("account").and_then(|a| a.get(k)).cloned() {
            if let Ok(Some(p)) = wb_enc::auth_field_string(&env, &key) {
                expect.push((format!("account.{k}"), p));
            }
        }
    }
    for (i, item) in doc
        .get("allAccounts")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        for k in ["nickname", "phoneNumber"] {
            if let Some(env) = item.get(k).cloned() {
                if let Ok(Some(p)) = wb_enc::auth_field_string(&env, &key) {
                    expect.push((format!("allAccounts.{i}.{k}"), p));
                }
            }
        }
    }

    // 解密（就地）
    assert!(wb_api::decrypt_envelope_fields(&mut doc), "解密应成功");
    // 写回前重新封装
    let (sealed, errs) = wb_api::seal_sensitive_fields(&mut doc);
    assert!(errs.is_empty(), "封装不应有失败: {errs:?}");
    println!("sealed {sealed} 个敏感字段");

    // 断言：文档里不再有任何明文敏感字段，且每个字段都能解回原值
    let json_text = serde_json::to_string(&doc).unwrap();
    for (k, plain) in &expect {
        assert!(
            !json_text.contains(plain.as_str()),
            "{k} 的明文仍出现在待落盘文档中（会写坏登录态）"
        );
        let node = k.split('.').fold(&doc, |acc, part| {
            if let Ok(i) = part.parse::<usize>() {
                acc.get(i).unwrap_or(&Value::Null)
            } else {
                acc.get(part).unwrap_or(&Value::Null)
            }
        });
        assert!(
            node.is_object() && node.get("$wbEncrypted").is_some(),
            "{k} 回路封装后应为信封"
        );
        let back = wb_enc::auth_field_string(node, &key).unwrap().unwrap();
        assert_eq!(&back, plain, "{k} 封装后解回的明文与原值不一致");
    }
    println!("whole-doc seal OK：{} 个字段全部信封化且无损", expect.len());
}

/// 登录态路径诊断：打印候选清单与最终选中项（排查「切换无效」时直接跑这个）。
#[test]
#[ignore]
fn dump_auth_path_report() {
    println!(
        "{}",
        serde_json::to_string_pretty(&wb_api::auth_path_report()).unwrap()
    );
    println!("seal_ready = {}", wb_api::at_rest_seal_ready());
}
