# #884 Tool Result Blob 根因迁移实施计划

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/884

## 决策冻结

1. Storage 继续只发布 `AtomicBlobPort` / `AtomicDatasetPort`，不新增 AppendLog OHS。
2. History 不复活；`#991` 删除结果视为最终 disposition。
3. Tool Result materialization 归 Runtime：Runtime 决定阈值、preview、inline replacement，并拥有窄出站 `ToolResultBlobPort`。
4. Runtime adapter 只把 `(session_id, tool_use_id, bytes)` 翻译为 `StorageKey(StorageNamespace::ToolResult, ...)`，内部调用 `AtomicBlobPort`；Storage 不解释 Tool Result schema。
5. Main/Sub 统一调用一个 async `ToolResultMaterializer`，不再各自复制 tuple 转换和同步文件 I/O。
6. 配置放在 `tools.tool_result`：默认保持当前行为 `threshold_chars=50000`、`preview_head_chars=2000`、`preview_tail_chars=500`；配置字段按 Unicode 字符计数，避免当前 bytes/chars 混称。
7. 写失败返回 typed materialization error；调用方保留完整 inline result 并记录诊断，绝不生成不存在的持久化引用。
8. 已有 `~/.agents/tool-results/{session}/{tool}.txt` 文件不迁移、不删除，旧 session 中的绝对引用继续可读。新写入使用 AtomicBlob adapter 的受约束布局；引用只来自 adapter 返回的 opaque locator，不由策略层拼物理路径。
9. 不改变 AtomicBlob 的全局物理协议来迎合单一 namespace；否则会扩大 #884 到全部 namespace 的 crash protocol 迁移。若产品要求新写入仍必须落在 legacy 精确路径，应另立 Storage layout migration issue，设计 data/protocol 分离及跨布局恢复。

## TDD 与实施步骤

### Task 1：Config RED

文件：
- `agent/shared/src/config/domain/tools.rs`
- `agent/shared/src/config/domain/merge.rs`
- `agent/shared/src/config/domain/snapshot.rs`

步骤：
1. 先写失败测试：旧配置缺字段取兼容默认；显式 snake_case 生效；partial patch 不清空其他字段；零值被拒绝或归一；head + tail 不得超过 threshold。
2. 新增 `ToolResultConfig`、patch 和 `ConfigSnapshot` 只读 accessor。
3. 运行 `cargo test -p share config` 的定向测试。

### Task 2：Runtime PL/port RED

文件：
- `agent/features/runtime/src/ports/tool_result_blob.rs`
- `agent/features/runtime/src/ports.rs`
- `agent/features/runtime/src/application/tool_result_materialization.rs`

步骤：
1. 先写纯策略失败测试：阈值边界、Unicode 字符、head/tail、大小展示、opaque locator、写失败保留 inline、非法 session/tool id typed failure。
2. 定义 Runtime-owned `ToolResultBlobPort`、`ToolResultBlobRef`、`ToolResultBlobError`。
3. 实现 `ToolResultMaterializer`，输入 typed `ToolExecution`，输出 provider message tuples；策略不依赖 Storage。
4. 运行 Runtime 定向测试。

### Task 3：Storage-backed adapter contract RED

文件：
- `agent/features/runtime/src/adapters/tool_result_blob.rs`
- `agent/features/runtime/src/adapters.rs`
- `agent/features/runtime/tests/tool_result_blob_contract.rs`

步骤：
1. 用 fake `AtomicBlobPort` 先写失败 contract：key namespace/segments、ProcessCrashSafe、write-once 幂等、非法 segment fail-closed、Storage error typed 映射、locator 不泄漏 adapter protocol artifact。
2. 实现 Runtime adapter，依赖 `Arc<dyn AtomicBlobPort>`。
3. 对既有 legacy `.txt` 引用增加只读兼容测试：迁移不移动或删除旧文件；旧 session 引用保持有效。

### Task 4：Main/Sub 生产接线 RED→GREEN

文件：
- `agent/features/runtime/src/application/resources.rs`
- `agent/features/runtime/src/application/main_loop/looping/loop_context.rs`
- `agent/features/runtime/src/application/main_loop/looping/tools.rs`
- `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- `agent/features/runtime/src/application/subagent/runner/loop_helpers.rs`
- `agent/features/runtime/src/application/subagent/runner/loop_run.rs`
- `agent/features/runtime/src/application/subagent/runner/setup.rs`
- `agent/features/runtime/src/application/client/from_args.rs`
- 受构造字段影响的现有测试夹具

步骤：
1. 将 Main/Sub 现有 oversize 测试先改为注入同一 fake materializer，并证明两条路径得到相同结果。
2. 把同步 helper 改成 async 单一 materialization 入口。
3. 在 bootstrap 构造共享 `FileSystemBlobAdapter` 与 Runtime adapter，从 `ConfigSnapshot` 注入不可变 policy。
4. 所有写失败分支保留完整 inline output，并记录单条 target 合规日志。
5. 运行 Main/Sub 定向测试。

### Task 5：退役 Storage 业务路径

文件：
- 删除 `agent/features/storage/src/tool_result.rs`
- 更新 `agent/features/storage/src/lib.rs`
- 更新 `agent/features/runtime/Cargo.toml`（若依赖形态变化需要）
- 更新 `.agents/hooks/check-crate-api-boundary.sh`

步骤：
1. 先新增 guard/静态失败证据：禁止 `storage::persist_oversized_results`、`storage::MAX_TOOL_RESULT_CHARS` 与 `storage/src/tool_result.rs` 复活。
2. 删除旧模块、re-export、硬编码阈值和 Runtime 直接调用。
3. Grep 证明旧符号与 Storage 直接 Tool Result 策略为零。

### Task 6：文档与 disposition

文件：
- `docs/design/02-modules/storage/README.md`
- `docs/design/01-system/03-context-map.md`
- `docs/design/03-engineering/03-migration-governance.md`
- `docs/design/03-engineering/04-testing-and-coverage.md`
- `docs/design/03-engineering/01-architecture-guards.md`

步骤：
1. 删除“#884 在 Storage 重新落地 History/AppendLog”的过期 Current 承诺。
2. 记录 Runtime-owned Tool Result materialization + blob adapter 边。
3. 记录 AppendLog 归 Audit、History 不复活及 legacy `.txt` 只读兼容边界。
4. 在 issue 差异表逐项标记已对齐/已修正文档/延期。

### Task 7：最终验证与审查

1. `cargo fmt --all -- --check`
2. `cargo test -p share --all-targets`
3. `cargo test -p storage --all-targets`
4. `cargo test -p runtime --all-targets`
5. `cargo test --workspace --all-targets`
6. `cargo clippy --workspace --all-targets -- -D warnings`
7. 全部 architecture hooks 与 `git diff --check`
8. 独立 reviewer 检查策略所有权、错误语义、Main/Sub 一致性、旧路径兼容和 façade 收紧。
9. 更新 issue 完成证据，提交、push，创建 base 为 `release/v0.1.0` 的 PR；不自行合并或关闭 issue。

## 非目标

- Audit Usage worker/File AppendLog/query/rotation/retention。
- 通用 framed append-log OHS。
- Tool Result retention、孤儿扫描和 Session 删除级联策略。
- 重写 AtomicBlob 全 namespace 物理布局。
