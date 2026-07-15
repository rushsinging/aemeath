# Runtime 引擎

**Scope**：`agent/features/runtime/**`——Agent 主循环、tool 执行编排、token budget、对话压缩（compact）、成本追踪、slash 命令系统。
**主触发**：改 `agent/features/runtime/**`。
**次触发**：改暂停 / 恢复 / 重试逻辑；改成本追踪；新增 slash 命令。
**配套**：`Tool` trait / `ToolRegistry` / MCP 主体在 `tools.md`；provider 调用在 `provider.md`。

## 会话历史唯一真相（#680）

- **MUST** 会话历史唯一可变真相 = `RuntimeHandle.current_chain: Arc<Mutex<ChatChain>>`。
- **MUST** segment 边界由 loop turn 开始时生成 segment_id，`chain.push(msg, &segment_id)` 指定段追加。
- **NEVER** 在 loop 外部（TUI / SDK / storage）持有可变消息副本或回写权威态。
- **MUST** save 完全是 runtime 自身职责（turn-level + loop-exit auto-save），TUI `/save` 仅 UX 反馈。
- **MUST** `ChatRequest` 只传增量 `user_input`，**NEVER** 传全量消息历史。
- microcompact 按 segment 边界保护最近 3 个大 loop（`microcompact_chain`）。

## Tool 执行编排

- 执行流程：LLM 返回 tool_use → Agent 收集 → 并发执行 → 结果注入回消息。
- `Tool` trait 与 `ToolRegistry` 的定义在 `agent/features/tools`（见 `tools.md`）；本分片只负责循环里的调度与结果回填。

## token budget / 压缩 / 成本

- token 估算由 Context BC 的 `context::api::compact::estimate_tokens` 提供，Runtime 在 `application/{agent,chat}` 编排中消费。
- **SHOULD** 修改涉及暂停 / 恢复 / 重试逻辑时同步检查 Context token estimation 调用点。
- 成本追踪与定价：`agent/features/runtime/src/application/cost/pricing.rs`。
- **SHOULD** 成本追踪逻辑更新时同步更新 `pricing.rs`。
- 成本历史落盘在 `~/.agents/cost_history.json`。

## slash 命令系统

- slash 命令的 SDK/TUI 入站路由属于 `application/client`；具体命令能力由对应 Feature 的 Published Language / Tool 提供。
- Runtime 内不再维护 `core/command` 固定层注册表；新增命令时按实际所有者更新 SDK、TUI 或对应 Feature，**NEVER** 恢复旧 `core/command/` 路径。
- 命令在 TUI 的展示样式见 `tui-cli.md`。
