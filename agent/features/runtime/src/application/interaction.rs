//! interaction — 处理执行中断。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - `AwaitingUser`（ask_user）：暂停 Run 等待用户输入
//! - `AwaitingToolApproval`（权限门）：暂停 Run 等待审批
//! - pause/resume
//! - 触发 Run 状态机迁移到 `AwaitingUser` / `AwaitingToolApproval`
//!
//! 消费：`InteractionPort`（UI 交互）、`PolicyPort`（权限判断）
//!
//! 实现由 #878 负责。

#![allow(dead_code)]
