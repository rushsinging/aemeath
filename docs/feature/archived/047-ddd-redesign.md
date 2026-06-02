# #47 以 DDD 思路重新设计 Aemeath 架构

**状态**：✅ 已完成

**确认日期**：2026-05-31

## 背景

Aemeath 已从单一 CLI 演进为包含 TUI、LLM provider、工具系统、hook、skill、memory、task、worktree、权限与会话管理的 AI 编程助手。当前代码仍主要按技术分层和 crate 边界组织，随着功能增加，领域概念之间的边界容易变得模糊，例如 Agent 会话、工具执行、权限评估、上下文压缩、项目配置、skills 与 hooks 之间的职责交叉。用户希望以 DDD（领域驱动设计）的思路重新设计项目，让后续重构先有清晰的领域模型和边界，而不是直接按文件/模块做局部移动。

## 设计结论

核心域为 Runtime；Agent 是配置化实体；Runtime 使用 Session / Chat / Agent Looping / Turn / TaskBoard 作为统一语言；Task 属于 Runtime，由 Agent Looping 推进，持久化投影进入 Storage；CLI/TUI/HTTP/SDK 等入口保持薄，TUI/业务代码逐步只通过 `packages/sdk` 的 `AgentClient` 契约接入核心域；当前单二进制 Rust 部署下，`apps/cli` 的 composition root 仍依赖 `agent/runtime` 装配真实实现，但不得直接依赖 supporting domains 或 share/core；Runtime 作为唯一编排者调度 Project、Policy、Prompt、Provider、Tools、Storage、Hook、Audit；Cargo dependency graph、forbidden import、public API visibility 和 Stop hook 必须共同防止双向依赖与边界绕过；COLA 作为工程分层参考，要求 Adapter / Application / Domain / Infrastructure / Client 职责分离；Audit 独立；PermissionDecision 与 HookDecision 分离；Prompt 独立承载 Skill / Guidance / instruction；完整设计见 [spec](../specs/047-ddd-redesign.md)。

## 实施总结

P0-P18 共 19 个阶段全部完成：

- **P0**：创建 `packages/sdk`（AgentClient trait + ChatStream/SessionSnapshot/ChangeSet 等公共类型）
- **P1**：`AgentClientImpl::from_args()` 吞掉 setup.rs 全部 build_* 编排，CLI 瘦身为 ~120 行
- **P2**：chat application 契约、sub-agent runner、runtime bootstrap 迁移到 `crates/runtime`
- **P3-P4**：严格方案 B 首轮实施，workspace 顶层收敛为 `apps/` + `crates/`
- **P5**：CLI 边界修正——CLI 同时依赖 sdk 与 runtime，sdk 提供契约，runtime 仅供 composition root
- **P6-P8**：TUI 统一 SDK 解耦，chat turn、cancel、/save、task status 均改走 SDK
- **P9**：CLI 非 UI 模块抽取到 `crates/runtime`
- **P10**：Storage、Project domain 拆分；skill→prompt、hook→hook、scheduler→runtime 等
- **P11**：`crates/` 重命名为 `agent/`，新增 `agent/share` 跨 service 公共抽象层
- **P12**：SDK 补齐 TUI 可消费的强类型 DTO
- **P13**：TUI runtime/domain 解耦收口
- **P14**：`client.rs`（1349 行）拆分为 10 个子模块
- **P15**：runtime 内部按职责分层为 core/business/utils
- **P16**：core/ 层端口隔离（TaskStorePort、ProviderInfoPort、HookNotificationPort）
- **P17**：share/core 瘦身（迁出 config/manager、message/integrity、token_estimation 等）
- **P18**：架构守卫固化为 8 个，TUI 白名单清零，全量编译/689 测试/cargo clippy 通过

## 关联

- Feature #42（权限管控系统）、#61（架构债务收口）、#62（audit/policy 域实现）
- Spec：[docs/feature/specs/047-ddd-redesign.md](../specs/047-ddd-redesign.md)
