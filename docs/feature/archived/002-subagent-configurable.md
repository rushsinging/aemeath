# #2 SubAgent 可配置

**归档日期**：2026-05-01

**目标**：支持通过配置文件定义 agent role（绑定 model、description、system_suffix），Agent tool 通过 `role` / `model` 参数路由到不同 LLM，让用户可以为不同任务派发不同能力的子 agent。

**实现**：

- 配置层支持 `agents` 数组，每条 agent 可绑定独立的 `model`、`description`、`system_suffix`
- Agent tool 接受 `role` 或 `model` 参数，运行时按该参数从 agent 池中选择对应配置
- `LLM pool` 支持运行时切换不同 provider / model
- TUI Agent tool call header 显示 role / model / description，让用户能看到子 agent 走的是哪条配置

**修复 commits**：
- `5a53bd0` — reasoning 运行时可切换 + agent role description + LLM pool
- `b5e660b` — Agent tool call 显示 role/model

**涉及文件**：
- `aemeath-core/src/config/`（agent 配置层）
- `aemeath-tools/src/agent/`（Agent tool role/model 路由）
- `aemeath-cli/src/agent_runner.rs`（agent 调度）
- `aemeath-cli/src/tui/output_area/tool_display.rs`（role/model header 显示）
- `aemeath-llm/src/`（LLM pool）

**未纳入本期**：
- SubagentStart / SubagentStop hook 事件（待 Feature #1 hook 系统扩展时补）
- 子 agent 进度的更细粒度 UI 反馈（已有 `[Turn N] calling: ...` 基础反馈）
- 子 agent 间共享 input queue / 上下文穿透（保持父子隔离）
