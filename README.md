# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## 关键设计

**[设计总纲](docs/design/01-outline.md)** — DDD 六边形架构、统一语言、Bounded Context、COLA 分层、依赖铁律。

| 主题 | 设计文档 | 角色 / 状态 |
|---|---|---|
| 架构总纲 | [01-outline.md](docs/design/01-outline.md) | 全局架构原则：Bounded Context、COLA 分层、依赖铁律 |
| 架构守卫 | [02-architecture-guards.md](docs/design/02-architecture-guards.md) | 17 个 guard + 白名单单一真相 |
| Runtime | [03-runtime-design.md](docs/design/03-runtime-design.md) | 核心域应用服务 |
| TUI | [04-tui-design.md](docs/design/04-tui-design.md) | 入站适配器（终端） |
| Agent 编排 | [05-agent-orchestration.md](docs/design/05-agent-orchestration.md) | 编排范式知识地图（知识储备） |
| Reasoning Graph | [06-agent-reasoning-graph.md](docs/design/06-agent-reasoning-graph.md) | 阶段节点驱动 reasoning effort（草案） |
| Server | [07-server-design.md](docs/design/07-server-design.md) | 入站适配器（远端，草案） |

## 项目结构

```text
aemeath/
├── apps/          # CLI/TUI 等应用入口
├── packages/      # core / llm / tools 等库 crate
├── docs/          # 设计文档与历史归档
├── AGENTS.md      # 项目级 agent 工作约束
└── README.md      # 项目入口说明
```

## 文档入口

- 设计真相源：[`docs/design/README.md`](docs/design/README.md)
- Bug / Feature 追踪：[GitHub Issues](https://github.com/rushsinging/aemeath/issues)
- Agent 工作约束：[`AGENTS.md`](AGENTS.md)
