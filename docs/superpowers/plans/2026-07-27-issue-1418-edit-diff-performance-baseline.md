# #1418 Edit diff 性能基线切片实施计划

> **执行要求：** 使用独立 worktree，严格按 TDD 顺序完成；本计划只建立可重复证据，不实施 #1420 的高亮复用或 viewport virtualization。

**目标：** 为长会话中的大型 Edit diff 建立确定、可重复、可比较的 TUI 性能基线，能够量化完整 diff 构建、syntax highlight 工作量以及静态 diff 跨 spinner frame 的缓存行为。

**架构：** 在 TUI render owner 内增加一个仅测试 target 编译、作用域化的诊断 collector。`syntax`、diff 构建和 `OutputDocumentRenderer` 通过 `cfg(test)` 上报计数与阶段耗时，release 生产二进制不含计时与计数路径。自动测试断言结构性工作量与缓存不变量，ignored release workload 重复采样并输出 P50/P95，避免把不稳定墙钟阈值放入普通 CI。

**技术栈：** Rust 2021、ratatui、syntect/oniguruma、现有 ConversationModel/ViewAssembler/OutputDocumentRenderer、Cargo tests。

**Issue：** [#1418](https://github.com/rushsinging/aemeath/issues/1418)，Parent [#1417](https://github.com/rushsinging/aemeath/issues/1417)。

---

## 范围与非目标

### 本切片包含

- 固定 Edit conversation fixture：多 Edit ToolResult、大 diff、Rust/Markdown/Unicode、异常长行。
- TUI render 阶段诊断：document render、Edit diff render、diff build、syntax highlight。
- 结构计数：diff 数、diff 总行数、高亮调用/输入行/输入字节、block cache hit/miss、gutted cache hit/miss。
- 自动回归：同 revision/width 下 spinner frame 变化不得重新渲染或重新高亮静态 Edit diff。
- ignored release workload：resume-like 首次 assemble、cold render、warm/spinner render 的多轮 P50/P95。

### 本切片不包含

- 不修改 `HighlightLines` 的生命周期与复用策略。
- 不增加 viewport、overscan、diff 行预算或超大 diff 降级。
- 不增加 Context snapshot、allocation、token drift 指标；这些属于 #1418 后续切片。
- 不引入 Criterion、dhat 或 allocator 替换；当前先固定结构性计数和 release workload。
- 不新增常态 per-frame 日志，避免观测本身放大卡顿。

## 文件结构

- Create: `apps/cli/src/tui/render/performance.rs`
  - 定义 `RenderPerformanceSnapshot`、作用域 collector、阶段计时与计数 API。
  - collector 仅在 test target 编译；workload/测试显式启用。
- Create: `apps/cli/src/tui/render/performance_tests.rs`
  - L1：collector 作用域隔离、阶段累加、P50/P95 计算与空输入边界。
- Modify: `apps/cli/src/tui/render.rs`
  - 注册 performance 模块及分离测试文件。
- Modify: `apps/cli/src/tui/render/syntax.rs`
  - 在 `highlight_line` 入口记录调用数、输入行/字节与耗时；不改变高亮实现。
  - 将现有内联测试迁到 `syntax_tests.rs`，符合当前测试分离门禁。
- Create: `apps/cli/src/tui/render/syntax_tests.rs`
  - 保留现有语义断言并增加观测断言。
- Modify: `apps/cli/src/tui/render/output/diff.rs`
  - 在 `build_diff_lines_from` 记录 diff 数、输出变化行数与阶段耗时；不裁剪、不降级。
  - 将现有内联测试迁到 `diff_tests.rs`。
- Create: `apps/cli/src/tui/render/output/diff_tests.rs`
  - 保留现有 diff 断言并增加大型 diff 工作量计数断言。
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`
  - 记录 document render 阶段、block cache hit/miss、gutted cache hit/miss。
  - 生产缓存语义不变。
- Modify: `apps/cli/src/tui/render/output/document_renderer/tests.rs`
  - 增加静态 Edit diff 跨 spinner frame 的结构性缓存断言。
- Modify: `apps/cli/src/tui/view_assembler/output_tests.rs`
  - 停止新增 `include!` 债务，将 performance workload 作为正常子模块声明。
- Create: `apps/cli/src/tui/view_assembler/output_tests/edit_diff_performance_tests.rs`
  - 放置确定性 fixture、自动结构测试与 ignored release workload。
- Modify: `apps/cli/src/tui/view_assembler/output_tests/bench_tests.rs`
  - 保留历史通用 refresh benchmark；Edit 基线不继续堆入该文件。

## Task 1：先建立 collector 的 RED 测试

**文件：**
- Create: `apps/cli/src/tui/render/performance_tests.rs`
- Modify: `apps/cli/src/tui/render.rs`

- [ ] 写 `capture_when_scope_active_returns_accumulated_snapshot`，断言作用域内计数和 duration 累加。
- [ ] 写 `record_when_scope_inactive_is_noop`，断言未启用 capture 时无状态残留。
- [ ] 写 `percentiles_sort_samples_and_use_nearest_rank`，覆盖空、单值和多值 P50/P95。
- [ ] 运行 `cargo test -p cli tui::render::performance::tests -- --nocapture`，确认因 API 尚不存在而 RED。

## Task 2：实现最小作用域诊断 collector

**文件：**
- Create: `apps/cli/src/tui/render/performance.rs`
- Modify: `apps/cli/src/tui/render.rs`

- [ ] 定义仅 TUI crate 内可见的 `RenderPerformanceSnapshot`，字段至少包括：
  - `document_render_calls/ns`
  - `edit_diff_calls/ns`
  - `diff_build_calls/ns/output_lines`
  - `syntax_highlight_calls/ns/input_bytes`
  - `block_cache_hits/misses`
  - `gutted_cache_hits/misses`
- [ ] 使用 thread-local scope，禁止跨测试线程共享可变全局计数；模块与调用点均受 `cfg(test)` 约束，生产二进制零观测开销。
- [ ] 提供 RAII 阶段 timer；Drop 时只在 active scope 累加。
- [ ] 提供纯函数 nearest-rank P50/P95。
- [ ] 运行 Task 1 测试并确认 GREEN。

## Task 3：先为 syntax/diff 观测建立 RED

**文件：**
- Create: `apps/cli/src/tui/render/syntax_tests.rs`
- Create: `apps/cli/src/tui/render/output/diff_tests.rs`
- Modify: `apps/cli/src/tui/render/syntax.rs`
- Modify: `apps/cli/src/tui/render/output/diff.rs`

- [ ] 迁移当前内联测试，不削弱原断言。
- [ ] 写 `highlight_line_records_call_bytes_and_duration_when_capture_active`。
- [ ] 写 `large_diff_records_all_output_and_highlighted_lines`：构造固定 100+ 行 Rust diff，断言当前实现完整处理输出；删除行不高亮，新增/上下文行的高亮调用数可精确计算。
- [ ] 运行定向测试，确认计数仍为零而 RED。

## Task 4：接入 syntax/diff 分阶段观测

**文件：**
- Modify: `apps/cli/src/tui/render/syntax.rs`
- Modify: `apps/cli/src/tui/render/output/diff.rs`

- [ ] `highlight_line` 在确认 syntax 存在后记录调用、输入字节和耗时；错误仍按现有 `None` 回退。
- [ ] `build_diff_lines_from` 记录调用、最终输出行数和整个 diff build 耗时。
- [ ] 不改变 `HighlightLines::new` 每行创建的现状，确保基线真实反映事故实现。
- [ ] 运行 Task 3 测试并确认 GREEN。

## Task 5：先建立静态 Edit diff 缓存 RED

**文件：**
- Modify: `apps/cli/src/tui/render/output/document_renderer/tests.rs`

- [ ] 构造包含 completed Edit ToolCall + structured ToolResult child 的 `OutputViewModel`。
- [ ] 在 capture scope 内执行 cold render，再以不同 spinner frame 执行 warm render。
- [ ] 分别截取 cold/warm delta，断言：
  - cold render 有一次 Edit diff build 和非零 syntax highlight；
  - warm render 的 `edit_diff_calls/diff_build_calls/syntax_highlight_calls` 均为零；
  - 静态 ToolCall 与 ToolResult 的 block/gutted cache 命中，miss 不增长。
- [ ] 运行定向测试；由于 cache hit/miss 尚未接入观测而 RED。

## Task 6：接入 document/cache 观测

**文件：**
- Modify: `apps/cli/src/tui/render/output/block_cache.rs`
- Modify: `apps/cli/src/tui/render/output/document_renderer.rs`

- [ ] `BlockCache::get_or_render` 在 key 命中/未命中时分别计数。
- [ ] `render_tree_with_animation_frame` 使用 timer 记录完整 document render。
- [ ] `gutted` cache 分支记录 hit/miss。
- [ ] 不改变现有 key、retain、Rc clone 和 spinner marker 语义。
- [ ] 运行 Task 5 及现有 cache 测试并确认 GREEN。

## Task 7：建立 Edit conversation workload

**文件：**
- Modify: `apps/cli/src/tui/view_assembler/output_tests.rs`
- Create: `apps/cli/src/tui/view_assembler/output_tests/edit_diff_performance_tests.rs`

- [ ] fixture 通过真实 `ConversationModel` intents 构造 completed Edit 调用，固定 ID，不访问文件、网络或 Provider。
- [ ] structured `content` 使用 `old/new/start_line`，`args_preview` 提供 `.rs` 路径以启用 syntect。
- [ ] 自动测试覆盖：
  - 多 Edit fixture assemble 后产生预期 ToolResult 子块数量；
  - cold render 的 diff/highlight 工作量随 fixture 规模增加；
  - warm/spinner render 不重复 diff/highlight。
- [ ] ignored release workload：
  - 规模至少包含 100/500/1000 diff lines 或等价 blocks × lines 矩阵；
  - 每个场景预热后重复采样；
  - 输出 assemble、cold、warm 的 P50/P95，以及 snapshot 中的结构计数；
  - 只打印汇总，不设置机器相关毫秒阈值。
- [ ] 手动运行 release workload，保存本地输出到 PR Test Plan，不提交机器生成 artifact。

## Task 8：验证与审查

- [ ] `cargo fmt --check`
- [ ] `cargo test -p cli tui::render::performance_tests -- --nocapture`
- [ ] `cargo test -p cli tui::render::syntax -- --nocapture`
- [ ] `cargo test -p cli tui::render::output::diff -- --nocapture`
- [ ] `cargo test -p cli tui::render::output::document_renderer -- --nocapture`
- [ ] `cargo test -p cli edit_diff_performance -- --nocapture`
- [ ] `cargo test -p cli`
- [ ] `cargo build -p cli`
- [ ] `cargo clippy -p cli --all-targets -- -D warnings`
- [ ] 运行与 TUI TEA、render purity、unsafe text、no-inline-tests、日志 target 相关的架构守卫。
- [ ] 检查 `git diff --check` 与工作树状态。
- [ ] 对照 #1418 checklist 自审：本 PR 只声明完成 Edit diff 基线切片，不关闭 #1418。

## 验收证据

本切片完成时必须同时提供：

1. 自动结构断言证明 cold render 完整处理大型 Edit diff。
2. 自动结构断言证明静态 Edit diff 跨 spinner frame 不重新高亮。
3. ignored release workload 输出可复跑的 P50/P95 与工作量计数。
4. 无生产行为变化、无常态日志噪声、无真实 Provider 依赖。
5. 以上验证命令全部通过，或对不可用门禁记录可验证原因。
