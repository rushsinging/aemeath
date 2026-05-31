# aemeath

基于 Rust 的 AI 编程助手，带 TUI 界面。支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## Constitution

1. 指令使用 **NEVER**、**MUST**、**SHOULD**、**MAY** 表示约束等级：
   - **NEVER**：绝对禁止，无例外。
   - **MUST**：强制要求。违反会导致构建失败、数据丢失或行为错误。
   - **SHOULD**：强烈推荐。跳过时必须有明确理由。
   - **MAY**：可选。有助于清晰度或便利性时使用。
2. 所有新增指令 **MUST** 遵循此约定。
3. 引入新规则或约定时，**MUST** 先征得用户同意再更新本文件。
4. **MUST** 遵循 DRY 原则。类型、辅助函数、常量、数据访问逻辑 **MUST** 定义一次、到处引用。**NEVER** 在多个文件重复相同逻辑。
5. **MUST NOT** 手动调整代码格式。格式化由 `cargo fmt` / `rustfmt` 处理，只关注逻辑变更。

## 项目结构

```
aemeath/                    # workspace root
├── apps/
│   ├── cli/                # CLI 二进制入口 + TUI (ratatui) + 旧版 REPL (rustyline)
│   ├── server/             # #36 API Server：REST/WS + gRPC
│   └── agents/             # #36 Agent runtime 与角色配置
├── packages/
│   ├── core/               # 核心库：消息、工具、配置、会话、成本追踪、压缩
│   ├── llm/                # LLM 客户端：provider API 调用、流式响应、模型池
│   ├── tools/              # 工具注册：文件读写、搜索、Bash、Agent、Web 等
│   ├── proto/              # 共享 proto 定义
│   └── sdk/                # 外部 SDK
├── infra/                  # MongoDB、部署与本地开发编排
├── docs/                   # bug / feature 追踪（active.md + archived/）
├── TODO.md                 # 待办事项（通过 /todo 命令维护）
└── AGENTS.md               # 本文件
```

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
├── logs/                    # 日志文件
│   ├── aemeath.log          #   应用主日志（追加模式）
│   ├── panic.log            #   Panic 日志
│   └── agent.log            #   审计日志（已废弃）
├── memory/                  # 持久化记忆存储
├── sessions/                # 会话持久化
├── skills/                  # 全局 Skills
├── mcp.json                 # MCP 工具配置
├── history.json               # 用户输入历史
├── cost_history.json          # 成本追踪历史
└── settings.json              # 全局设置
```

## 工作流约束

- **MUST** 所有代码、文档、配置修改都在独立 git worktree 中执行，NEVER 直接在 `main` 工作区修改。
- **MUST** worktree 分支完成验证并提交后合并回 `main`。
- **MUST** worktree 分支合并回 `main` 前，先在 `main` 工作区执行 `git pull`（或等价的 fetch + fast-forward）拉取最新更新，再从 worktree 合并；若拉取后存在冲突，**MUST** 在 worktree 分支上 rebase/merge 最新 `main` 并重新通过验证后才能合并。
- **MUST** 合并回 `main` 后在 `main` 上运行对应验证，并清理已完成的 worktree。

## 编码规范

### NEVER
- **NEVER** 在 core 库中使用 `println!` / `eprintln!`（`lib.rs` 已 deny）。
- **NEVER** 直接读取环境变量——使用配置分层。
- **NEVER** 硬编码 API key、base URL。
- **NEVER** 提交没有单元测试覆盖的新增核心逻辑。
- **NEVER** 在 `update()` 中直接调用 `tokio::spawn`、hook notification、clipboard/image 等副作用——所有副作用通过 `Cmd` 描述并由 runtime 执行。

### MUST
- **MUST** 错误消息使用中文（`ErrorDisplay`）。
- **MUST** 遵循 `AemeathError` 错误类型体系。
- **MUST** 配置优先于硬编码默认值。
- **MUST** 异步 trait 方法使用 `async_trait`。
- **MUST** TUI 模式下所有应用主日志路由到 `~/.agents/logs/aemeath.log`。
- **MUST** 新增 `pub fn` 在同一文件末尾添加 `#[cfg(test)] mod tests`。
- **MUST** 单元测试覆盖三种路径：正常路径、边界条件、错误路径。
- **MUST** 开始工作前查看 `docs/bug/active.md` 和 `docs/feature/active.md`，确认当前修改是否与已知条目相关。
- **MUST** 修复 bug 时先添加重现该 bug 的测试用例，再提交修复代码。
- **MUST** 解决 bug 或完成 feature 后，同步更新 `docs/bug/active.md` 或 `docs/feature/active.md`，记录问题、解决思路和当前解决状态。

### SHOULD
- **SHOULD** 单个 `.rs` 文件控制在 400 行以内（含测试代码）；过长时按职责拆分。无强制守卫，超限不阻断构建。
- **SHOULD** 新增 provider 时同步添加 model guidance 文件。
- **SHOULD** 修改涉及暂停/恢复/重试逻辑时更新 `token_estimation`。
- **SHOULD** 成本追踪逻辑更新时同步更新 `pricing.rs`。
- **SHOULD** 为辅助函数（`private fn`）编写测试，除非是一行委托/包装。
- **SHOULD** 测试命名遵循 `test_<被测函数名>_<场景描述>` 模式。

## 架构约定

### 1. 配置分层（优先级从高到低）
1. CLI 参数（`--provider`、`--model` 等）
2. 环境变量（`AEMEATH_*`、`ANTHROPIC_API_KEY` 等）
3. 项目级配置：`.agents/aemeath.json` 优先，其次兼容 `.claude/settings.json` 的 hooks 配置
4. 全局配置（`~/.agents/aemeath.json`）
5. 硬编码默认值

### 2. Guidance 系统
- Guidance 文件存放在 `~/.agents/guidance/`。
- `_default.md` — 所有模型通用。
- `{prefix}.md` — 按 model id 前缀匹配（最长匹配优先）。
- `_reasoning.md` — reasoning 开启时附加。
- 首次运行自动生成默认文件，不覆盖用户编辑。

### 3. 命令系统
- Slash 命令通过 `inventory` crate + `CommandRegistry` 单例自动收集（`aemeath-core/src/command/`）。
- 新增命令只需两步：
  1. 在 `aemeath-core/src/command/commands/` 下创建文件，用 `inventory::submit!` 声明命令。
  2. 在 `commands/mod.rs` 添加 `pub mod <name>;`。
- `CommandRegistry::initialize()` 在启动时自动遍历所有 `submit!` 的命令并注册。
- 命令自动出现在 TUI 自动补全中，无需额外修改。

### 4. Provider 支持
- Anthropic、OpenAI、OpenRouter、DeepSeek、Moonshot、Zhipu、DashScope、MiniMax、Ollama、OpenAICompatible。
- 每个 provider 有默认 base URL、model、API key 环境变量名。
- 新 provider 需在 `aemeath-core/src/provider.rs` 和 `aemeath-llm/src/providers/` 添加。

### 5. 错误处理
- 统一使用 `AemeathError`（`aemeath-core/src/error.rs`），`thiserror` derive。
- `ErrorDisplay` 提供中文用户消息和建议。
- `is_retryable()` 区分可重试/不可重试错误。

### 6. 工具（Tool）系统
- Tool 通过 `ToolRegistry` 注册（`aemeath-core/src/tool.rs`）。
- `aemeath-tools` 中各个工具实现 `Tool` trait。
- 执行流程：LLM 返回 tool_use → `Agent.execute_tools()` → 并发执行 → 结果注入回消息。
- MCP 工具通过 `mcp_loader.rs` 动态加载。

### 7. Claude Code 兼容
- 项目指令读取 **MUST** Claude 优先：`{cwd}/CLAUDE.md` 优先，其次 `{cwd}/AGENTS.md`；全局指令仍读取 `~/.agents/AGENTS.md`。
- 项目配置读取 **MUST** `.agents/aemeath.json` 优先，其次兼容 `.claude/settings.json`；Claude Code hooks 结构需转换为 Aemeath hooks。
- 项目 skills 读取 **MUST** `.claude/skills` 优先，其次 `.agents/skills`；同名 skill 以 Claude Code 项目 skill 为准。
- Hook 执行环境 **MUST** 同时注入 `AEMEATH_PROJECT_DIR` 与 `CLAUDE_PROJECT_DIR`，兼容现有 Claude Code hook 脚本。

## 验证门禁

- **CLI 编译**：`cargo build` 或 `cargo build -p <crate>`
- **完整检查**：`cargo check` / `cargo clippy`
- **测试**：`cargo test -p <crate>`
- 库层面 `#![deny(clippy::print_stdout, clippy::print_stderr)]`

## 日志规范

- `env_logger` 驱动，从配置文件的 `logging` 段读取 module_levels（默认 `aemeath=debug`）。
- 日志文件：`~/.agents/logs/aemeath.log`（追加模式）。
- Panic 日志：`~/.agents/logs/panic.log`。
- Agent 审计日志：`~/.agents/logs/agent.log`（已废弃，保留兼容枚举；当前无写入点）。
- 设置 `AEMEATH_LOG_STDERR=1` 可恢复 stderr 输出（用于 `--no-tui` 模式）。

## 测试规范

- **MUST** 每个包含公共函数的模块文件末尾有 `#[cfg(test)] mod tests`。
- **MUST** 每个公共函数至少 3 个测试用例：正常路径、边界条件、错误路径。
- **MUST** 测试使用 `assert!` / `assert_eq!` / `matches!` 显式断言，不可仅打印后人工观察。
- **SHOULD** 私有辅助函数通过公有函数间接覆盖，或直接 `use super::*` 导入测试。
- **MUST** 纯逻辑函数（无 I/O、无副作用）为最高优先级测试目标。UI 渲染代码、`main.rs` 入口代码可豁免。
- 一行委托/包装函数可豁免 3 测试用例要求，但仍 SHOULD 有测试。
- Code review 时 reviewer **MUST** 检查新增代码的测试覆盖。未覆盖核心逻辑的 PR 不应合并。

## Bug/Feature 追踪联动

- **编号独立**：bug 与 feature **NEVER** 共享编号序列，各自独立递增。bug 编号取 `docs/bug/active.md` 与 `docs/bug/archived/` 的最大值 +1；feature 编号取 `docs/feature/active.md` 与 `docs/feature/archived/` 的最大值 +1。新增条目前 **MUST** 在对应类别（bug 或 feature）内核对最大编号，不得跨类别取号。
- **Bug 修复 MUST 使用 git worktree**：修复 bug 或实现 feature 时，**MUST** 在独立 git worktree 中执行所有修改，NEVER 直接在 `main` 工作区修改。worktree 分支完成验证并提交后合并回 `main`，在 `main` 上运行对应验证后清理已完成的 worktree。详见上方工作流约束。
- **Bug 状态流程**：`活动中` → `修复中` → `待确认` → 用户确认后归档。
- **修改涉及已知 bug 时 MUST**：
  1. 在 `docs/bug/active.md` 的对应行更新状态。
  2. 在 commit message 中引用 bug 编号（如 `refs #1`）。
  3. 修复后将 commit hash 更新到归档文件的"修复"字段。
- **新增 bug 发现时 MUST**：在 `docs/bug/active.md` 表格中添加行（状态"活动中"），并在详情区域记录症状、根因、修复方向。
- **实现 feature 时 MUST**：在 `docs/feature/active.md` 登记，完成后归档。
- **归档门禁**：bug 修复或 feature 完成后，**MUST** 等待用户确认，确认后从 `active.md` 移除并将详情总结到 `archived/`。在 `main` 上更新文档后 **MUST** 立即提交，不与其他改动混入同一 commit。

## 开放决策

- MCP 工具的动态加载方式（当前通过 `mcp_loader.rs` + `serde_json::Value` 配置）。
- Skill 系统的热重载策略。
- 多模型 pool 的自动故障转移策略。
