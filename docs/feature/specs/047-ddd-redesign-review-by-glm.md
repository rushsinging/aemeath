# Feature #47 DDD/COLA 重构 Review

> 生成工具：GLM (Zhipu/glm-5.1) | 日期：2026-05-24

## 审查范围

| 文档 | 路径 |
|------|------|
| Spec | `docs/feature/specs/047-ddd-redesign.md` |
| Phase 1 Plan | `docs/superpowers/plans/2026-05-24-feature-47-ddd-cola-application-service-phase1.md` |
| Phase 2 Plan | `docs/superpowers/plans/2026-05-24-feature-47-chat-runtime-port-phase2.md` |
| Phase 3 Plan | `docs/superpowers/plans/2026-05-24-feature-47-chat-runtime-context-phase3.md` |
| Phase 4 Plan | `docs/superpowers/plans/2026-05-24-feature-47-chat-bootstrapping-boundary-phase4.md` |

**已实现代码**：

| 文件 | 职责 |
|------|------|
| `cli/src/application/chat/request.rs` | DTO：`ChatLaunchOptions`、`NoTuiChatLaunch`、`TuiChatLaunch` |
| `cli/src/application/chat/port.rs` | Port trait：`ChatRuntimePort`、`ChatRuntimeContext`、`TuiChatOutcome` |
| `cli/src/application/chat/service.rs` | Application Service：`ChatApplicationService` |
| `cli/src/run_orchestration/runtime.rs` | Adapter 层：`NoTuiChatRuntimeAdapter`、`TuiChatRuntimeAdapter` + 入口函数 |
| `cli/src/run_orchestration.rs` | 主编排函数 `run_chat`（未改动） |

## 总体评价

Phase 1-3 代码落地质量好，逐步把 `run_chat` 最外层从"直接调用 repl/tui"变成了"通过 Application Service → Port → Adapter 分发"。Phase 4 生成了 plan 但无代码。当前重构的价值被 `run_chat` 仍未瘦身的事实稀释。

## 逐项审查

### 1. `run_chat` 仍是 345 行巨型函数

**严重度**：中

Phase 1-3 重构了 `runtime.rs`（adapter 层），但 `run_orchestration.rs` 的 `run_chat` 从 cwd 解析到并发限制的全部准备逻辑仍然是 280 行线性函数。Phase 4 plan 认识到了这个问题（`ChatBootstrap` + `ChatModeSelection`），但目前只生成了 plan 文档。

**影响**：Phase 1-3 引入的 DTO / Port / Service 抽象层看起来是"为加层而加层"，因为调用方没有变简洁。

**建议**：Phase 4 尽快执行。

### 2. Adapter 入口函数参数列表未缩短

**严重度**：中

`runtime.rs` 中 `run_no_tui`（17 参数）和 `run_tui`（17 参数）仍然使用 `#[allow(clippy::too_many_arguments)]`。它们内部手动将参数组装成 `launch` + `context` 再传给 service——与 `run_chat` 中的组装重复。

```
run_chat 组装参数 → 传给 run_no_tui/run_tui → 再次拆成 launch + context → 传给 service
```

**建议**：Phase 4 的 `ChatBootstrap` 应消除这层重复，让 `run_chat` 直接构建 `launch` + `context`，跳过中间函数。

### 3. `max_agent_concurrency` 归属不明确

**严重度**：低

`max_agent_concurrency` 在 `ChatLaunchOptions` 中（两种 launch 共享），但只有 `TuiChatRuntimeAdapter` 使用它。`NoTuiChatRuntimeAdapter` 通过 `ChatRuntimeContext.agent_semaphore` 间接限制并发，`launch.options.max_agent_concurrency` 被无声忽略。

```rust
// TuiChatRuntimeAdapter 使用：
launch.options.max_agent_concurrency,  // ← 传给 app.run

// NoTuiChatRuntimeAdapter 使用：
context.agent_semaphore,  // ← 来自 ChatRuntimeContext
// launch.options.max_agent_concurrency 从未读取
```

**建议**：将 `max_agent_concurrency` 从 `ChatLaunchOptions` 移到 `TuiChatLaunch` 专属字段，或在 no-TUI adapter 中也使用它。

### 4. `?Send` trait 限制了未来扩展

**严重度**：低（当前无害）

```rust
#[async_trait(?Send)]
pub(crate) trait ChatRuntimePort {
```

`?Send` 是因为测试中 `RecordingRuntimePort` 使用了 `Mutex<usize>`（非 `Send`）。生产 adapter 都是零大小 struct，完全可以 `Send`。`?Send` 导致 port 的 future 不是 `Send`，无法用于 `tokio::spawn`。

**建议**：测试中改用 `std::sync::atomic::AtomicUsize`，去掉 `?Send`。

### 5. `validate` 可被绕过

**严重度**：低

`validate_no_tui_launch` / `validate_tui_launch` 是 `pub(crate)` 关联函数。当前调用路径是 `service → validate → adapter`，保证 validate 一定执行。但同一 crate 内的其他代码可以直接构造 adapter 跳过 validate。

**建议**：当前阶段可接受。Phase 4 暴露 `ChatBootstrap` 时注意封装。

### 6. `ChatBootstrap` 与 `ChatRuntimeContext` 字段重叠

**严重度**：中

Phase 3 的 `ChatRuntimeContext`（13 字段）和 Phase 4 plan 的 `ChatBootstrap`（19 字段）高度重叠。`ChatBootstrap` 包含 `ChatRuntimeContext` 的全部 13 字段 + 6 个额外字段。

**建议**：Phase 4 让 `ChatBootstrap` 内嵌 `ChatRuntimeContext` 字段而非平铺：

```rust
struct ChatBootstrap {
    context: ChatRuntimeContext,
    args: Args,
    cwd: PathBuf,
    resolved_model: ResolvedModel,
    session_id: String,
    max_tool_concurrency: usize,
    max_agent_concurrency: usize,
    mode_selection: ChatModeSelection,
}
```

### 7. Spec 中 `Chat` 的定义未体现在代码中

**严重度**：低（不在 Phase 1-4 范围内）

Spec 定义 "Chat: 一次用户输入触发的完整处理单元"，但代码中 `Chat` 概念没有显式建模。Chat 级别状态（messages、turns、task）仍散落在 `tui::App` 和 `repl::run_repl`。

**建议**：后续 phase 追踪。否则 DDD 重构停留在 DTO 更名层面。

## 代码与 Plan 一致性

| Phase | Plan 落地情况 | 一致性 |
|-------|--------------|--------|
| Phase 1 | `ChatApplicationService` + `NoTuiChatLaunch` / `TuiChatLaunch` + validate | ✅ 完全一致 |
| Phase 2 | `ChatRuntimePort` trait + `NoTuiChatRuntimeAdapter` / `TuiChatRuntimeAdapter` | ✅ 完全一致 |
| Phase 3 | `ChatRuntimeContext` 统一依赖 + `runtime.rs` 简化 | ✅ 完全一致 |
| Phase 4 | 仅 plan 文档，无代码 | ⏳ 待执行 |

## 汇总

| 维度 | 评价 |
|------|------|
| Spec 质量 | 高。DDD 建模清晰，Bounded Context、统一语言、Context Map 完整 |
| Plan 质量 | Phase 1-3 步骤精确、可直接执行；Phase 4 偏高层描述、缺精确代码片段 |
| 代码质量 | 干净，测试充分（5 个 service test + DTO validate test） |
| 当前价值 | Application Service 边界已建立，但 `run_chat` 未瘦身，价值被稀释 |
| 风险项 | `?Send` trait、DTO 重叠、`max_agent_concurrency` 归属 |

## 建议优先级

1. **高**：执行 Phase 4，让 `ChatBootstrap` 内嵌 `ChatRuntimeContext`
2. **中**：`max_agent_concurrency` 移到 `TuiChatLaunch` 专属字段
3. **低**：去掉 `?Send`，测试改用 `AtomicUsize`
