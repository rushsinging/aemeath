//! Composition update 装配测试（L2 冒烟）。
//!
//! `UpdateGateway` 的 UA 绑定与 HTTP 行为由 update feature 的
//! `gateway/tests.rs` 承担；这里锁定 Composition 装配语义：
//! `wire_update` 每次调用构造独立 handle，`default_user_agent`
//! 与 Config 默认值保持单一真相。

use composition::update::{default_user_agent, wire_update};
use sdk::UpdateService;

#[test]
fn default_user_agent_matches_config_default_single_source() {
    assert_eq!(
        default_user_agent(),
        share::config::Config::default().api.user_agent,
        "default_user_agent 必须以 Config::default().api.user_agent 为唯一真相"
    );
}

#[test]
fn wire_update_constructs_distinct_shared_handles_per_call() {
    let first: std::sync::Arc<dyn UpdateService> = wire_update("aemeath-composition-test/1.0");
    let second: std::sync::Arc<dyn UpdateService> = wire_update(default_user_agent());

    assert!(
        !std::sync::Arc::ptr_eq(&first, &second),
        "wire_update 每次调用必须构造新的 gateway 实例"
    );
}
