# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## 关键设计

本项目的架构设计核心文档：

- **[#47 DDD 架构设计](docs/feature/specs/047-ddd-redesign.md)** — 以 DDD 领域模型 + COLA 分层规范为架构基线，定义 Agent Runtime 核心域、统一语言（Session/Chat/Agent Looping/Turn/Task）及 Bounded Context 边界。
- **[TUI Model/View 架构设计](docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md)** — TUI 按 Model/View 分离架构重设计：`Model` 保存业务真相（Conversation/Input/Runtime/Diagnostic 四个 Context），`ViewAssembler` 组装 `ViewModel`，`Render` 纯渲染不反推状态。TUI 作为 CLI Adapter 不定义 Domain Model。
- **[TUI SDK DTO 边界设计](docs/feature/specs/047-tui-sdk-dto-boundary-design.md)** — TUI 与 runtime 的类型边界彻底消解：`sdk::ChatEvent` 使用强类型 DTO，TUI 内部只使用 SDK DTO 或私有 view model。

核心结论：

- 核心域是 **Agent Runtime**。
- Agent 是由 `ConfigurationSnapshot` 解析出的配置化执行者实体。
- Agent Runtime 使用 `Session` / `Chat` / `Agent Looping` / `Turn` / `Task` 作为统一语言。
- `Task` 属于 Agent Runtime，由 `Agent Looping` 推进，持久化投影进入 `Session History`。
- `PermissionDecision` 与 `HookDecision` 分离。
- `Audit` 独立记录权限、hook、工具、模型调用和最终 outcome。
- `Skill / Guidance` 独立于 `Configuration`，`Memory` 不依赖 `Skill / Guidance`。
- HTTP / CLI / TUI / SDK 等入口保持薄，只作为 inbound adapter 接入统一 application service。
- COLA 作为工程分层参考，要求 Adapter / Application / Domain / Infrastructure / Client 职责分离。
- TUI 遵循 Model/View 分离架构：Model（Conversation/Input/Runtime/Diagnostic）保存业务真相 → ViewAssembler 组装 ViewModel → Render 纯渲染 → Effect 集中执行副作用。
- 架构边界由 8 个自动化守卫脚本强制执行（见 `.agents/hooks/check-architecture-guards.sh`）。

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
