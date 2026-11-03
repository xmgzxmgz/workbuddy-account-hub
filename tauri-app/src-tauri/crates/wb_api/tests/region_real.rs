//! 真实登录态的区域识别回归测试（默认 ignore，需要本机已登录）：
//! cargo test -p wb_api --test region_real -- --ignored --nocapture
//!
//! 目的：确认改造后「读登录态 → 取 auth.domain → 推导网关」这条链在真实数据上成立，
//! 且国内账号**没有**被误判成国际版（这是改造最容易引入的回归）。
use wb_api::{api_base_for_domain, load_login, region_of_domain};

#[test]
#[ignore]
fn real_login_region_is_consistent() {
    let login = load_login().expect("应能读到本机登录态");
    println!("uid      = {}", &login.uid[..8.min(login.uid.len())]);
    println!("domain   = {:?}", login.domain);
    println!("region   = {}", region_of_domain(&login.domain));
    println!("api_base = {}", api_base_for_domain(&login.domain));

    // domain 可以缺失（旧快照），但只要有值就必须能推出网关
    let base = api_base_for_domain(&login.domain);
    assert!(
        base == "https://copilot.tencent.com" || base == "https://www.workbuddy.ai",
        "推导出的网关应为已知的两个之一，实际 {base}"
    );
    // 本次测试机是国内账号：若这里变成 global，说明区域判定有回归
    if login.domain.contains("workbuddy.cn") {
        assert_eq!(region_of_domain(&login.domain), "cn");
        assert_eq!(base, "https://copilot.tencent.com");
    }
}
