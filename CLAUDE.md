# aemeath

A Rust-based AI coding agent with TUI interface. 支持多 provider、多模型、子代理（sub-agent）和技能（skill）系统。

## 项目结构

```
aemeath/                    # workspace root
├── aemeath-core/           # 核心库：消息、工具、配置、会话、成本追踪、压缩
├── aemeath-cli/            # CLI 二进制入口 + TUI (ratatui) + 旧版 REPL (rustyline)
├── aemeath-llm/            # LLM 客户端：provider API 调用、流式响应、模型池
├── aemeath-tools/          # 工具注册：文件读写、搜索、Bash、Agent、Web 等
├── TODO.md                 # 待办事项（通过 /todo 命令维护）
└── CLAUDE.md               # 本文件
```

## 架构约定

### 1. 配置分层（优先级从高到低）
1. CLI 参数（`--provider`、`--model` 等）
2. 环境变量（`AEMEATH_*`、`ANTHROPIC_API_KEY` 等）
3. 项目级配置（`.aemeath/config.json`）
4. 全局配置（`~/.aemeath/config.json`）
5. 硬编码默认值

### 2. Guidance 系统
- Guidance 文件存放在 `~/.aemeath/guidance/`
- `_default.md` — 所有模型通用
- `{prefix}.md` — 按 model id 前缀匹配（最长匹配优先）
- `_reasoning.md` — reasoning 开启时附加
- 首次运行自动生成默认文件，不覆盖用户编辑

### 3. 命令系统
- Slash 命令通过 `CommandRegistry` 注册（`aemeath-core/src/command/`）
- 新增命令步骤：
  1. 在 `aemeath-core/src/command/commands/` 下创建文件
  2. 在 `mod.rs` 的 `builtin` 模块中导出
  3. 在 `registry.rs` 的 `register_defaults()` 中注册
  4. 在 `mod.rs` 的 `cmd` 模块中定义常量（可选）
- 命令自动出现在 TUI 自动补全中（无需额外修改）

### 4. Provider 支持
- Anthropic、OpenAI、OpenRouter、DeepSeek、Moonshot、Zhipu、DashScope、MiniMax、Ollama、OpenAICompatible
- 每个 provider 有默认 base URL、model、API key 环境变量名
- 新 provider 需在 `aemeath-core/src/provider.rs` 和 `aemeath-llm/src/providers/` 添加

### 5. 错误处理
- 统一使用 `AemeathError`（`aemeath-core/src/error.rs`）
- 使用 `thiserror` derive
- `ErrorDisplay` 提供中文用户消息和建议
- `is_retryable()` 区分可重试/不可重试错误

### 6. 工具（Tool）系统
- Tool 通过 `ToolRegistry` 注册（`aemeath-core/src/tool.rs`）
- `aemeath-tools` 中各个工具实现 `Tool` trait
- 执行流程：LLM 返回 tool_use → Agent.execute_tools() → 并发执行 → 结果注入回消息
- MCP 工具通过 `mcp_loader.rs` 动态加载

### 7. 验证门禁
- **CLI 编译**：`cargo build` 或 `cargo build -p <crate>`
- **完整检查**：`cargo check` / `cargo clippy`
- **测试**：`cargo test -p <crate>`
- 库层面 `#![deny(clippy::print_stdout, clippy::print_stderr)]`

## 编码规范

### NEVER
- **NEVER** 在 core 库中使用 `println!` / `eprintln!`（lib.rs 已 deny）
- **NEVER** 直接读取 `process.env` — 使用配置分层
- **NEVER** 硬编码 API key、base URL
- **NEVER** 提交没有单元测试覆盖的新增核心逻辑

### MUST
- **MUST** 使用中文错误消息（`ErrorDisplay`）
- **MUST** 遵循 `AemeathError` 错误类型体系
- **MUST** 配置优先于硬编码默认值
- **MUST** 异步函数使用 `async_trait`（对于 trait 方法）
- **MUST** TUI 模式下所有日志路由到 `~/.aemeath/aemeath.log`
- **MUST** 新增公共函数（`pub fn`）在同一个文件末尾添加 `#[cfg(test)] mod tests`
- **MUST** 单元测试覆盖三种路径：正常路径、边界条件、错误路径
- **MUST** 单个 `.rs` 文件不超过 400 行（含测试代码）。超出时立即拆分职责

### SHOULD
- **SHOULD** 新增 provider 时同步添加 model guidance 文件
- **SHOULD** 修改涉及暂停/恢复/重试逻辑时更新 token_estimation
- **SHOULD** 成本追踪逻辑更新时同步更新 `pricing.rs`
- **SHOULD** 为辅助函数（private fn）编写测试，除非它是一行委托/包装
- **SHOULD** 测试命名遵循 `test_<fn_name>_<scenario>` 模式

## 日志规范
- `env_logger` 驱动，默认级别 `warn,aemeath_llm=debug`
- 日志文件：`~/.aemeath/aemeath.log`（追加模式）
- debug 信息额外写入 `~/.aemeath/debug.log`
- 设置 `AEMEATH_LOG_STDERR=1` 可恢复 stderr 输出（用于 `--no-tui` 模式）

## 测试规范

### 8. 单元测试覆盖

- 8.a **每个包含公共函数的模块文件末尾 MUST 有 `#[cfg(test)] mod tests`**。没有测试的文件就是没有信心的文件。
- 8.b **每个公共函数 MUST 至少有 3 个测试用例**：正常路径（happy path）、边界条件（empty/zero/null/min/max）、错误路径（非法输入、异常状态）。
- 8.c **一行委托/包装函数**（直接调用下层函数，无额外逻辑）可豁免 8.b，但仍 SHOULD 有测试。
- 8.d **测试命名 MUST 使用 `test_<被测函数名>_<场景描述>` 模式**，例如 `test_parse_todos_empty`、`test_toggle_todo_not_found`。
- 8.e **测试 MUST 使用 `assert!` / `assert_eq!` / `matches!` 做显式断言**，不可仅打印然后人工观察。
- 8.f **私有辅助函数 SHOULD 也经过测试**——通过公有函数间接覆盖，或直接 `use super::*` 导入测试。
- 8.g **纯逻辑函数（无 I/O、无副作用）是最高优先级测试目标**。UI 渲染代码、main.rs 入口代码可豁免。
- 8.h **修复 bug 时 MUST 先添加重现该 bug 的测试用例，再提交修复代码**（测试驱动 bug 修复）。
- 8.i Enforcement：Code review 时 reviewer MUST 检查新增代码的测试覆盖。未覆盖核心逻辑的 PR 不应合并。

### 测试示例（参考已有模式）

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_ascii() {
        let tokens = estimate_tokens("hello world");
        assert!(tokens >= 3 && tokens <= 5);
    }

    #[test]
    fn test_format_tokens_edge() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1k");
    }

    #[test]
    fn test_parse_not_command() {
        let parser = CommandParser::new();
        let result = parser.parse("hello world");
        assert!(matches!(result, ParseResult::NotCommand(_)));
    }
}
```

### 哪些文件目前已有测试（作为参考）
- `aemeath-core/src/error.rs` — 错误显示/重试判定
- `aemeath-core/src/guidance.rs` — glob 匹配、前缀匹配、目录初始化
- `aemeath-core/src/session.rs` — Session 序列化/反序列化
- `aemeath-core/src/state.rs` — 状态管理
- `aemeath-core/src/token_estimation.rs` — token 估算
- `aemeath-core/src/command/parser.rs` — 命令解析
- `aemeath-core/src/permission.rs` — 权限判定
- `aemeath-core/src/security.rs` — 安全扫描
- `aemeath-core/src/config/mod.rs` — 配置加载
- `aemeath-core/src/history.rs` — 历史记录
- `aemeath-core/src/skill.rs` — Skill 加载
- `aemeath-core/src/cost/mod.rs` — 成本追踪

### 9. Bug/Feature 追踪联动

- **开始工作前 MUST 查看 `docs/bug/active.md` 和 `docs/feature/active.md`**，确认当前修改是否与已知 bug 或 feature 相关
- **修改涉及已知 bug 时 MUST**：
  1. 在 `docs/bug/active.md` 的对应行标记状态（修复中/待回归等）
  2. 在 commit message 中引用 bug 编号（如 `refs #1`）
  3. 修复后将 commit hash 更新到归档文件的"修复"字段
- **新增 bug 发现时 MUST**：
  1. 先在 `docs/bug/archived/` 创建详细文件（用模板）
  2. 再在 `docs/bug/active.md` 表格中添加一行
- **实现 feature 时 MUST**：
  1. 在 `docs/feature/active.md` 登记
  2. 完成归档后从 `active.md` 移除，文件移入 `archived/`
- **归档门禁**：bug 修复或 feature 完成后，**MUST 等待用户确认**，确认后才能在 `active.md` 中移除该条目并将文件移入 `archived/`

## 开放决策
- MCP 工具的动态加载方式（当前通过 `mcp_loader.rs` + `serde_json::Value` 配置）
- Skill 系统的热重载策略
- 多模型 pool 的自动故障转移策略
