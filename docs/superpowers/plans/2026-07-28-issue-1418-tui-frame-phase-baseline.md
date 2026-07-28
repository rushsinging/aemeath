# #1418 TUI 帧阶段性能基线切片实施计划

> **执行要求：** 在独立 worktree 中按 TDD 实施。本切片只建立 TUI assemble、viewport render、terminal buffer diff/backend flush 的可重复性能基线，不引入生产慢帧日志、allocator/RSS 采样或渲染策略变更。

**目标：** 对长会话的一帧渲染建立 test-only 分阶段观测，稳定回答 assemble 处理了多少 timeline item、viewport 从多大文档投影了多少可见行、Ratatui buffer diff 写出了多少 cell，以及 backend flush 调用了多少次、各阶段耗时多少。

**Issue：** [#1418](https://github.com/rushsinging/aemeath/issues/1418)，Parent [#1417](https://github.com/rushsinging/aemeath/issues/1417)。

## 范围

### 本切片包含

- 扩展现有 test-only `RenderPerformanceSnapshot`：
  - assemble calls/ns、source timeline items、output roots；
  - viewport render calls/ns、source lines、visible lines；
  - terminal draw calls/ns、diff cells；
  - backend flush calls/ns。
- 在 `OutputViewAssembler::assemble_from_conversation` 与 `OutputArea::render` 的真实入口接入采集。
- 增加 test-only `InstrumentedBackend<B>`，完整委托 Ratatui `Backend`，只在 `draw` 和 `flush` 边界记录工作量与耗时。
- 建立 100/500/1000 blocks 的确定性帧 workload，覆盖 cold draw、warm/spinner-only draw 与 resize draw。
- ignored release workload 输出 assemble、viewport、terminal draw、backend flush 和整体 draw 的 P50/P95；普通测试只断言结构性工作量，不设置机器相关墙钟阈值。

### 非目标

- 不采集真实 Crossterm 写终端的稳定阈值；CI 的 backend flush 使用 `TestBackend` wrapper，只证明阶段边界和调用次数。
- 不实现 allocator/RSS peak 或 retained allocation；这是第 6 切片。
- 不实现生产慢帧结构化日志、resume 首帧诊断；这是第 7 切片。
- 不改变 viewport 算法、缓存键、历史窗口、spinner、resize 或绘制语义。
- 不向普通生产构建加入计时、计数或测试 Backend。

## 测试层级

- **L1：** collector 字段累加、inactive no-op、backend 委托与 diff/flush 计数。
- **L2：** assembler 与 output-area 真实入口分别上报结构指标。
- **L4：** `App::prepare_frame + App::draw + InstrumentedBackend<TestBackend>` 组合 workload，证明整帧阶段链路。
- **L0：** production build、all-target clippy、fmt 与架构守卫，证明 test-only 能力未泄漏生产路径。

## TDD 顺序

1. 扩展 collector 测试，先引用尚不存在的字段和 record API，确认 RED。
2. 增加 assembler 与 viewport 入口测试，先断言当前 snapshot 没有阶段数据，确认 RED。
3. 最小实现 collector 字段、record API 和两个真实入口接点，确认 GREEN。
4. 为 `InstrumentedBackend<TestBackend>` 写委托、diff cell 与 flush 计数测试，先确认 API 缺失的 RED。
5. 实现完整 Backend wrapper，不修改 `App::draw` 生产签名，确认 GREEN。
6. 增加 100/500/1000 blocks、spinner-only、resize 的帧 workload 与 ignored release workload。
7. 执行定向测试、release workload、CLI 全测、build、clippy、fmt、diff check 和架构守卫。

## 文件范围

- Modify: `apps/cli/src/tui/render/performance.rs`
- Modify: `apps/cli/src/tui/render/performance_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler/output.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_tests.rs`
- Create: `apps/cli/src/tui/view_assembler/output_tests/frame_performance_tests.rs`
- Modify: `apps/cli/src/tui/render/output_area/render.rs`
- Modify: `apps/cli/src/tui/render/output_area/render_tests.rs`
- Modify: `apps/cli/src/tui/app/testing.rs`
- Modify: `apps/cli/src/tui/app/testing/harness.rs`
- Create: `apps/cli/src/tui/app/testing/instrumented_backend.rs`
- Modify: `apps/cli/src/tui/app/scenario_tests.rs`
- Create: `apps/cli/src/tui/app/scenario_tests/frame_performance.rs`

## 验收

- assemble capture 精确记录一次调用的 timeline item 与 root 数。
- viewport capture 精确记录 source document lines 与实际可见 document lines。
- cold draw 的 terminal diff cells 非零；内容不变的 warm draw diff cells 不高于 cold draw。
- 每次成功 `Terminal::draw` 恰有一次 backend flush。
- spinner-only draw 不触发静态内容 re-assemble；resize draw 重新投影 viewport，且阶段数据可断言。
- 100/500/1000 blocks release workload 输出各阶段 P50/P95 和结构计数。
- 生产 build 不包含 collector 或 InstrumentedBackend 路径。
