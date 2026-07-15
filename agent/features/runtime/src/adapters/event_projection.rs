//! event_projection — 领域事件 -> SDK ChatEvent 的横切投影。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - 领域事件 -> SDK `ChatEvent`
//! - Main/Sub 路由与命名（Main -> TUI，Sub -> 父 Run）
//! - 补 `agent_id`（#612 缺口）
//!
//! 消费：`EventSink`
//!
//! event_projection 被各模块 emit 调用，不反向依赖业务逻辑。
//!
//! 实现由 #874 负责。

#![allow(dead_code)]
