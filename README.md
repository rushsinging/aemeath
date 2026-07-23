# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## 设计导航

设计真相按系统级、模块级与工程守则三层组织，完整索引见 [`docs/design/README.md`](docs/design/README.md)。

| 层级 | 入口 | 角色 |
|---|---|---|
| **01 · 系统级** | [系统级设计索引](docs/design/01-system/README.md) | 产品与领域、统一语言、上下文地图、系统架构、依赖规则与代码组织 |
| **02 · 模块级** | [模块设计索引](docs/design/02-modules/README.md) | 各能力的战术模型、公开 façade 与真实外部 seam |
| **03 · 工程守则** | [工程守则索引](docs/design/03-engineering/README.md) | 架构守卫、Agent 编排设计、测试架构与覆盖率治理、迁移治理 |

## 项目结构

```text
aemeath/
├── apps/
│   └── cli/                # CLI 二进制、TUI 与旧版 REPL
├── agent/
│   ├── features/           # runtime / tools / provider / project 等业务能力
│   ├── shared/             # 横切基础设施与最小共享内核
│   └── composition/        # 唯一生产装配入口
├── packages/
│   ├── sdk/                # CLI ↔ Runtime 公共契约
│   └── global/
│       ├── logging/        # 日志 projection 适配
│       └── utils/          # 通用无业务语义 helper
├── docs/
│   ├── design/             # 三层设计真相源
│   ├── snapshot/           # 历史 spec 快照
│   ├── superpowers/        # 设计与实施工作流产物
│   ├── mockups/            # UI 草图
│   └── visual/             # 可视化资产
├── specs/                  # 按路径 / 场景加载的开发约束
├── AGENTS.md               # 仓库级 agent 工作约束
└── README.md               # 仓库入口
```

## 文档入口

- 设计真相源：[`docs/design/README.md`](docs/design/README.md)
- Bug / Feature 追踪：[GitHub Issues](https://github.com/rushsinging/aemeath/issues)
- Agent 工作约束：[`AGENTS.md`](AGENTS.md)

## License

本项目使用 [MIT License](LICENSE) 开源。
