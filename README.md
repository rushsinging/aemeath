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

## 本地构建

```bash
./build_cli.sh        # 构建并安装到 ~/.local/bin/aemeath
```

构建脚本默认仅在 **macOS arm64** 启用 `composition/systemone-daemon` feature（编译 llama.cpp 原生评分引擎，支持 `systemone serve`）；其他平台 embedded 引擎不支持，默认关闭以免徒增编译时间与体积。

- **macOS**：Xcode Command Line Tools 自带 clang，开箱可用。
- **Ubuntu/Debian**：如需显式开启该 feature（`AEMEATH_CLI_FEATURES="composition/systemone-daemon" ./build_cli.sh`），需先安装 clang 内置 C 标准头，否则构建报 `'stdbool.h' file not found`：

  ```bash
  sudo apt install libclang-common-18-dev   # Ubuntu 24.04；其他版本按系统 clang 版本选择
  ```

- 瘦身安装：`AEMEATH_CLI_FEATURES= ./build_cli.sh`（空串关闭默认 feature）。

## 文档入口

- 设计真相源：[`docs/design/README.md`](docs/design/README.md)
- Bug / Feature 追踪：[GitHub Issues](https://github.com/rushsinging/aemeath/issues)
- Agent 工作约束：[`AGENTS.md`](AGENTS.md)

## License

本项目使用 [MIT License](LICENSE) 开源。
