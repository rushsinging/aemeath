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

## Progressive Disclosure

根级 CLAUDE.md 定义仓库级规则。作用域级说明存放在 `specs/` 下，操作对应区域前 **MUST** 先读取。

- 涉及 `aemeath-core/`：读取 `specs/core.md`。
- 涉及 `aemeath-cli/`：读取 `specs/cli.md`。
- 涉及 `aemeath-llm/`：读取 `specs/llm.md`。
- 涉及 `aemeath-tools/`：读取 `specs/tools.md`。

当多条作用域规则同时适用时，全部遵循。如果作用域规则与根规则出现冲突，停止操作并询问用户。

## 项目结构

```
aemeath/                    # workspace root
├── aemeath-core/           # 核心库：消息、工具、配置、会话、成本追踪、压缩
├── aemeath-cli/            # CLI 二进制入口 + TUI (ratatui) + 旧版 REPL (rustyline)
├── aemeath-llm/            # LLM 客户端：provider API 调用、流式响应、模型池
├── aemeath-tools/          # 工具注册：文件读写、搜索、Bash、Agent、Web 等
├── specs/                  # 作用域级说明（core / cli / llm / tools）
├── docs/                   # bug / feature 追踪（active.md + archived/）
├── TODO.md                 # 待办事项（通过 /todo 命令维护）
└── CLAUDE.md               # 本文件
```

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
- **MUST** TUI 模式下所有日志路由到 `~/.aemeath/aemeath.log`。
- **MUST** 新增 `pub fn` 在同一文件末尾添加 `#[cfg(test)] mod tests`。
- **MUST** 单元测试覆盖三种路径：正常路径、边界条件、错误路径。
- **MUST** 单个 `.rs` 文件不超过 400 行（含测试代码）。超出时立即拆分职责。
- **MUST** 开始工作前查看 `docs/bug/active.md` 和 `docs/feature/active.md`，确认当前修改是否与已知条目相关。
- **MUST** 修复 bug 时先添加重现该 bug 的测试用例，再提交修复代码。

### SHOULD
- **SHOULD** 新增 provider 时同步添加 model guidance 文件。
- **SHOULD** 修改涉及暂停/恢复/重试逻辑时更新 `token_estimation`。
- **SHOULD** 成本追踪逻辑更新时同步更新 `pricing.rs`。
- **SHOULD** 为辅助函数（`private fn`）编写测试，除非是一行委托/包装。
- **SHOULD** 测试命名遵循 `test_<被测函数名>_<场景描述>` 模式。

## 架构约定

### 1. 配置分层（优先级从高到低）
1. CLI 参数（`--provider`、`--model` 等）
2. 环境变量（`AEMEATH_*`、`ANTHROPIC_API_KEY` 等）
3. 项目级配置（`.aemeath/config.json`）
4. 全局配置（`~/.aemeath/config.json`）
5. 硬编码默认值

### 2. Guidance 系统
- Guidance 文件存放在 `~/.aemeath/guidance/`。
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

## 验证门禁

- **CLI 编译**：`cargo build` 或 `cargo build -p <crate>`
- **完整检查**：`cargo check` / `cargo clippy`
- **测试**：`cargo test -p <crate>`
- 库层面 `#![deny(clippy::print_stdout, clippy::print_stderr)]`

## 日志规范

- `env_logger` 驱动，从配置文件的 `logging` 段读取 module_levels（默认 `aemeath=debug`）。
- 日志文件：`~/.aemeath/aemeath.log`（追加模式）。
- Panic 日志：`~/.aemeath/panic.log`。
- Agent 审计日志：`~/.aemeath/agent.log`（LLM 请求/响应摘要、token 用量）。
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

- **Bug 状态流程**：`活动中` → `修复中` → `待确认` → 用户确认后归档。
- **修改涉及已知 bug 时 MUST**：
  1. 在 `docs/bug/active.md` 的对应行更新状态。
  2. 在 commit message 中引用 bug 编号（如 `refs #1`）。
  3. 修复后将 commit hash 更新到归档文件的"修复"字段。
- **新增 bug 发现时 MUST**：在 `docs/bug/active.md` 表格中添加行（状态"活动中"），并在详情区域记录症状、根因、修复方向。
- **实现 feature 时 MUST**：在 `docs/feature/active.md` 登记，完成后归档。
- **归档门禁**：bug 修复或 feature 完成后，**MUST** 等待用户确认，确认后从 `active.md` 移除并将详情总结到 `archived/`。

## 开放决策

- MCP 工具的动态加载方式（当前通过 `mcp_loader.rs` + `serde_json::Value` 配置）。
- Skill 系统的热重载策略。
- 多模型 pool 的自动故障转移策略。
