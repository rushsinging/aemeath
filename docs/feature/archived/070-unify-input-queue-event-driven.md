# Feature #70：统一 input_queue 到事件驱动

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 登记日期 | 2026-05 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认完成 |

## 背景

用户输入旧有三条路径，其中 `input_queue`/`QueueDrainPort`/`DrainQueuedInput` 是死代码：
1. **非 processing Enter（普通文本）** → `enter.rs` 直接构造 `ChatMessage` + `spawn_processing`
2. **非 processing Enter（slash 命令）** → `enter.rs` `push_queue` + `pending_slash` 返回 run_loop 执行
3. **processing 期间 Enter** → `key.rs` `SendChatInputEvent` → `InputEventPort` → runtime `drain_sources`

`input_queue` 只在路径 2 中被写入，但 `DrainQueuedInput` 消费者在 processing 期间才执行，此时队列永远为空——slash 命令在非 processing 时通过 `pending_slash` 走另一条路径，根本不经过 drain。

## 改造目标

- 所有用户输入统一走 `InputEventPort`（事件驱动）
- 移除 `input_queue`/`QueueDrainPort`/`DrainQueuedInput` 整条链路
- slash 命令和对话框选模型改用 `ChatInputEvent::SlashCommand` 通过事件发送
- 顺带启动 TUI single-source phase 2：将 InputArea selection / history / text cursor / render 等镜像字段移除，由 `InputArea` 状态作为单一真相

## 主要实现

- `packages/sdk/`：删除 `QueueDrainPort`/`QueueFuture` 类型及 `ChatRequest.queue_drain` 字段
- `agent/features/runtime/business/chat/looping/`：删除 `queue.rs` 全模块、`input_gate` / `loop_runner` / `looping` / `chat` 中的相关泛型与 import
- `agent/features/runtime/core/client/`：删除 `RuntimeQueueDrainPort` 及测试
- `apps/cli/`：删除 `InputState.input_queue`/`push_queue`/`drain_queue`/`queued_count`/`queued_front`、`UiEvent::DrainQueuedInput`、`TuiQueueDrainPort` 及 processing.rs 中相关 drain 逻辑
- TUI single-source 第二阶段：相继移除 InputArea selection / history / text cursor / render 镜像字段

## 关联提交

- `35ac308 / 7c9e9a1` InputArea render 去镜像
- `7b55f5b / 7104da2` InputArea history 去镜像
- `81850fd / d51ee2b` InputArea selection 去镜像
- `954707d / eb70c3f` InputArea text cursor 去镜像
- `4c65f83 / 01a1234` design 文档校正
- 以及前期 `input_queue`/`QueueDrainPort` 删除相关提交

## 验证

- `cargo test -p cli`
- `cargo test -p runtime`
- 用户确认完成。
