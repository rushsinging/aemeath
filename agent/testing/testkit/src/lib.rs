//! 测试替身集中营（testkit）。
//!
//! 口径（2026-11-02 拍板）：**定义方不负责 fake**——各 crate 的
//! `src/test_support.rs` 通道撤销（守卫 `pattern.pl.fake-impl-location`
//! 禁止该路径段重新出现），跨 crate 共享的测试替身统一在本 crate 维护；
//! 消费方经 `[dev-dependencies] testkit` 引用，不进生产依赖图。
//! 各 crate 测试内的局部内联 stub（同文件 `#[cfg(test)]`、不跨模块导出）
//! 仍允许。未来其他测试基建（fixture/harness）加在 `agent/testing/` 下。

pub mod blob;
pub mod context;
pub mod memory;
pub mod policy;
pub mod project;
pub mod provider;
pub mod runtime;
pub mod systemone;
pub mod tools;
