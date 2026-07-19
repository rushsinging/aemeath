# 持久化（Storage）

**Scope**：`agent/features/storage/**`——memory、task、tool_result 的当前持久化实现及后续通用机制。
**主触发**：改 `agent/features/storage/**`。
**次触发**：改会话 / 记忆 / 任务 / 历史的落盘格式或路径。

## Target 机制结构

- Storage 采用 `domain + ports + adapters` Hexagonal + Clean：`domain/` 只含 Published Language 与机械策略，`ports/` 只含 Storage-owned OHS，`adapters/` 终止文件系统 detail。
- `domain/` **NEVER** 使用 `std::fs` / `tokio::fs`、持有物理 `PathBuf` 或依赖 `adapters`；跨 BC 只经 crate-root 窄 façade。
- `memory_store/`、`task_store/` 与 `tool_result.rs` 是 #991 过渡实现，分别由 #883/#884 迁出或退役；**NEVER** 作为新增 Storage 机制的放置位置。
- 选择 Hexagonal 的工程依据是易由静态 Guard 证明层间方向、I/O 归属和公开面，防止 adapter 细节向内漂移及长期结构劣化。

## 子域与落盘位置

- **Memory**：`agent/features/storage/src/memory_store/`（`store.rs`、`path.rs`）→ `~/.agents/memory/`。
- **Task**：`agent/features/storage/src/task_store/`（`store.rs`、`list.rs`、`batch.rs`、`types.rs`、`display.rs`）——任务追踪持久化。
- **History**：旧 `storage/src/business/history.rs` 无生产消费者，已在 #991 作为仅测试可达死代码删除；后续 History 持久化能力由 #884 按目标端口重新落地。
- **Tool result**：`agent/features/storage/src/tool_result.rs`——大体积 tool 结果落盘（默认上限见 MCP/工具配置）。
- 会话持久化目录：`~/.agents/sessions/`。

改落盘格式或路径时，**MUST** 兼顾已有数据的可读性，避免破坏现有 `~/.agents/` 下的用户数据。

## Session 落盘策略（#869 / #872）

Session schema 与编排归 Context Management；Storage 只发布 AtomicBlob 机制：

- **MUST** finalized RunStep 通过 `ContextPort::append_and_persist` 提交，Context 收集 Task / Workspace snapshot 后编码 canonical envelope。
- **MUST** manual/automatic compact 与 reset 使用同一 canonical backing、mutation gate 和 AtomicBlob writer。
- **MUST** 启动 resume 与运行期 `/resume` 通过 Context 的兼容 reader 读取 legacy wire，再由联合协调器发布 committed Session。
- **NEVER** Runtime 持有可变 `ChatChain` 第二 backing、调用 `save_chain`，或在 loop 退出时重复落盘。
- **NEVER** 新 writer 输出 top-level `messages` / `cwd`；它们只允许存在于 Context 的 legacy reader DTO。
- **MUST** unknown future schema 原字节保留，禁止 fallback、quarantine 或覆写。
- Session 物理数据使用 `StorageNamespace::Session`，由 `AtomicBlobSessionStore` 映射 primary / previous / quarantine 协议。

## Session Lock 文件（#636 D3）

为防止两个 aemeath 实例同时操作同一 session_id 造成数据互相覆盖，启动时 acquire session lock：

- **Lock 路径**：`~/.agents/sessions/{session_id}.lock`，内容 JSON：`{ pid, created_at, hostname }`。
- **API**：`sdk::session_lock::acquire(id)` / `force_acquire(id)` / `release(&mut lock)`。CLI 端包装在 `apps/cli/src/session_lock.rs`。
- **冲突处理**：lock 存在且 pid 存活 → 返回 `LockError::HeldAlive`；TUI 模式下 stderr 提示「PID X 启动于 Y 占用，是否强制接管？[y/N]」，quiet 模式直接 exit(4)。用户确认后 `force_acquire` 覆盖。
- **释放**：进程正常退出时 `Drop` 自动删除 lock；被 `kill -9` 时残留 lock 由下次启动的 pid liveness 检测兜底。
- **pid liveness**：Unix `kill(pid, 0)`，ESRCH 表示进程不存在。
