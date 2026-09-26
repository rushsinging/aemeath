#![deny(clippy::print_stdout, clippy::print_stderr)]

//! Memory 支撑域。
//!
//! # PL：`memory::api` 是唯一发布面（根零导出）
//!
//! 对照 update crate 的 façade 形态，本 crate 对外只发布 [`api`] 模块；
//! crate 根不 `pub use` 任何符号，内部模块间一律经真实模块路径
//! （`crate::domain::…` / `crate::ports::…`）互相引用。按 DDD 分类，
//! `api` 发布以下五类：
//!
//! - **端口（Ports）**：[`api::MemoryPort`]（读写/反思应用）、
//!   [`api::ReflectionHistoryStore`] / [`api::ReflectionHistoryQuery`]（反思历史）、
//!   [`api::MemoryOpener`] / [`api::LegacyMemorySourceFactory`]（打开与 legacy 发现）。
//!   实现方只依赖这些 trait，Storage/文件细节不越界。
//! - **值对象（Value Objects）**：[`api::MemoryEntry`]、[`api::MemoryId`]、
//!   [`api::MemoryLayer`]、[`api::MemoryCategory`]、[`api::MemorySource`]、
//!   [`api::ProjectMemoryKey`]、[`api::MemoryQuery`] 等——纯数据、不可变语义。
//! - **命令/编排（Commands / Orchestration）**：[`api::ReflectionWorkflow`] 族
//!   （[`api::ReflectionExecutionIdentity`]、[`api::ReflectionExecutionResult`]、
//!   [`api::ReflectionWorkflowError`]）——按固定顺序编排端口调用，无状态。
//! - **适配器（Adapters）**：[`api::InMemoryMemory`] 为内存实现，
//!   **test-only**；[`api::NoOpMemory`] 为读侧单体空对象
//!   （Run 禁用 Memory 时的占位实现），生产路径由 composition 注入真实现。
//! - **错误族（Errors）**：[`api::MemoryError`]、[`api::MemoryOpenerError`]、
//!   [`api::MemoryOpenError`]、[`api::LegacyMemorySourceError`] 等，
//!   内部各层经 `From` 透传保留，`Display` **NEVER** 携带 raw 正文/对话。
//!
//! 测试独占符号（`CompactResult`、`MemorySuggestion` 等）暂留 `api`，
//! 判定记录：随测试迁移批收窄。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:memory";

// ---------- composition-only wiring（根级 wire 工厂，config crate 同判） ----------
//
// 三个实现体（`DatasetMemoryOpener`、`FileLegacyMemorySourceFactory`、
// `AtomicDatasetReflectionHistoryStore`）的构造入口：**composition 及跨 crate
// 测试的唯一构造点**。实现体本身与 `new` 均已收窄 `pub(crate)`——crate 外
// 一切散落构造在编译期不可达，只能经下列工厂取得 trait 对象。参数保持
// composition 既有的输入（存储句柄 / legacy 路径 / project key），装配语义不变。

/// Composition 打开 Memory 的唯一构造入口：返回 `Box<dyn MemoryOpener>`
/// （MainSession 依赖的消费签名）。`DatasetMemoryOpener` 为 crate 内实现
/// 细节，其 `new` 已收窄 `pub(crate)`，crate 外不可达；legacy 发现经
/// [`wire_legacy_memory_source_factory`] 取得的 trait 对象注入。
pub fn wire_memory_opener(
    storage: std::sync::Arc<dyn storage::AtomicDatasetPort>,
    legacy_factory: std::sync::Arc<dyn crate::ports::LegacyMemorySourceFactory>,
) -> Box<dyn crate::ports::MemoryOpener> {
    Box::new(crate::adapters::DatasetMemoryOpener::new(
        storage,
        legacy_factory,
    ))
}

/// Composition 构造 legacy 发现工厂的唯一入口：返回
/// `Arc<dyn LegacyMemorySourceFactory>`。`FileLegacyMemorySourceFactory`
/// 为 crate 内实现细节，其 `new` 已收窄 `pub(crate)`，crate 外不可达。
pub fn wire_legacy_memory_source_factory(
    base_dir: impl Into<std::path::PathBuf>,
) -> std::sync::Arc<dyn crate::ports::LegacyMemorySourceFactory> {
    std::sync::Arc::new(crate::adapters::FileLegacyMemorySourceFactory::new(
        base_dir,
    ))
}

/// Composition 构造反思历史存储的唯一入口：返回
/// `Arc<dyn ReflectionHistoryStore>`。`AtomicDatasetReflectionHistoryStore`
/// 为 crate 内实现细节，其 `new` 已收窄 `pub(crate)`，crate 外不可达。
pub fn wire_reflection_history_store(
    storage: std::sync::Arc<dyn storage::AtomicDatasetPort>,
    project: crate::domain::ProjectMemoryKey,
) -> std::sync::Arc<dyn crate::ports::ReflectionHistoryStore> {
    std::sync::Arc::new(crate::adapters::AtomicDatasetReflectionHistoryStore::new(
        storage, project,
    ))
}

mod adapters;
mod application;
mod codec;
mod domain;
mod noop;
mod ports;
mod service;

pub mod api;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;
