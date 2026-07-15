# 持久化（Storage）

**Scope**：`agent/features/storage/**`——memory、task、history、tool_result 的持久化。
**主触发**：改 `agent/features/storage/**`。
**次触发**：改会话 / 记忆 / 任务 / 历史的落盘格式或路径。

## 子域与落盘位置

- **Memory**：`agent/features/storage/src/business/memory/`（`store.rs`、`path.rs`）→ `~/.agents/memory/`。
- **Task**：`agent/features/storage/src/business/task/`（`store.rs`、`list.rs`、`batch.rs`、`types.rs`、`display.rs`）——任务追踪持久化。
- **History**：`agent/features/storage/src/business/history.rs` → `~/.agents/history.json`（用户输入历史）。
- **Tool result**：`agent/features/storage/src/business/tool_result_storage.rs`——大体积 tool 结果落盘（默认上限见 MCP/工具配置）。
- 会话持久化目录：`~/.agents/sessions/`。

改落盘格式或路径时，**MUST** 兼顾已有数据的可读性，避免破坏现有 `~/.agents/` 下的用户数据。

## Session 落盘策略（#636 / #680）

会话编排位于 `agent/features/runtime/src/application/chat/`，会话领域与持久化能力由 Context/Storage Published Language 提供（注意：Runtime 不拥有独立 Session 状态机）。落盘策略如下：

- **turn-level save（核心）**：每轮 turn 完成进入 Idle 前，同步调用 `save_chain(&chain)`，保证已完成 turn 立即落盘。即使进程被 `kill -TERM` 或意外退出，最多丢失正在跑的那一轮。
- **loop-exit save（兜底）**：`process_chat_loop` 返回最终 chain → spawn task 写回 `current_chain` → `save_session_from_handle` 落盘。
- **SIGTERM/SIGHUP graceful shutdown**：TUI 主 loop（`apps/cli/src/tui/app/run_loop.rs`）与非交互模式（`apps/cli/src/chat/no_tui.rs`）注册了 `tokio::signal::unix` 监听 SIGTERM/SIGHUP，收到信号后设置 `should_exit`，让主 loop 走正常 cleanup 路径，触发最后一次 save。
- **失败日志**：`save_session()` 失败时记录 `error!` 日志，不再静默忽略。
- **MUST** 落盘忠实序列化 `ChatChain` 的 `active_segments()`（真实 segment 边界），**NEVER** 从扁平 messages 反构造单段。

## SessionLoadError 错误分类（#636 D2）

`business/session/storage.rs` 的 `load_session` 返回 `Result<Session, SessionLoadError>`，错误分类：

- `NotFound { id }` —— session 文件不存在。CLI/TUI 收到 `SessionResumeFailed { kind: NotFound }`，提示「session 不存在，用 `/sessions` 查看」。
- `Corrupt { id, parse_err, corrupt_path }` —— JSON 损坏且 `.bak` 回退失败；原文件已转存到 `{id}.json.corrupt` 供手工抢救。
- `Io { id, source }` —— 底层 IO 错误（权限、磁盘）。

错误通过 `ChatEvent::SessionResumeFailed` 回传前端，由 TUI/no_tui 分支展示。

## Session Lock 文件（#636 D3）

为防止两个 aemeath 实例同时操作同一 session_id 造成数据互相覆盖，启动时 acquire session lock：

- **Lock 路径**：`~/.agents/sessions/{session_id}.lock`，内容 JSON：`{ pid, created_at, hostname }`。
- **API**：`sdk::session_lock::acquire(id)` / `force_acquire(id)` / `release(&mut lock)`。CLI 端包装在 `apps/cli/src/session_lock.rs`。
- **冲突处理**：lock 存在且 pid 存活 → 返回 `LockError::HeldAlive`；TUI 模式下 stderr 提示「PID X 启动于 Y 占用，是否强制接管？[y/N]」，quiet 模式直接 exit(4)。用户确认后 `force_acquire` 覆盖。
- **释放**：进程正常退出时 `Drop` 自动删除 lock；被 `kill -9` 时残留 lock 由下次启动的 pid liveness 检测兜底。
- **pid liveness**：Unix `kill(pid, 0)`，ESRCH 表示进程不存在。
