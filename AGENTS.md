# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## Constitution

1. 指令使用 **NEVER**、**MUST**、**SHOULD**、**MAY** 表示约束等级：
   - **NEVER**：绝对禁止，无例外。
   - **MUST**：强制要求。违反会导致构建失败、数据丢失或行为错误。
   - **SHOULD**：强烈推荐。跳过时必须有明确理由。
   - **MAY**：可选。有助于清晰度或便利性时使用。
2. 所有新增指令 **MUST** 遵循此约定。
3. 引入新规则或约定时，**MUST** 先征得用户同意再更新本文件或任一 `specs/` 分片。
4. **MUST** 遵循 DRY 原则。类型、辅助函数、常量、数据访问逻辑 **MUST** 定义一次、到处引用。**NEVER** 在多个文件重复相同逻辑。
5. **MUST NOT** 手动调整代码格式。格式化由 `cargo fmt` / `rustfmt` 处理，只关注逻辑变更。

## 渐进式披露（Progressive Disclosure）

根指令（本文件）覆盖全仓库，始终适用。`specs/` 下的分片承载特定工作的 detailed 规则。**开始工作前**先加载匹配的分片，再动手。

两种触发机制，按序应用：

1. **路径触发（主，机械式）**：当工作会读、改或运行某条路径前缀下的代码时，**MUST** 加载对应分片。路径重叠即触发，无需判断。
2. **场景触发（次，语义式）**：当工作触及某分片的范围但未碰它的路径——典型是别处在*消费*该能力（如改 provider 默认值会影响 `prompt` 的 guidance 选择）——也 **MUST** 加载。

多行命中时，加载**全部**匹配分片。分片之间若冲突，停下来问用户，**NEVER** 自行取舍。

### 架构地图与触发表

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
| `specs/logging.md` | `packages/global/logging/**`、全仓库 `log::xxx!` 调用点 —— 日志 target 命名、14 字段 schema、event_type 枚举、级别策略 | 新增/修改 log 调用、改日志路由、改 schema 字段、新增日志文件 |
| `docs/design/architecture-guards.md` | `.agents/aemeath.json`、`.agents/hooks/**` —— 架构守卫注册表与 17 个 guard 脚本 | 新增 / 调整守卫、白名单、Hook 编排；Stop 时 `check-architecture-guards.sh` 失败需排查；改 `docs/design/architecture-guards.md` 本身 |

> `agent/shared/**`（除 `config/`）、`agent/composition/**`、`packages/**` 的改动按内容落到最相关分片；纯横切改动至少加载 `rust-coding.md`。

## 项目结构

```
aemeath/                    # workspace root
├── apps/
│   └── cli/                # CLI 二进制入口 + TUI (ratatui) + 旧版 REPL (rustyline)
├── agent/
│   ├── features/           # 业务 feature：runtime/tools/provider/prompt/project/storage/policy/hook/audit/update
│   ├── shared/             # 横切基础设施 + 外部 adapter + 最小共享内核
│   └── composition/        # 组合根：唯一生产装配入口
├── packages/
│   ├── sdk/                # AgentClient trait + 公共类型（CLI↔Runtime 通信契约）
│   └── global/logging/     # 日志 projection 适配
├── docs/                   # 设计真相与历史归档
│   ├── design/             #   设计真相源：outline / runtime / tui / server / file-split-plan / architecture-guards
│   ├── snapshot/           #   历史 spec 快照（已废弃、不参与运行时）
│   ├── superpowers/        #   brainstorm / superpowers 工作流产物
│   ├── mockups/            #   UI 草图
│   └── visual/             #   可视化资产
├── specs/                  # 渐进式披露分片：按需加载的 detailed 规则
├── TODO.md                 # 待办事项（通过 /todo 命令维护）
└── AGENTS.md               # 本文件（CLAUDE.md 软链指向它）
```

> bug / feature 追踪改在 GitHub Issues（仓库 `rushsinging/aemeath`），**NEVER** 再向 `docs/` 写入新的 bug / feature 条目。

## 运行时目录（`~/.agents`）

```
~/.agents/                   # 运行时根目录
├── aemeath.json             # 全局配置
├── AGENTS.md                # 全局指令
├── guidance/                # 模型 Guidance 文件
│   ├── _default.md          #   所有模型通用
│   ├── _reasoning.md        #   reasoning 开启时附加
│   └── {prefix}.md          #   按 model id 前缀匹配（最长优先）
├── hooks/                   # 全局 Hook 脚本
├── logs/                    # 日志文件（12 个 target 路由文件 + 兜底 + panic）
│   ├── aemeath.log          #   兜底日志（不匹配任何合法 target 时路由到此）
│   ├── panic.log            #   Panic 日志（panic_hook.rs 直写，不纳入 UnifiedLogger）
│   ├── tui.log              #   aemeath:tui — TUI / CLI 入口
│   ├── shared.log           #   aemeath:shared — shared 层
│   ├── composition.log      #   aemeath:composition — composition 层
│   ├── agent-provider.log   #   aemeath:agent:provider — provider + LLM 输入/输出
│   ├── agent-runtime.log    #   aemeath:agent:runtime — agent 循环
│   ├── agent-tools.log      #   aemeath:agent:tools — tool 执行
│   ├── agent-prompt.log     #   aemeath:agent:prompt — Guidance 系统
│   ├── agent-hook.log       #   aemeath:agent:hook — hook 执行
│   ├── agent-storage.log    #   aemeath:agent:storage — 持久化
│   ├── agent-project.log    #   aemeath:agent:project — worktree 管理
│   ├── agent-policy.log     #   aemeath:agent:policy — 权限评估
│   └── agent-audit.log      #   aemeath:agent:audit — 审计事件
> **废弃文件**：`input.log` / `output.log` / `tool.log` / `audit.log` / `agent.log` 已废弃，NEVER 再使用。详见 `specs/logging.md`。
>
> **终端测试**：`echo '{prompt}' | AEMEATH_VERSION= RUST_LOG= cargo run -- -qv`（`-q` 静默模式 + `-v` 日志输出到 stderr，适合非交互式 CLI 测试；`AEMEATH_VERSION=` 覆盖版本号、`RUST_LOG=` 覆盖日志级别，方便测试）。
├── memory/                  # 持久化记忆存储
├── sessions/                # 会话持久化
├── skills/                  # 全局 Skills
├── mcp.json                 # MCP 工具配置
├── history.json               # 用户输入历史
├── cost_history.json          # 成本追踪历史
└── settings.json              # 全局设置
```

## 工作流约束

### Bug / Feature 执行流程

bug / feature 追踪改在 GitHub Issues（仓库 `rushsinging/aemeath`），按以下步骤执行，**NEVER** 跳过：

1. **阅读 Issue**：用 `gh issue view <编号> --repo rushsinging/aemeath` 拉取 issue 标题、labels、完整 body（body 顶部有 `<!-- Migrated from: <source> -->` 标记，可追溯到原 `docs/bug/archived/<id>-<slug>.md` 或 `docs/active.md#<id>`）。设计稿类 issue **SHOULD** 配套阅读 `docs/snapshot/specs/<file>.md`，每份 spec 顶部已写入 `> 对应 Issue: <url>` 指针。
2. **定位问题并给出方案**：阅读相关源码，定位根因或设计点，**MUST** 向用户输出可执行的修复/实现方案（含改动范围、根因分析、验证计划）。方案中的任务必须拆分为单一、具体、可验证的最小步骤（如“修改 A 文件的 B 函数”“为 C 场景添加测试”“运行 cargo clippy”），**NEVER** 使用“实施并验证”这类宽泛任务概括多个步骤。复杂改动 **MUST** 调用 `superpowers:writing-plans` 制定详细计划；即使是简单改动，也 **MUST** 先给出简明方案，禁止直接开始修改。
3. **等待用户明确同意**：在获得用户的明确书面同意（如“同意”、“开始改”）前，**NEVER** 调用 Edit/Write/Bash 等会修改文件或系统状态的工具。如果用户只给出笼统意图（如“修一下”）而未确认具体方案，**MUST** 先呈现方案并等待确认。
4. **执行与验证**：在 worktree 中实施，通过编译、测试、clippy 验证后合并回 `main`。
5. **用户确认后关闭 Issue**：agent **NEVER** 自行关闭 issue。合并完成后，由用户确认是否关闭；用户确认后，可用 `gh issue close <编号> --repo rushsinging/aemeath --comment "..."` 关闭 issue，comment 引用合并 commit / PR。

修复 bug 或实现 feature 时，**MUST** 做根因层面的修正（fact-check），而不是只做最小化补丁绕过症状。应评估相关代码路径、数据流和状态机，确保同类问题不再复发。

如果同一问题既可临时止血也能彻底重构，**MUST** 同时给出最小化补丁和根因级彻底方案，并说明两者优劣、成本与风险。对于存在复发风险、明显设计缺陷或结构性不合理的情况，**SHOULD** 默认推荐并优先实施彻底方案，除非用户明确要求只做最小修改。

创建新 issue 时 **MUST** 应用 `kind:bug` 或 `kind:feature` label（按问题类型二选一），有明确优先级时再加 `priority:high|medium|low`。`migrated-from:docs` 仅用于历史迁移条目。

### Git 工作流

- **MUST** 所有代码、文档、配置修改都在独立 git worktree 中执行，NEVER 直接在 `main` 工作区修改。
- **MUST** worktree 分支完成验证并提交后，通过 **Pull Request** 提交回 `main`；**NEVER** 直接 push 到 `main`（`main` 已受保护，不允许直接推送）。
- **MUST NEVER** 由 agent 自动合并 PR。PR 创建后由用户 review，用户确认后手动合并；agent 只能在用户明确授权后执行合并动作。
- **MUST** 创建 PR 前，在 worktree 分支上执行 `git pull origin main` 拉取最新 main；若存在冲突，解决后重新通过验证门禁，才能推送分支并创建 PR。

### 发版

发版通过 push `v*` tag 触发 `.github/workflows/release.yml` 自动完成（校验 → macOS 双架构构建 → 创建 GitHub Release + checksums）。**MUST** 遵守：

- **MUST** 由用户明确指定版本号（如"发 0.0.2"），agent **NEVER** 自行决定发版或推演版本号。
- **MUST** tag 打在 `origin/main` 最新 commit 上（已 review、已同步到远端），**NEVER** 打在本地未 push 的 HEAD 上。本地若有未 push 的 commit，**MUST** 先判断其是否为已通过 PR 合入内容的噪音（如 merge commit、仅含 `.agents/*` 等运行时副产物的提交）；若是则 tag 仍打在 `origin/main`，发版后 reset 清理。
- **MUST** 使用轻量 tag（沿用 v0.0.1 风格），格式 `vX.Y.Z`。**NEVER** 改 `Cargo.toml` 的 `workspace.version`（占位符 `0.0.0`）；实际版本由 `release.yml` 的 `build` job 显式注入 `AEMEATH_VERSION` env（取自 `validate.outputs.version`），`build.rs` 不再从 git 历史读 tag。
- **MUST** push tag 前先向用户输出方案（tag 指向哪个 commit、版本号、自上一 tag 以来的变更摘要）并等待确认；**NEVER** 未经确认直接 push tag。
- **MUST** push 后用 `gh run list --workflow=release.yml` 监控三个阶段（Validate & Gate / Build ×2 / Create Release）全部通过；任一失败 **MUST** 排查并报告。
- release notes 由 `generate_release_notes: true` 自动从 PR 生成，agent **NEVER** 手写发版说明。
- **MUST** 用 `gh release view vX.Y.Z` 确认 Release 已发布且包含 aarch64 / x86_64 tar.gz + checksums.txt 三个 asset。

### Hook 阻断处理

工作中若遇到 hook 阻断（例如 PreToolUse 阻止 Edit/Write）：

1. **MUST** 先止血：立即切换到正确的工作上下文（如进入 git worktree），让用户请求的原始操作能够继续执行。
2. **MUST** 向用户报告：发生了什么阻断、阻断原因、以及采取了什么措施来处理。

## 开放决策

- MCP 工具的动态加载方式（当前通过 `mcp_loader.rs` + `serde_json::Value` 配置）。
- Skill 系统的热重载策略。
- 多模型 pool 的自动故障转移策略。
- Server Foundation MVP 的落地路径（目前仅设计文档，尚无 `server` crate）。**server 代码落地时 MUST 同步在触发表补 `agent/features/server/**` 行并新增对应分片**，否则破坏渐进式披露的路径覆盖。
