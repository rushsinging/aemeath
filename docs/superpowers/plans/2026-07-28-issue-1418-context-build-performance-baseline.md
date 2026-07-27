# #1418 Context build 性能基线切片实施计划

> **执行要求：** 在独立 worktree 中按 TDD 实施。本切片只建立 Context window build 的可重复结构与耗时基线，不宣称完成 #1418 的内存分配、TUI 端到端 draw 或慢帧日志范围。

**目标：** 对 `ContextApplicationService::build_candidate` 建立 test-only 分阶段观测，稳定回答一次 Context window 构建读取了多少历史、处理了多少 ToolResult 数据、各阶段耗时多少，以及压缩决策采用 Provider 实际 usage 还是启发式估算。

**Issue：** [#1418](https://github.com/rushsinging/aemeath/issues/1418)，Parent [#1417](https://github.com/rushsinging/aemeath/issues/1417)。

## 范围

### 本切片包含

- 在 Context crate 内增加仅 `cfg(test)` 编译的 performance collector。
- 为 `build_candidate` 分段记录：
  - session snapshot；
  - prompt materialization；
  - memory materialization；
  - candidate assembly；
  - token estimation 与 compaction decision。
- 记录稳定结构指标：
  - backing revision；
  - snapshot、pending 与最终 message 数；
  - system block 数；
  - ToolResult block 数和结构化 content bytes；
  - heuristic token estimate；
  - provider-reported total tokens（若存在）；
  - compaction decision token 与 reason。
- 建立不依赖真实 Provider/Storage 的 100/500/1000 messages 和长 ToolResult ignored release workload，输出 P50/P95 与代表性结构指标。
- 重复 build 使用相同 fixture，断言输入未变化时结构计数与 token 结果稳定且无累计增长。

### 非目标

- 不修改 ContextPort Published Language 或 `ContextWindow` 生产字段。
- 不新增生产日志，不改变 14 字段 schema。
- 不引入 allocator 或 RSS 采样；peak/retained allocation 留给后续内存切片。
- 不修改 Runtime、SDK 或 TUI。
- 不覆盖 TUI assemble/document/viewport/terminal flush；它们属于后续切片。

## 设计

### ContextBuildPerformanceSnapshot

在 `application/performance.rs` 定义 test-only snapshot 和 thread-local capture scope。collector 只在显式测试 capture 内累加，生产 build 不编译该模块与调用点。

结构字段分为：

1. **调用与耗时**：build、snapshot、prompt、memory、assembly、decision 的 calls/ns。
2. **输入规模**：revision、snapshot/pending/final messages、system blocks。
3. **ToolResult 规模**：block count、content bytes。
4. **Token 决策**：estimated total、provider actual、decision count、decision reason。

### 观测边界

- session `snapshot().await` 前后记录 snapshot 阶段。
- prompt/memory `materialize().await` 分别记录阶段。
- 合并 messages、system blocks、summary/task reminder 记录 assembly 阶段。
- `token_budget` 与 `calculate` 合并记录 decision 阶段，避免为观测重复执行估算。
- ToolResult bytes 以候选 messages 内 `content.to_string().len()` 计数；这是确定性结构工作量，不伪装成 allocator retained bytes。

## TDD

### RED

在 service 同级测试文件构造包含 history、pending 和长 ToolResult 的 fake repository：

1. capture 一次 `build_window`；
2. 断言 revision、message 分类、ToolResult count/bytes、estimated token、actual token 和 decision reason；
3. 当前没有 collector，测试先无法编译或失败。

再增加重复 build 场景，断言两次独立 capture 的结构指标相等，不发生跨 capture 累计。

### GREEN

实现 test-only collector 和阶段接点，使测试通过，不改变 `ContextWindow`、ContextPort 或生产日志。

### Release workload

矩阵：

- 100 条短文本消息；
- 500 条混合中英文/Markdown 消息；
- 1000 条消息；
- 100 条消息，其中每 10 条含一个 64 KiB ToolResult。

每个场景运行固定样本数，输出 build P50/P95、阶段 P50/P95、message/tool-result/token 指标。性能数值只输出用于比较，测试仅断言结构和非零工作量，避免依赖墙钟阈值。

## 验证

- `cargo fmt --all -- --check`
- `cargo test -p context --lib --no-fail-fast`
- `cargo test -p context --test application_service_contract --no-fail-fast`
- `cargo test -p context --release context_build_release_workload -- --ignored --nocapture`
- `cargo test -p context --no-fail-fast`
- `cargo check -p context`
- `cargo clippy -p context --all-targets -- -D warnings`
- `git diff --check`

## 后续切片

1. Context/session 生命周期与 cache retained object 计数。
2. 进程 allocator/RSS peak 与 retained allocation workload，区分临时峰值和真实 retained growth。
3. TUI 100/500/1000 blocks 的 assemble/document/viewport/terminal flush 分段基线。
4. resume 首帧、resize、spinner-only 的端到端慢帧结构化诊断。
