# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## 关键设计

**[设计总纲](docs/design-outline.md)** — 涵盖架构总纲（DDD + COLA）、Runtime、TUI、Server 四大模块的设计终态。

核心结论：

- 核心域是 **Agent Runtime**。
- Agent 是由 `ConfigurationSnapshot` 解析出的配置化执行者实体。
- Agent Runtime 使用 `Session` / `Chat` / `Agent Looping` / `Turn` / `Task` 作为统一语言。
- 内部实体 ID（ChatId/ChatTurnId/ToolCallId）使用 UUIDv7，与 provider 协议 ID 严格分离。
- workspace 状态由 `WorkspaceService`（project feature）单一持有，runtime 仅持有实例生命周期。
- TUI 遵循 Model/View 分离架构，通过 `AgentClient` trait（`packages/sdk`）与 Runtime 通信。
- Server 采用控制面薄代理 + worker 自托管 WS + CLI 双模式，控制面 NEVER 承载领域实体。
- 架构边界由自动化守卫脚本强制执行（见 `.agents/hooks/check-architecture-guards.sh`）。

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

- 当前活跃 feature：[`docs/snapshot/active.md`](docs/snapshot/active.md)
- 当前活跃 bug：[`docs/bug/active.md`](docs/bug/active.md)
- DDD 架构设计：[`docs/snapshot/specs/047-ddd-redesign.md`](docs/snapshot/specs/047-ddd-redesign.md)
