//! macOS 原生解密真实数据测试（默认 ignore：cargo test -p wb_api -- --ignored）
use wb_api::load_login;

#[test]
#[ignore]
fn mac_decrypt_real_login() {
    let login = load_login().expect("load_login 应成功（Mac 原生解密加密登录态）");
    println!("uid: {}", login.uid);
    println!("token len: {}", login.token.len());
    println!("token head: {}", &login.token[..12.min(login.token.len())]);
    assert!(login.token.starts_with("eyJ"), "token 应为明文 JWT");
    // 不硬编码长度：官方会话续签会刷新 token，长度随之变化；只做 JWT 形状合理性校验
    assert!(login.token.len() > 500, "token 应为完整 JWT（当前长度 {}）", login.token.len());
    assert!(!login.uid.is_empty());
}
