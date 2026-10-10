# #1913 StuckGuard：废除 HardPause，升级直接 Failed + step 粒度 fuse

> 分支：`bug-1913-stuckguard-fail-no-hardpause`
> Issue：https://github.com/rushsinging/aemeath/issues/1913
> 工作目录：`~/.agents/worktrees/aemeath/bug-1913-stuckguard-fail-no-hardpause`

## Goal

1. StuckGuard 升级路径不再挂 `AwaitingUser` / HardPause interaction；Main/Sub 一律 `Failed`，用户发下一条消息继续。
2. `ToolCallFuse` 改为 **step 粒度**：同一 `step_id` 内同一指纹只计 1 次；阈值沿用 Soft=3 / Hard=5 / `blocked_count≥3`。
3. 删除 HardPause 交互协议与设计文档描述。

## 拍板

- HardPause interaction 协议废除（非补 Continue/Cancel UI）。
- 计数选项 **A**：按 step 命中计，不看成败。
- 阈值沿用现常量。

## 文件地图

| 区域 | 路径 |
|---|---|
| Fuse | `agent/features/runtime/src/application/tool/coordination/loop_guard.rs` + `loop_guard_tests.rs` + `constants.rs` |
| Guard | `agent/features/runtime/src/application/loop_engine/stuck_guard.rs` |
| Engine | `.../engine/step_driver.rs`、`interaction_driver.rs`、`control_driver.rs`（`fail_run`） |
| Domain | `.../domain/agent_run/{domain,state,tests}.rs`（`ContinueAfterHardPause`） |
| SDK | `packages/sdk/src/interaction.rs` + contract/client tests |
| TUI | `apps/cli/src/tui/**` HardPause body/mapping |
| Docs | `docs/design/02-modules/runtime/04-stuck-prevention.md` 等 |
| Scenarios | `engine_scenarios_tests.rs` HardPause 用例改 Failed |

## Tasks

### Task 1：TDD — step 粒度 fuse（红）

改写 `loop_guard_tests.rs`：

1. 同 step 连续 3 次相同指纹 → **Allow**（只计 1）。
2. 跨 3 个不同 step 相同指纹 → **SoftBlock**。
3. 跨 5 个 step 或 SoftBlock 累计 `blocked_count≥3` → **Fail**（原 HardPause 变体改名）。
4. 周期循环按 step 条目检测。
5. JSON key 规范化仍成立（跨 step）。

`inspect` 签名改为 `inspect(step_id, &ToolCall)`。

### Task 2：实现 step 粒度 fuse（绿）

- `recent` 存 `(step_id, fingerprint)`；同 step 同指纹不重复 push。
- `consecutive_count` 按 recent 尾部相同 fingerprint 的条目数（每条目已是一步）。
- `ToolFuseDecision::HardPause` → `Fail`；常量名 `TOOL_FUSE_HARD_PAUSE_LIMIT` → `TOOL_FUSE_FAIL_LIMIT`（值仍 3）。

### Task 3：StuckGuard / step_driver 升级改 Failed

- `StuckDecision::HardPause` → `Fail { reason }`。
- `inspect_tool(&step_id, call)` 透传 step。
- text stall 升级同样 `Fail`（与工具一致，废除挂起）。
- 所有原 `handle_hard_pause` 调用点改为 `record_stuck` + `fail_run`（工具轮：可先 SoftBlock 本批升级 call，轮末 `fail_run`；或检测到 Fail 后收口工具轮再 fail——保持「先喂回 blocked 再 Failed」需在场景测试钉死；**推荐**：升级即 `fail_run`，本 step 内已 SoftBlock 的调用仍写入 fuse 错误结果后 fail，避免再 begin interaction）。
- 删除 `handle_hard_pause` / `close_out_hard_pause_resume` / `HardPauseBegin` / `HardPauseStepClose`。

### Task 4：删除 HardPause 协议面

- Domain：`InteractionContinuation::ContinueAfterHardPause`。
- SDK：`InteractionRequestBody::HardPause`、`InteractionReply::HardPauseContinue`。
- Runtime interaction port/coordinator/tests、ingress tests。
- TUI：`UiInteractionBody::HardPause`、event mapping、executor tests。
- 场景测试：`tool_fuse_hard_pause_*` / `repeated_text_hard_pause_*` / degrade/unavailable 用例 → 改为「升级 → Failed，无 interaction；用户下一输入可开新 Run」；Sub unavailable 特例随协议删除而消失（Main/Sub 同 Failed）。

### Task 5：文档

- `04-stuck-prevention.md`：分级改为 SoftBlock → Failed；删 HardPause 行；写明 step 粒度。
- `01-domain-model.md` / `03-loop-and-state-machine.md` / `06-ports-and-adapters.md` / TUI 相关：删 ContinueAfterHardPause / HardPauseContinue / HardPause 交互描述。
- Issue #1913 body 同步新口径。

### Task 6：验证

```bash
cargo test -p runtime --lib
cargo test -p sdk --test interaction_contract
cargo test -p cli --lib
```

门禁：fuse 同 step 不误杀；跨 step 升级 Failed；无 HardPause interaction；Failed 后可再输入。

## 风险

- 删除 SDK variant 是 wire breaking：旧 session 若序列化了 HardPause body，反序列化需 fail-open 或拒绝——查既有 serde 兼容策略。
- text stall 与 tool fuse 一并改 Failed，行为面比原「只修 cancel」更大，但与用户拍板一致。
- 场景脚本 `five_step_hard_pause_drain_script` 阈值在 step 粒度下仍约 5 step 触发 Fail（Hard=5）。
