# Stop Hook 反馈修复实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 Stop hook 将成功日志误判为阻断的问题，并以结构化、可持久化的双表示反馈在 TUI、resume 与 LLM continuation 中保持一致。

**Architecture:** Hook 域必须区分“结构化 JSON 协议输出”与“普通成功日志”；只有明确 JSON 才解析 directive。Stop hook 阻断由 Runtime 构造成包含实际阻断 subscription、command、exit code、摘要和输出引用的结构化 payload：持久化/SDK/TUI 使用 JSON 表示，LLM 使用受控文本表示。TUI 实时与 resume 均从同一 payload 投影 Hook notice。

**Tech Stack:** Rust、serde、Hook Dispatcher、Runtime Context/Session、SDK wire schema、Ratatui TUI、cargo test/clippy、xtask schema guard。

---

## 当前根因与边界

1. `Hook` 协议的 `classify_output()` 将 `exit=0 + 任意非空 stdout` 强制解析为 JSON。`check-agent-stop.sh` 成功时会输出 cargo/guard 日志，解析失败后成为 `ExecutionFailed`；Stop 点重试耗尽后固定合成为 `Block`。因此成功守卫被错误显示为 Stop 阻断。
2. 当前 Runtime 仅从 `RuntimeHookDispatch.executions.last()` 取 stdout/stderr，丢失真正阻断 subscription 的 command、exit code 和 directive；后续成功或 StopFailure execution 会覆盖显示来源。
3. 当前 Stop feedback 仅是 `Message::stop_hook_feedback("<system-reminder>..." )`。TUI 只能从拼接文本生成 notice，无法区分 body/details，也无法在 resume 中保留实际 command/结构化输出。
4. `MessageSource::StopHook` 已在 #1336 分支中加入，但 payload 仍仅有文本；本计划将其扩展为稳定的结构化 metadata，旧 session 仍按文本模板兼容。
5. 不改变宿主生命周期 Stop hook 的语义；只修复 Aemeath 自身 hook dispatcher、session payload 与显示投影。

## 数据模型与语义

### 1. Hook 分类

`classify_output()` 的成功出口规则：

- `exit_code != 0`：保持 `HookDirective::Block`。
- `exit_code == 0 && stdout.trim().is_empty()`：保持 `Continue`。
- `exit_code == 0 && stdout.trim_start()` **不以 `{` 开头**：视为普通成功日志，返回 `Continue`，不尝试 JSON 解析。
- `exit_code == 0 && stdout.trim_start()` 以 `{` 开头：解析 JSON；非法 JSON 仍为 `InvalidJson`，保留协议错误与 Stop 阻断语义。

这让脚本可安全输出构建/守卫日志，仍可使用 JSON 写明确的 `continue:false` 或 `decision:block`。

### 2. 实际阻断执行

Hook domain 在 `HookOutcome` 或等价 runtime projection 中暴露 `StopHookBlockDetail`：

- `command: String`：发生 block 的实际 subscription command（完成变量展开后的执行命令；若敏感字段风险存在，使用配置原命令并遵循现有日志脱敏规则）。
- `exit_code: Option<i32>`。
- `reason: HookReason / RuntimeHookReason`。
- `stdout: String`、`stderr: String`：原始、已受 Hook 协议输出上限保护的内容。
- `attempt`、`execution_ordinal`。

Dispatcher 在发现 `HookDirective::Block` 时，记录当前 subscription 的 command 与其最终 execution，不能从 `all_executions.last()` 推断。StopFailure 的 observation execution 不得覆盖该 detail。

### 3. Stop feedback payload

新增 share/SDK 对齐、带 `#[serde(default)]` 的 `StopHookFeedback` metadata：

- `summary`：简短固定摘要，例如 `Stop hook blocked completion.`。
- `command`、`exit_code`、`reason`。
- `stdout_preview`、`stderr_preview`：分别按 TUI 上限截断；各自有 `stdout_truncated` / `stderr_truncated`。
- `output_file: Option<String>`：完整输出超出 LLM 上限时的 session-scoped 安全文件路径。
- `llm_text` 不持久化为重复字段；由 payload 在 Runtime 生成。

`MessageMetadata` 保持 `source: StopHook`，新增可选 `stop_hook: Option<StopHookFeedback>`；旧消息 `stop_hook=None` 完全兼容。

### 4. 双表示规则

- **TUI/Resume JSON:** 使用 payload 的 summary、command、exit code、reason、preview、truncated 状态和 file path 生成 notice。
- **LLM:** 使用 `<system-reminder>` 包装的摘要。stdout/stderr 任一超出 LLM 上限时，将完整拼接内容写入 `~/.agents/...` 的 session-scoped hook output 文件，只发送安全路径与“使用 Read 查看完整输出”的提示；不得把大日志继续注入 prompt。
- TUI body 使用 `TEXT_MUTED`（普通灰色）；header 保留 blocked 的 error marker 色；details 保持 `TEXT_DIM`。

## 文件结构

| 文件 | 职责 |
|---|---|
| `agent/features/hook/src/domain/protocol.rs` | 区分 JSON directive 与普通成功 stdout。 |
| `agent/features/hook/src/domain/outcome.rs` | 为阻断 subscription 定义/承载 command 与实际 execution detail。 |
| `agent/features/hook/src/adapters/dispatcher.rs` | 发生 Block 时保存当前 subscription 的实际 command 与 execution，而不是取最后一条全局 execution。 |
| `agent/features/hook/src/adapters/dispatcher/tests.rs` | Hook 分类、成功日志、JSON 损坏与多 subscription 阻断者的场景。 |
| `agent/shared/src/message/{types.rs,constructors.rs}` | 扩展 `MessageMetadata` 的 Stop hook payload。 |
| `packages/sdk/src/session.rs` | 发布对等 Stop hook feedback DTO，保留 serde 兼容。 |
| `packages/sdk/schema/wire-components.schema.json` | xtask 生成的 schema 更新。 |
| `agent/features/runtime/src/application/hook_adapter.rs` | 将 hook block detail 投影为 Runtime payload。 |
| `agent/features/runtime/src/application/chat/looping/finalize.rs` | 构造双表示、分别截断 stdout/stderr、必要时写文件指针。 |
| `agent/features/runtime/src/application/chat/looping/main_run_port.rs` | 持久化带 payload 的 Stop feedback，并继续将 LLM text 放入本轮 continuation。 |
| `agent/features/runtime/src/application/chat/looping/{finalize_tests.rs,loop_runner_tests.rs}` | 验证 actual blocker、文件指针、continuation 和正常放行。 |
| `agent/features/runtime/src/application/client/mapping.rs` | share → SDK 的 Stop hook payload 映射。 |
| `apps/cli/src/tui/adapter/agent_event.rs` | 实时 `StopHookBlocked` 从结构化 payload 生成 notice。 |
| `apps/cli/src/tui/model/conversation/{history_parse.rs,intent_impls.rs}` | resume 从 payload 重建同构 notice；旧文本 fallback 保留。 |
| `apps/cli/src/tui/render/output/blocks/diagnostic.rs` | Hook notice body 改用普通灰色。 |
| `apps/cli/src/tui/**/tests.rs` | CLI adapter、resume timeline、渲染颜色场景覆盖。 |

## 实施任务

### Task 1: Hook 协议接受普通成功日志

**Files:**
- Modify: `agent/features/hook/src/domain/protocol.rs`
- Modify: `agent/features/hook/src/domain/tests.rs`

- [ ] **Step 1: 写失败测试：成功的普通 stdout 不触发 JSON 协议错误**

新增测试：`HookPoint::Stop + exit=0 + stdout="cargo guard passed\n"` 返回 `HookDirective::Continue`。

- [ ] **Step 2: 运行定向测试确认失败**

Run: `cargo test -p hook classify_output_stop_plain_success_log_continues`

Expected: FAIL，当前实现返回 `InvalidJson`。

- [ ] **Step 3: 写失败测试：JSON-like 损坏输出仍是协议错误**

新增测试：`exit=0 + stdout="{not-json"` 返回 `ClassifyError::InvalidJson`。

- [ ] **Step 4: 实现 JSON 候选门槛**

仅当 `stdout.trim_start().starts_with('{')` 时调用 `serde_json::from_str`；普通文本直接 `Continue`。非零退出和空 stdout 的现有语义不变。

- [ ] **Step 5: 运行 Hook domain 测试**

Run: `cargo test -p hook domain::tests`

Expected: PASS。

### Task 2: 保留实际阻断 subscription 与 execution

**Files:**
- Modify: `agent/features/hook/src/domain/outcome.rs`
- Modify: `agent/features/hook/src/adapters/dispatcher.rs`
- Modify: `agent/features/hook/src/adapters/dispatcher/tests.rs`

- [ ] **Step 1: 写失败测试：多 Stop subscription 使用实际阻断 command**

构造：第一个 Stop subscription 非零退出，第二个成功并输出普通日志。断言 outcome 的 block detail 指向第一个 command、exit code 与 stderr；第二个 subscription 不得执行（Block 短路）。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p hook stop_block_detail_keeps_blocking_subscription`

Expected: FAIL，当前 `HookOutcome` 无 detail。

- [ ] **Step 3: 在 HookOutcome 引入 block detail**

新增 typed `HookBlockDetail`，保存 command、执行 ordinal、attempt、实际 `HookExecution` 与 reason；`HookOutcome` 新增 optional field 并用 `#[derive(Debug, Clone)]` 对齐。

- [ ] **Step 4: Dispatcher 在 block 短路时填充 detail**

从当前 `sub.command` 和当前 subscription 最终 execution 构造 detail；StopFailure 追加 execution 时不修改 detail。

- [ ] **Step 5: 写并运行正常通过场景**

Run: `cargo test -p hook stop_block_detail_keeps_blocking_subscription`

Expected: PASS。

### Task 3: 定义可持久化 Stop hook payload 与 SDK 映射

**Files:**
- Modify: `agent/shared/src/message/types.rs`
- Modify: `agent/shared/src/message/constructors.rs`
- Modify: `agent/shared/src/message/tests.rs`
- Modify: `packages/sdk/src/session.rs`
- Modify: `agent/features/runtime/src/application/hook_adapter.rs`
- Modify: `agent/features/runtime/src/application/client/mapping.rs`

- [ ] **Step 1: 写 share 失败测试：payload round-trip 且旧 metadata 可读**

断言带 `source=StopHook` 和完整 payload 的 Message JSON round-trip；仅有 `source=system_generated` 的旧 metadata 反序列化后 `stop_hook=None`。

- [ ] **Step 2: 写 SDK 失败测试：payload schema 映射完整**

断言 command、exit code、reason、两个 preview、truncated 标记与 `output_file` 都在 SDK DTO 中。

- [ ] **Step 3: 定义共享 payload 与 SDK DTO**

字段必须 `#[serde(default)]`；新增字段不得破坏旧 session 或旧 SDK wire。

- [ ] **Step 4: 写 Runtime mapping 测试并实现 share → SDK 映射**

输入带 payload 的 share Message，断言 SDK ChatMessage 中 source 和 payload 完整保留。

- [ ] **Step 5: 更新 wire schema**

Run: `cargo run -p xtask -- sdk-wire-schema write packages/sdk/schema/wire-components.schema.json`

- [ ] **Step 6: 验证 schema**

Run: `cargo run -p xtask -- sdk-wire-schema check`

Expected: PASS。

### Task 4: Runtime 构造 LLM text 与 TUI payload

**Files:**
- Modify: `agent/features/runtime/src/application/chat/looping/finalize.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/finalize_tests.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/loop_runner_tests.rs`

- [ ] **Step 1: 写失败测试：长 stdout/stderr 分别截断**

构造实际 block detail，两个输出均超过 TUI preview limit。断言 payload 的 stdout/stderr preview 各自截断、各自标记 truncated；body 只含简短 summary。

- [ ] **Step 2: 写失败测试：LLM text 使用文件指针**

当合并原始输出超过 LLM limit 时，断言完整文本被写入 session-scoped 临时文件，LLM text 含文件路径与 Read 指令，不含完整 stdout/stderr。

- [ ] **Step 3: 写失败测试：普通短输出保留在 LLM text**

断言短 stderr/block reason 仍直接进入 `<system-reminder>`，不创建文件。

- [ ] **Step 4: 实现 feedback builder**

将现有 `stop_hook_feedback() -> String` 替换为返回 `{ payload, llm_text }` 的 typed value：
- 仅使用 `RuntimeHookDispatch.block_detail`；
- 使用实际 command；
- stdout/stderr 分别截断；
- 超限时原子写入 session-scoped hook output 路径；
- 生成简短 LLM 指令文本。

- [ ] **Step 5: MainRunPort 持久化 payload，continuation 使用 llm_text**

`Message::stop_hook_feedback` 接收 typed payload 与 LLM text；仍生成 `<system-reminder>` 包装，仍进入 `stop_hook_feedback → pending_stop_hook_feedback → freeze_step`，并继续排除 accepted user input。

- [ ] **Step 6: 验证 runtime 定向测试**

Run: `cargo test -p runtime --lib stop_hook`

Expected: PASS。

### Task 5: TUI 实时和 resume 同构 notice

**Files:**
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event/tests.rs`
- Modify: `apps/cli/src/tui/model/conversation/history_parse.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent_impls.rs`
- Modify: `apps/cli/src/tui/render/output/blocks/diagnostic.rs`
- Modify: corresponding CLI tests

- [ ] **Step 1: 写失败测试：实时 StopHookBlocked 使用 payload details**

输入带 payload 的 `ChatMessageSource::StopHook`：断言生成一个 `HookNotice`，header 为 `Hook blocked: Stop`，body 为 summary，details 包含实际 command、exit code、reason、截断标记和文件路径。

- [ ] **Step 2: 写失败测试：resume 同样 payload 生成相同 notice**

恢复 `User → StopHook → Assistant` 混合历史；断言 notice 内容和实时路径相同，StopHook 不生成 UserMessage。

- [ ] **Step 3: 写失败测试：Hook notice body 使用普通灰色**

渲染 blocked HookNotice，断言 title 仍使用 error 语义色、body 使用 `theme::TEXT_MUTED`、details 使用 `theme::TEXT_DIM`。

- [ ] **Step 4: 实现 shared TUI projection helper**

在 TUI conversation/adapter 边界定义单一 helper：`StopHookFeedback payload → HookNoticeContent`。实时 `StopHookBlocked` 和 `ResumeConversation` 必须共用它，避免两条路径漂移。

- [ ] **Step 5: 保留 legacy fallback**

payload 缺失时：只对旧 `SystemGenerated` + 精确的历史 Stop 模板构建 legacy notice；details 标记“历史记录未保存结构化执行信息”，不猜测 command。

- [ ] **Step 6: 调整渲染色**

`render_hook_notice` 对 blocked notice 的 body 改为 `TEXT_MUTED`；其它 notice 维持当前 body 风格，避免无关 UI 改动。

- [ ] **Step 7: 验证 CLI 定向测试**

Run: `cargo test -p cli stop_hook`

Expected: PASS。

### Task 6: L4 场景测试与回归验证

**Files:**
- Modify/Create: `apps/cli/src/tui/app/scenario_tests/stop_hook_feedback.rs`（按既有 scenario module 注册方式接入）
- Modify: scenario test module index（若现有结构要求）

- [ ] **Step 1: 写 L4 场景：失败 → 成功日志 → 正常结束**

构造真实 dispatcher/scripted executor 与 TUI event chain：
1. 第一次 Stop hook 非零退出，生成一个 payload notice；
2. LLM continuation 后再次 Stop，hook exit 0 且输出普通 guard 日志；
3. 最终必须 `Done`，不得产生第二个 `StopHookBlocked`；
4. timeline 中第一条 notice 指向实际失败 command，且没有伪 UserMessage。

- [ ] **Step 2: 写 L4 场景：持久化后 resume**

将第一条 Stop hook payload 写入 canonical session，resume 后断言 TUI timeline 中恢复同构 HookNotice，显示 command、摘要、截断/file pointer 信息；没有 UserMessage。

- [ ] **Step 3: 运行 L4 定向场景**

Run: `cargo test -p cli stop_hook_feedback_scenario`

Expected: PASS。

### Task 7: 全量验证、审查与 PR 更新

**Files:**
- Modify: PR #1336 body（GitHub）

- [ ] **Step 1: 格式化与定向测试**

Run:
```bash
cargo fmt
cargo test -p hook
cargo test -p share
cargo test -p sdk
cargo test -p context
cargo test -p runtime --lib
cargo test -p cli --bin aemeath
```

Expected: PASS。

- [ ] **Step 2: 编译与 lint**

Run:
```bash
cargo clippy -p hook -p share -p sdk -p context -p runtime -p cli --all-targets -- -D warnings
bash .agents/hooks/check-architecture-guards.sh --full
cargo run -p xtask -- sdk-wire-schema check
git diff --check
```

Expected: all PASS。

- [ ] **Step 3: 代码审查**

审查 payload 是否只从实际 block detail 构建、普通 stdout 是否永不阻断、LLM 长输出是否仅留文件指针、实时与 resume 是否复用同一 notice helper。

- [ ] **Step 4: 提交和同步主分支**

Run:
```bash
git add <intended-paths>
git commit -m "fix(hook): #1335 收敛 Stop hook 反馈语义"
git pull origin main
# 重跑 Task 7 Step 1-2 的验证
git push
```

- [ ] **Step 5: 更新 PR #1336**

PR body 保持 `Closes #1335`，补充：
- 普通成功 stdout 不再误判为 JSON 协议失败；
- TUI JSON / LLM text 双表示与文件指针；
- L4 Stop hook 场景测试；
- 结构化 command/exit/output resume 回显。

## 验收清单

- [ ] `check-agent-stop.sh` exit 0 且输出正常 guard 日志时，Stop hook 通过。
- [ ] JSON-like 无效 stdout 仍是协议失败，不能静默吞掉。
- [ ] Stop hook block notice 指向实际阻断 command 与 execution，而非最后成功 execution。
- [ ] TUI body 是普通灰色；stdout/stderr 分别截断；details 含 command、exit code、reason 和文件指针。
- [ ] LLM 仅收到摘要与必要的文件指针；长输出不进入 prompt。
- [ ] resume 从新 payload 还原与实时一致的 notice；旧 session 保持可读。
- [ ] 一个完整场景验证“阻断一次、后续成功、最终结束”。
- [ ] PR #1336 仍只关联并关闭 #1335。
