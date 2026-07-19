# Issue #872 实施计划

> 按 TDD 顺序执行；每一步只完成一个可验证交付。

## 目标

删除 Runtime 的 Session/ChatChain 兼容投影和旧 writer，令 Context Management 成为 Session、compact、resume 与持久化的唯一权威；旧 `messages/cwd` 仅存在于兼容 reader wire DTO。

## 范围边界

- #872：Session/Context 权威、旧 writer、双轨、边界 Guard。
- #879：`RuntimeResources`、`MainRunPort`、Run control 等更广泛 Runtime 退役，不在本计划扩张。
- #1055：父项级测试完整性审查在 #872 完成后单独执行。

## TDD 与实现步骤

### 1. 建立 RED 证据

1. 在 Context 契约测试中加入：legacy `messages/cwd` 可升级，但 canonical writer 不再输出它们。
2. 在 Context Application/Session 管理测试中加入：list/export/import/metadata/delete 经 gate-aware façade；message count 从 canonical chats 派生。
3. 在 ContextPort 测试中加入：manual compact 可明确绕过自动阈值并持久提交；reset/clear 使用 revision 与 durable writer。
4. 在 Runtime input gate 测试中加入：gate 只产出 pending `Message`，不操作 `ChatChain`，撤回/abort/reset 保持原语义。
5. 运行对应定向测试，确认因缺少新 API 或仍依赖旧路径失败。

### 2. Context-owned Session 管理 façade

1. 在 Context Published Language 定义 session list/export/import/metadata/delete DTO 与错误。
2. 在 `MainSessionWiring` 增加 gate-aware session query/command façade。
3. 查询使用 shared permit；resume/active-session mutation 使用 exclusive permit与 mutation gate。
4. adapter 负责目录扫描与 AtomicBlob 读取；canonical codec 负责兼容升级。
5. Runtime 只消费 façade 返回的 Published Language，不接触 `Session`/`CanonicalSession`/`ChatChain`。

### 3. Context-owned idle compact/reset

1. 扩展 ContextPort 的 Context-owned command 能力，支持 manual compact 与 clear/reset。
2. manual compact 使用 stable canonical backing，沿用 Context 的 writer、revision 与 compact outcome。
3. clear/reset 原子清空 canonical chats，并收集 Task/Workspace snapshot 后持久化。
4. 保持 hook/reflection UI orchestration 在 Runtime；Context 只拥有 compact 数据策略和提交。

### 4. Runtime 切线

1. 将 `PendingInputBuffer`/gate 改为返回已采用的 `Vec<Message>`，不再接收 `ChatChain`。
2. 每个新 Run 只把本轮用户输入作为 `ContextRequest.pending_messages`；历史由 Context backing 提供。
3. `MainRunPort.messages` 只保存当前 Run/Step 投影；finalized append 继续增量提交。
4. `/resume` 统一调用 `resume_session_to_backing`，事件消息从 canonical restore 只读投影产生。
5. `/compact` 调 ContextPort manual compact；`/clear` 调 Context-owned reset。
6. `process_chat_loop` 返回 `()`，不再返回 `ChatChain`。

### 5. 删除兼容投影与旧 writer

1. 删除 `RuntimeProjectionParticipant` 与 `SessionProjectionParticipant`。
2. 删除 `RuntimeHandle.current_chain/frozen_chats/active_summary`。
3. 删除 `ChatLoopContext.chain/frozen_chats/active_summary/save_chain`。
4. 删除 `save_chain_to_handle`、`save_session_from_handle` 和 loop-exit auto-save。
5. 删除旧 `compact_outcome` 对 Runtime chain 的变更路径。

### 6. 删除 Session 双轨

1. 从 live `Session` 删除 `messages` 与 `cwd`。
2. 删除 `migrate_legacy_messages` 和 legacy `save_session` writer。
3. legacy `messages/cwd` 只保留在 `domain/session/envelope.rs::LegacySession`。
4. list/search/metadata/import/export 统一通过 canonical codec 与 AtomicBlob store。
5. 删除不再被消费的 crate-root re-export 与死代码。

### 7. Guard 收口

1. 扩大 Runtime→Context 边界扫描到全部 Runtime 生产源码。
2. 禁止 `context::session::*`、`ChatChain`、`ChatSegment`、`SessionRestore`、`save_chain`、projection participant 回流。
3. 测试文件 exclusion 继续使用已登记 `scope.runtime.shared-loop-tests`，不新增 migration exception。
4. 添加故意违规测试：Runtime 生产文件引用 `ChatChain` 或 `save_chain` 时单 Guard 与总编排均 exit 2。
5. 白名单预算保持 migration debt `6 → 6`；Runtime migration exception `5 → 5`，本 Issue 不机械修改 #879 所有的五项。

### 8. 文档同步

同步：

- `specs/runtime.md`
- `specs/storage.md`
- `docs/design/02-modules/context-management/README.md`
- `docs/design/02-modules/context-management/01-session.md`
- `docs/design/03-engineering/03-migration-governance.md`
- `docs/design/03-engineering/01-architecture-guards.md`

### 9. 验证

依次运行：

1. Context/Runtime 定向测试。
2. `cargo fmt --check`。
3. `cargo test -p context`。
4. `cargo test -p runtime`。
5. `cargo test --workspace`。
6. `cargo run -p xtask -- production-reachability .`。
7. `cargo clippy --workspace --all-targets -- -D warnings`。
8. 相关单 Guard、Guard 故意违规测试与总编排。
9. `./scripts/coverage.sh` 并记录 Context/Runtime 信号。

任何首次失败都保留为真实结果并修复，不用重跑成功覆盖。
