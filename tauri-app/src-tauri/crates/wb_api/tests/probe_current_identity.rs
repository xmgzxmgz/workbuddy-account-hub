//! 解密当前活跃登录态的账号信息，确认客户端当前登录的是哪个手机号。
//! cargo test -p wb_api --test probe_current_identity -- --ignored --nocapture
use wb_api::load_login;

#[test]
#[ignore]
fn probe_current_identity() {
    let l = load_login().expect("读不到当前登录态");
    println!("file   = {}", l.file);
    println!("uid    = {}", l.uid);
    println!("domain = {}", l.domain);
    let acc = &l.account;
    for k in ["uin", "type", "enterpriseId", "idp", "nickname", "phoneNumber", "accountType"] {
        println!("  {k} = {}", acc.get(k).unwrap_or(&serde_json::Value::Null));
    }
}
