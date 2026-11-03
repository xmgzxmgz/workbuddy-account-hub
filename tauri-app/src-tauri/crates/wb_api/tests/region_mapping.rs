//! 区域识别与 API 网关推导测试（issue #4：支持海外版）。
//!
//! 纯逻辑，不依赖登录态与网络，CI 会执行。
//! 海外版与国内版是两套互不通用的网关：国内 copilot.tencent.com、国际 www.workbuddy.ai。
//! 推导错了会把海外账号打到国内网关（必然 401），或反之。

use wb_api::{api_base_for_domain, region_of_domain};

#[test]
fn cn_domains_map_to_cn_gateway() {
    for d in ["www.workbuddy.cn", "workbuddy.cn", "copilot.tencent.com", ""] {
        assert_eq!(region_of_domain(d), "cn", "domain={d:?} 应判为国内");
        assert_eq!(
            api_base_for_domain(d),
            "https://copilot.tencent.com",
            "domain={d:?} 应走国内网关"
        );
    }
}

#[test]
fn global_domains_map_to_global_gateway() {
    // 国际版有两个品牌域名：桌面端登录在 workbuddy.ai，CodeBuddy CLI 登录在 codebuddy.ai
    for d in ["www.workbuddy.ai", "workbuddy.ai", "www.codebuddy.ai", "codebuddy.ai"] {
        assert_eq!(region_of_domain(d), "global", "domain={d:?} 应判为国际");
        assert_eq!(
            api_base_for_domain(d),
            "https://www.workbuddy.ai",
            "domain={d:?} 应走国际网关"
        );
    }
}

#[test]
fn domain_matching_is_case_insensitive_and_whitespace_tolerant() {
    assert_eq!(region_of_domain("  WWW.WorkBuddy.AI  "), "global");
    assert_eq!(api_base_for_domain("  WWW.WorkBuddy.AI  "), "https://www.workbuddy.ai");
    assert_eq!(region_of_domain(" WWW.WORKBUDDY.CN "), "cn");
}

#[test]
fn unknown_domain_falls_back_to_cn_for_backward_compat() {
    // 旧快照可能没有 auth.domain 字段（value 为空），必须保持原有行为，
    // 否则存量用户会集体被切到国际网关而全部失效。
    assert_eq!(region_of_domain(""), "cn");
    assert_eq!(api_base_for_domain(""), "https://copilot.tencent.com");
}
