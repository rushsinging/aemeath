//! context_coordination — 构建本轮 Context Window。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - 取历史消息
//! - compact 家族（L2 snip / L3 microcompact / L4 collapse / L5 auto-compact）
//! - memory 注入
//! - prompt/guidance 装配
//! - token budget 计算
//!
//! 消费：`ContextPort`（Context Management BC）、`MemoryPort`
//!
//! 注：Session 对话历史属 Context Management，本模块只是 Runtime 侧调用协调。
//! Memory 边界：检索归 Memory（`MemoryPort.retrieve`），注入进 Context Window 归
//! Context Management——记忆本体是独立 BC，不是 Context 的一部分。
//!
//! 实现由 #876 负责。

#![allow(dead_code)]
