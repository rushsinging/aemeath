# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## DDD 架构设计

Feature #47 将 Aemeath 的架构基线定义为 DDD 领域模型，并以 COLA 分层规范作为工程落地参考，详见 [`docs/feature/specs/047-ddd-redesign.md`](docs/feature/specs/047-ddd-redesign.md)。

核心结论：

- 核心域是 **Agent Runtime**。
- Agent 是由 `ConfigurationSnapshot` 解析出的配置化执行者实体。
- Agent Runtime 使用 `Session` / `Chat` / `Agent Looping` / `Turn` / `Task` 作为统一语言。
- `Task` 属于 Agent Runtime，由 `Agent Looping` 推进，持久化投影进入 `Session History`。
- `PermissionDecision` 与 `HookDecision` 分离。
- `Audit` 独立记录权限、hook、工具、模型调用和最终 outcome。
- `Skill / Guidance` 独立于 `Configuration`，`Memory` 不依赖 `Skill / Guidance`。
- HTTP / CLI / TUI / SDK 等入口保持薄，只作为 inbound adapter 接入统一 application service。
- 包或模块边界应逐步靠近 Bounded Context，避免 `core` 成为所有领域概念的混合仓库。
- COLA 作为工程分层参考，要求 Adapter / Application / Domain / Infrastructure / Client 职责分离。

## 项目结构

```text
aemeath/
├── apps/          # CLI/TUI 等应用入口
├── packages/      # core / llm / tools 等库 crate
├── docs/          # bug / feature 追踪与设计文档
├── AGENTS.md      # 项目级 agent 工作约束
└── README.md      # 项目入口说明
```

## 文档入口

- 当前活跃 feature：[`docs/feature/active.md`](docs/feature/active.md)
- 当前活跃 bug：[`docs/bug/active.md`](docs/bug/active.md)
- DDD 架构设计：[`docs/feature/specs/047-ddd-redesign.md`](docs/feature/specs/047-ddd-redesign.md)
