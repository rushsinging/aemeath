<!-- Migrated from: docs/feature/archived/058-tui-output-render-pipeline.md -->
# Feature #58：TUI 输出区渲染管线统一重构

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：高

## 背景

#53 TUI Model/View 单源迁移后，输出区渲染仍存在双表示（legacy `OutputLine`/`LineStyle`/`lines` 字段 + 新 ViewModel→Render 管线并存）、有损桥（命令式 `push_system`/`push_error`/`push_user_message`/streaming 旧重渲/AskUser 选项原地改写 `lines`）、markdown+theme 在工具结果/diff 路径未接线等系统性问题。详见 [plan](../superpowers/plans/2026-05-29-tui-output-render-pipeline.md) 与 [spec](../superpowers/specs/2026-05-29-tui-output-render-pipeline-design.md)。

## 解决方案

统一为单一 **ViewModel → Render** 管线，恢复 markdown+theme，消除有损桥/双表示，按 Phase 推进：

### Phase 5（迁移）

- **T1** 迁移 `push_system`/`push_error` 到单一真相源。
- **T2** 迁移 `push_user_message` 到单一真相源。
- **T3** 清理 tool_display 旧命令式渲染。
- **T4** 删除 streaming 旧重渲与 legacy 排队机制。
- **T5** 删除旧「行级渲染链」及其缓存。
- **T6a** 迁移 agent_progress / cancelled / init 横幅到 ConversationModel。
- **T6b** 迁移 AskUser 选项交互（↑↓ 导航、Space 勾选、Enter 提交、Esc 取消、自由输入子态）到 ConversationModel 单一真相，AskUser 选项块由 ConversationModel 单写入者+ViewModel 派生，selected 索引在 conversation/runtime 流转，View 仅消费。
- **T6c** 删除 `OutputLine`/`LineStyle`/`lines` 字段/legacy 垫片/镜像，输出区只剩 `document: RenderedDocument` 单一表示。

### Phase 6（守卫 + 收尾）

- **T7** 渲染隔离守卫 + 删除门禁 + 回归收尾。

### 后续 Gap 补完（G1-G4）

- **G1** diff 完整接线（含 #61 原始场景修复）：`---DIFF---` 标记驱动 Edit 子块渲染加减色 diff，恢复行号/缩进。
- **G2** 工具结果 fence/markdown 接线（修 #65）+ 结果摘要 gap 收口：assistant 与工具结果共用的 fence/markdown 状态机提取为共享原语 `primitives/fenced.rs::render_fenced_markdown`，DRY；fence 结束后普通行恢复 base 色，结构上隔离泄漏。
- **G3** 排队中输入即时显示接线：用户在 spinner 期间提交的下一轮输入即时显示在历史下方而非堆在 input_queue。
- **G4** markdown 完整性（嵌套列表 + 引用块）+ AskUser default 行 + done 间距：补完嵌套列表/引用块渲染，AskUser default 行展示，done 后块间距规范化。

## 关联修复

- 结构性修复 bug #76（reasoning 后 Grep 扁平渲染）、#80（滚动条不跟随）、#62（Grep 标题不可见）、#82（tool call theme 颜色丢失）、#83（tool result 重复刷屏）、#65（fence 样式泄漏）、#94（Bash 阻塞 streaming）。
- 与 #63（Block trait 化 + 嵌套规则 + gutter）协同。

## 验证

`cargo test -p cli` 全通过、`cargo clippy -p cli -- -D warnings`、`.agents/hooks/check-architecture-guards.sh` 通过。2026-05-30 用户确认 feature #58 已完成。

## 详细 Phase/Gap 文档

每个 Phase/Gap 的详情段曾在 `docs/feature/active.md` 跟踪，归档前从 active 移除。完整设计与实施细节见 spec/plan：

- spec：`docs/superpowers/specs/2026-05-29-tui-output-render-pipeline-design.md`
- plan：`docs/superpowers/plans/2026-05-29-tui-output-render-pipeline.md`
