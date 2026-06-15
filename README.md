# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## 关键设计

**[设计总纲](docs/design/outline.md)** — DDD 六边形架构、统一语言、Bounded Context、COLA 分层、依赖铁律。

| 模块 | 设计文档 | 六边形角色 |
|---|---|---|
| Runtime | [runtime-design.md](docs/design/runtime-design.md) | 核心域应用服务 |
| TUI | [tui-design.md](docs/design/tui-design.md) | 入站适配器（终端） |
| Server | [server-design.md](docs/design/server-design.md) | 入站适配器（远端） |

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
