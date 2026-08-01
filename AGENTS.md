# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## 1. Constitution

1. 指令使用 **NEVER**、**MUST**、**SHOULD**、**MAY** 表示约束等级：
   - **NEVER**：绝对禁止，无例外。
   - **MUST**：强制要求。违反会导致构建失败、数据丢失或行为错误。
   - **SHOULD**：强烈推荐。跳过时必须有明确理由。
   - **MAY**：可选。有助于清晰度或便利性时使用。
2. 所有新增指令 **MUST** 遵循此约定。
3. 引入新规则或约定时，**MUST** 先征得用户同意再更新本文件或任一 `specs/` 分片。
4. **MUST** 遵循 DRY 原则。类型、辅助函数、常量、数据访问逻辑 **MUST** 定义一次、到处引用。**NEVER** 在多个文件重复相同逻辑。
5. **MUST NOT** 手动调整代码格式。格式化由 `cargo fmt` / `rustfmt` 处理，只关注逻辑变更。
6. **MUST** 遵循 TDD（测试先行）。新增或修改核心逻辑前，**MUST** 先新增或修改对应测试（feature 先写表达期望行为的测试、bug 先写复现问题的失败测试、重构先确认现有测试覆盖目标行为），再用 `cargo test` 验证修改结果；细节见 `specs/rust-coding.md`。
7. **MUST** 跨层链路改动 **MUST** 为**每一层**补单元测试或场景测试，**NEVER** 只测首尾。跨 share → runtime → sdk → tui 的数据流改动，若只测源头 emit 和最终渲染，中间任一层（SDK 转发、adapter 分发、model 写入）的覆写 / 绕过 / 字段丢失都无法被测试捕获。典型反面教材：数据全链路正确但 TUI 不显示，根因是 `AgentDisplay::format_header_line_with_result` 覆写了 trait 默认方法绕过了消费逻辑——因为链路中每一层都没有测试覆盖。
8. 类型、trait、模块、函数、方法与变量的名称 **MUST** 直接表达单一业务职责。**NEVER** 使用 `Projection` / `projection` 作为宽泛命名，也 **NEVER** 用该词掩盖同时承担状态、生命周期、编排、IO 或多个转换方向的大型抽象；发现职责混合时 **MUST** 先按职责拆分，再分别使用能说明来源、目标或用途的名称。
9. 生产代码、测试与 Guard 脚本中的变量名称 **MUST** 表达其职责或内容。除数学坐标、闭包局部占位等领域惯例且作用域极小的情形外，**NEVER** 使用无语义单字母名称；路径、源码、正则与索引等变量 **MUST** 分别使用 `path`、`source`、`pattern`、`index` 等可辨识名称。
10. 代码、代码注释与 `docs/design/**` 设计文档 **NEVER** 引用 Issue/PR 编号或其他外部追踪号；架构与行为说明 **MUST** 使用稳定的领域术语、类型名、代码路径和文档章节引用。Issue/PR 关联仅允许出现在 Issue/PR 本身、提交信息、实施计划、任务追踪记录与发版工作流材料中。
11. Stop Hook 阻断发生时，Agent **MUST** 优先于当前任务诊断并修复阻断根因，**MAY** 无需再次请求用户确认即可修改阻断相关代码、测试、Guard 与配套设计文档并运行必要验证；**NEVER** 借此扩大到与阻断无关的改动，且仍 **MUST** 遵守安全、TDD、渐进式披露与验证门禁。

## 2. 工作流（路径 / 目录结构 / Git / 发版）

以下内容已按渐进式披露原则移至 `specs/workflow.md`，**涉及对应场景时 MUST 加载**：

- **Bug / Feature 执行流程**（issue → checklist 门禁 → 方案 → 确认 → worktree → PR → 关闭；所有 check 必须完成或记录合理理由；Agent 发现工作可能需要拆分时先询问用户，未经明确同意不得自行创建或拆分 Issue）
- **Milestone / Release Gate 管理**（版本验收、收尾退役、大文件拆分）
- **大型工作的拆分与跟踪**（GitHub Sub-issues 原生层级）
- **Git 工作流**（main 开发、release 发版、PR 策略、squash / merge commit）
- **代码修改后检查**（废弃路径 / 死代码清理）
- **发版**（tag 触发、版本号、release notes）
- **Hook 阻断处理**（止血 + 报告）

加载触发：任何 bug 修复 / feature 实现 / PR 创建 / 发版 / Hook 阻断处理。

### 2.1 运行时路径与日志调试

- 默认运行时根目录为 `~/.agents/`；设置 `AEMEATH_AGENTS_DIR` 时，以下全局路径 **MUST** 相对该目录解析。
- 全局配置文件：`~/.agents/aemeath.json`。
- 日志目录：`~/.agents/logs/`。日志按 target 路由到多个文件；具体映射 **MUST** 以 `specs/logging.md` 的 `TargetCatalog` 为准，未注册 target 才写入 `aemeath.log`。
- Session 文件目录：`~/.agents/sessions/`；session lock 位于 `~/.agents/sessions/{session_id}.lock`。
- 日志级别由 `AEMEATH_LOG_LEVEL` 控制，替代旧 `RUST_LOG`；支持全局级别和 per-target directive：

```bash
AEMEATH_LOG_LEVEL=debug cargo run
AEMEATH_LOG_LEVEL=aemeath:tui=debug,aemeath:agent:runtime=trace cargo run
```

完整配置优先级与环境变量规则见 `specs/config-compat.md`，Session 落盘规则见 `specs/storage.md`。

## 3. 渐进式披露（Progressive Disclosure）

根指令（本文件）覆盖全仓库，始终适用。`specs/` 下的分片承载特定工作的 detailed 规则。**开始工作前**先加载匹配的分片，再动手。

两种触发机制，按序应用：

1. **路径触发（主，机械式）**：当工作会读、改或运行某条路径前缀下的代码时，**MUST** 加载对应分片。路径重叠即触发，无需判断。
2. **场景触发（次，语义式）**：当工作触及某分片的范围但未碰它的路径——典型是别处在*消费*该能力（如改 provider 默认值会影响 `prompt` 的 guidance 选择）——也 **MUST** 加载。

多行命中时，加载**全部**匹配分片。分片之间若冲突，停下来问用户，**NEVER** 自行取舍。

### 3.1 架构地图与触发表

| 分片 | 角色 / 路径（主触发） | 同时加载（次触发） |
|---|---|---|
| `specs/rust-coding.md` | 横切，任意 `**/*.rs`、`**/Cargo.toml` —— 编码 / 测试 / 日志 / 验证门禁 / 错误处理 | 跑验证门禁、新增任何核心逻辑、调试构建或测试失败 |
| `specs/tui-cli.md` | `apps/cli/src/**` —— TUI（ratatui）、旧版 REPL（rustyline）、主题色板（Catppuccin Macchiato） | 改输入 / 渲染 / 快捷键 / 选区复制，或新增工具显示（`ToolDisplayEntry`），或改颜色 / 主题 |
| `specs/runtime.md` | `agent/features/runtime/**` —— Agent 循环、tool 执行编排、token budget、compact、成本、slash 命令 | 改暂停 / 恢复 / 重试（同步更新 `token_estimation`）、成本追踪（同步更新 `pricing.rs`）、新增 slash 命令 |
| `specs/tools.md` | `agent/features/tools/**` —— `Tool` trait、`ToolRegistry`、MCP 主体 | 新增内置 Tool，或改 MCP 工具加载 / 注册 |
| `specs/provider.md` | `agent/features/provider/**` —— provider 的 HTTP / stream 实现 | 新增 provider（同步加 model guidance 文件，并在 `config-compat` 补默认值） |
| `specs/prompt.md` | `agent/features/prompt/**` —— Guidance 系统、系统提示、上下文注入 | 改 provider 默认 model（影响 guidance 前缀匹配），或改系统提示注入 |
| `specs/project.md` | `agent/features/project/**` —— worktree 工作区上下文（`WorkspaceService` 单一可变状态源、COLA 分层、git 出站端口） | 改 worktree 进入 / 退出 / 持久化（同步 `storage.md` 会话落盘），或经 slash 命令操作 worktree（涉及 `runtime.md`） |
| `specs/config-compat.md` | `agent/shared/src/config/**` —— 配置分层、provider 默认值 / env / base_url、Claude Code 兼容、运行时路径 | 新增 `AEMEATH_*` 配置项，或改指令 / 配置 / skills / hooks 的读取优先级 |
| `specs/storage.md` | `agent/features/storage/**` —— memory、task、history、tool_result 持久化 | 改会话 / 记忆 / 任务 / 历史的落盘格式或路径 |
| `specs/policy-hook-audit.md` | `agent/features/{policy,hook,audit}/**` —— 权限评估、hook 执行、审计 | 改 hook 执行环境变量注入（`AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR`） |
| `specs/update.md` | `agent/features/update/**` —— 版本检查（GitHub API + semver + 缓存）、`aemeath update` 子命令 | 改版本检查逻辑 / 缓存策略 / 更新渠道配置 |
| `specs/bug-feature-tracking.md` | 无路径触发 | 任何 bug 修复或 feature 实现；操作 GitHub Issues（迁移自 `docs/bug/`、`docs/snapshot/`） |
| `specs/workflow.md` | 无路径触发 | 任何 bug 修复 / feature 实现 / PR 创建 / 发版 / Hook 阻断处理；含项目结构、运行时目录、Git 工作流、Milestone 管理 |
| `specs/logging.md` | `packages/global/logging/**`、全仓库 `log::xxx!` 调用点 —— 日志 target 命名、14 字段 schema、event_type 枚举、级别策略 | 新增/修改 log 调用、改日志路由、改 schema 字段、新增日志文件 |
| `docs/design/03-engineering/01-architecture-guards.md` | `.agents/aemeath.json`、`.agents/hooks/**` —— 架构守卫注册表与 17 个 guard 脚本 | 新增 / 调整守卫、白名单、Hook 编排；Stop 时 `check-architecture-guards.sh` 失败需排查；改 `docs/design/03-engineering/01-architecture-guards.md` 本身 |

> `agent/shared/**`（除 `config/`）、`agent/composition/**`、`packages/**` 的改动按内容落到最相关分片；纯横切改动至少加载 `rust-coding.md`。

## 4. 开放决策

- MCP 工具的动态加载方式（当前通过 `mcp_loader.rs` + `serde_json::Value` 配置）。
- Skill 系统的热重载策略。
- 多模型 pool 的自动故障转移策略。
- Server Foundation MVP 的落地路径（目前仅设计文档，尚无 `server` crate）。**server 代码落地时 MUST 同步在触发表补 `agent/features/server/**` 行并新增对应分片**，否则破坏渐进式披露的路径覆盖。
