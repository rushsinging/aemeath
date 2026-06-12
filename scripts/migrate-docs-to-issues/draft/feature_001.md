<!-- Migrated from: docs/feature/archived/001-hook-system.md -->
# #1 Hook 功能（参考 Claude Code 设计）

**归档日期**：2026-05-01

**目标**：参考 Claude Code 的 hook 系统，在关键生命周期点执行用户自定义 shell 命令，支持注入上下文、阻止操作、修改输入。

**实现**：

### 已落地事件

| 事件 | 说明 |
|------|------|
| PreToolUse | 工具执行前，可阻止 / 修改输入 |
| PostToolUse | 工具执行后，注入上下文 |
| PostToolUseFailure | 工具失败后，注入修复指导 |
| UserPromptSubmit | 用户输入提交前，检查 / 修改 / 拒绝 |
| Stop | Agent 停止前，质量门 |
| StopFailure | API 错误，观察性 |
| SessionStart | 会话开始，注入上下文 |
| PreCompact | 上下文压缩前 |
| PostToolBatch | 批量工具完成后汇总 |

### JSON 输出协议

exit 0 + stdout JSON 支持：
- `continue: false` + `stopReason` — 全局停止
- `decision: "block"` + `reason` — 阻止操作
- `additionalContext` — 注入额外上下文
- `systemMessage` — 系统警告
- `hookSpecificOutput` — PreToolUse 特定控制（allow/deny/ask + updatedInput）

exit 2 = 阻止操作，stderr 反馈给 LLM；其他非 0 = 非阻塞错误。

### 设计原则

- 阻止操作时，反馈消息传给 LLM 让其继续调整，不中断用户交互
- 所有注入上下文在 LLM 对话流中可见
- 不新增 UI 事件类型
- HookRunner 自动传入 `AEMEATH_PROJECT_DIR` 环境变量

**修复 commits**：
- `6e3aacc` — 实现 Hook 生命周期系统（PreToolUse/PostToolUse/Stop/UserPrompt）
- `996b281` — Hook 系统全面扩展 + CLI 子命令重构 + ToolDisplay inventory 注册
- `f93b2e6` — HookRunner 传入 project_dir 作为 AEMEATH_PROJECT_DIR 环境变量

**涉及文件**：
- `aemeath-core/src/hook.rs` — 数据结构 + JSON 输出解析
- `aemeath-core/src/config/hooks.rs` — 事件枚举 + 配置
- `aemeath-cli/src/tui/app/input_handler.rs` — UserPromptSubmit 调用
- `aemeath-cli/src/tui/app/stream.rs` — PostToolUse / PostToolUseFailure / PostToolBatch / PreCompact
- `aemeath-cli/src/tui/app/update.rs` — StopFailure
- `aemeath-cli/src/main.rs` — SessionStart

**未纳入本期**：
- SessionEnd / PostCompact 配对事件
- SubagentStart / SubagentStop（依赖 Feature #2 SubAgent）
- TaskCreated / TaskCompleted（反思系统天然触发点）
- PermissionRequest / PermissionDenied（P2）
- Notification / InstructionsLoaded / ConfigChange（P2）
- Elicitation / ElicitationResult（依赖 MCP 完善度）
- UserPromptExpansion / CwdChanged / FileChanged / TeammateIdle（P3，按需）
- WorktreeCreate / WorktreeRemove（aemeath 不支持，**不实施**）

后续若需要扩展事件，可在现有 HookEvent 枚举上追加。
