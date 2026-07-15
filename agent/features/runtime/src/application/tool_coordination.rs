//! tool_coordination — Tool 调用编排：Policy/Hook/审批/并发/结果回收。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - ToolCall 双 ID 映射（领域 `ToolCallId` <-> `provider_id`）
//! - Policy/Hook/审批门禁
//! - timeout/cancellation
//! - 多调用并发执行
//! - 结果回收与 Run Step 写入
//! - 内置 ToolLoopGuard（工具循环熔断，StuckGuard L2）
//! - SubAgent 派生工具 -> 触发 agent_run 的 `derive_sub_run`
//!
//! 消费：`ToolCatalogPort`、`ToolExecutionPort`、`PolicyPort`、`HookPort`
//!
//! 实现由 #877 负责。

#![allow(dead_code)]
