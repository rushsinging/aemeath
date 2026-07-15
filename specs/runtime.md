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

## Run Step 控制与 Session 回放（#700）

- **MUST** 日常停止只使用 `CancelRunStep`：立即停止当前 Step scope，异步共用 StepFinalizer 收口，最长 10s，然后固定进入 `DrainingInput`；有输入继续下一 Step，无输入正常 Completed。
- **MUST** 真正退出使用 `TerminateRun`：取消 Run root scope，共用同一 deterministic Tool/Agent summary 与 StepFinalizer，最长 5s，随后 flush Session 并进入 Terminated。
- **NEVER** 在 CancelRunStep 或 TerminateRun 收尾时调用 LLM 生成摘要；摘要只能来自 typed Tool/Agent receipts、已确认 partial output、artifact 与固定模板。
- **MUST** CancelRunStep 与 TerminateRun 使用同一 summary schema、价值门禁、原 ToolCall 顺序和 partial Step 持久化语义。
- **MUST** Session committed content 是 resume/replay 的唯一数据源；TUI 临时 projection、future/token/waiter 与 InputBuffer 都不是回放源。
- **MAY** TerminateRun 丢弃尚未进入 Session 的 InputBuffer 内容；这些内容不持久化、不恢复。已经绑定 Step 并提交 Session 的输入不属于 InputBuffer，必须可回放。
- **MUST** 当前不实现 Force Terminate；deadline 到达后把未确认工作写为 `CancellationUnconfirmed`，继续持久化与 Session flush。
## slash 命令系统

- slash 命令的 SDK/TUI 入站路由属于 `application/client`；具体命令能力由对应 Feature 的 Published Language / Tool 提供。
- Runtime 内不再维护 `core/command` 固定层注册表；新增命令时按实际所有者更新 SDK、TUI 或对应 Feature，**NEVER** 恢复旧 `core/command/` 路径。
- 命令在 TUI 的展示样式见 `tui-cli.md`。
