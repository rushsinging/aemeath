# 已归档 Feature

> 排序规范：表格行均按 ID 升序排列。同一 ID 对应多个归档文件时，各保留一行，行间顺序不限。

| # | 标题 | 归档文件 |
|---|------|----------|
| 1 | Hook 功能（参考 Claude Code 设计） | [archived/001-hook-system.md](archived/001-hook-system.md) |
| 2 | SubAgent 可配置 | [archived/002-subagent-configurable.md](archived/002-subagent-configurable.md) |
| 3 | CLI 子命令 | [archived/003-cli-subcommands.md](archived/003-cli-subcommands.md) |
| 4 | AskUserQuestion TUI 美化 | [archived/004-ask-user-question-tui.md](archived/004-ask-user-question-tui.md) |
| 5 | Agent 调用显示优化 | [archived/005-agent-display.md](archived/005-agent-display.md) |
| 6 | Task 调用显示优化 | [archived/006-task-display.md](archived/006-task-display.md) |
| 7 | Input Queue 优化 | [archived/007-input-queue.md](archived/007-input-queue.md) |
| 10 | 日志文件规范化 | [archived/010-log-file-spec.md](archived/010-log-file-spec.md) |
| 11 | Esc 打断对话 | [archived/011-esc-interrupt.md](archived/011-esc-interrupt.md) |
| 11 | OpenAI reasoning_effort 配置支持 | [archived/011-openai-reasoning-effort.md](archived/011-openai-reasoning-effort.md) |
| 12 | Input Queue 双层循环优化 | [archived/012-input-queue-double-loop.md](archived/012-input-queue-double-loop.md) |
| 13 | Task list 显示在 spinner 下方 | [archived/013-task-list-below-spinner.md](archived/013-task-list-below-spinner.md) |
| 14 | Session ID 自增无冲突方案 | [archived/014-session-id-uuidv7.md](archived/014-session-id-uuidv7.md) |
| 15 | 通过 max_tokens 配置 LLM 输出 + thinking 双上限 | [archived/015-max-tokens-thinking-budget.md](archived/015-max-tokens-thinking-budget.md) |
| 16 | Spinner 行合并状态显示 + Hook 调用信息 | [archived/016-spinner-merged-status-hook-info.md](archived/016-spinner-merged-status-hook-info.md) |
| 17 | Skill 延迟加载 + 命名空间前缀 | [archived/017-skill-lazy-load-namespace.md](archived/017-skill-lazy-load-namespace.md) |
| 18 | Task list 跨轮次 batch 机制 | [archived/018-task-list-batch.md](archived/018-task-list-batch.md) |
| 19 | config model 支持 zhipu api 类型 | [archived/019-zhipu-api-type.md](archived/019-zhipu-api-type.md) |
| 20 | config model 支持 litellm api 类型 | [archived/020-litellm-api-type.md](archived/020-litellm-api-type.md) |
| 21 | TUI 优化 Agent 调用输出展示 | [archived/021-tui-agent-output-display.md](archived/021-tui-agent-output-display.md) |
| 23 | TUI 字符串/切片安全索引收口 | [archived/023-safe-text-index.md](archived/023-safe-text-index.md) |
| 23 | TUI 字符串/切片安全索引收口 | [archived/023-tui-safe-text-indexing.md](archived/023-tui-safe-text-indexing.md) |
| 24 | Spinner 下方 task list 限量显示（最多 7 条） | [archived/024-task-list-windowed-display.md](archived/024-task-list-windowed-display.md) |
| 25 | Task list 跨轮次生命周期策略 | [archived/025-task-lifecycle-policy.md](archived/025-task-lifecycle-policy.md) |
| 27 | 日志分化：input.log / output.log / tool.log | [archived/027-log-split.md](archived/027-log-split.md) |
| 29 | Task reminder 被动注入 | [archived/029-task-reminder-passive-injection.md](archived/029-task-reminder-passive-injection.md) |
| 30 | Agent loop 收尾工作 | [archived/030-agent-loop-finalize.md](archived/030-agent-loop-finalize.md) |
| 31 | TUI 架构守卫脚本（TEA 纯度 + 400 行限制） | [archived/031-tui-architecture-guards.md](archived/031-tui-architecture-guards.md) |
| 32 | TUI 选中和复制逻辑统一 | [archived/032-tui-selection-copy-unify.md](archived/032-tui-selection-copy-unify.md) |
| 33 | 优化 TaskListCreate / TaskListComplete 工具调用显示 | [archived/033-task-list-display-optimization.md](archived/033-task-list-display-optimization.md) |
| 35 | Diff 渲染 add 行语法高亮 + 行号显示 | [archived/035-diff-add-highlight-line-numbers.md](archived/035-diff-add-highlight-line-numbers.md) |
| 37 | 火山引擎（Volcengine）Coding Plan Provider | [archived/037-volcengine-coding-plan-provider.md](archived/037-volcengine-coding-plan-provider.md) |
| 39 | Ctrl+C 两段式退出 | [archived/039-ctrlc-two-stage-exit.md](archived/039-ctrlc-two-stage-exit.md) |
| 39 | TUI 配色方案重新设计 | [archived/039-tui-theme-redesign.md](archived/039-tui-theme-redesign.md) |
| 40 | 配置文件改造：对齐 Claude 优先兼容的 <code>~/.agents</code> / <code>CLAUDE.md</code> / skills 读取 | [archived/040-claude-compatible-agents-config.md](archived/040-claude-compatible-agents-config.md) |
| 43 | 在 git worktree 中工作时 cwd 应设置为 worktree 目录 | [archived/043-worktree-cwd-context.md](archived/043-worktree-cwd-context.md) |
| 44 | Commit Style Context 与 AI 协作者 trailer | [archived/044-commit-style-context.md](archived/044-commit-style-context.md) |
| 45 | 为 LLM 提供 EnterWorktree / ExitWorktree 工具 | [archived/045-enter-exit-worktree-tools.md](archived/045-enter-exit-worktree-tools.md) |
| 46 | TUI status line 增加第二行并显示 cwd/current worktree | [archived/046-tui-status-line-worktree-context.md](archived/046-tui-status-line-worktree-context.md) |
| 47 | 以 DDD 思路重新设计 Aemeath 架构 | [archived/047-ddd-redesign.md](archived/047-ddd-redesign.md) |
| 48 | TUI 窗口 resize 时重新计算渲染层并刷新显示层 | [archived/048-tui-resize-render-refresh.md](archived/048-tui-resize-render-refresh.md) |
| 50 | CLI 目录整理 — 收拢碎片、统一模块层级 | [archived/050-cli-directory-cleanup.md](archived/050-cli-directory-cleanup.md) |
| 51 | UI Domain DDD 设计 — 将 apps/cli 提升为核心域 | [archived/051-ui-domain-ddd-design.md](archived/051-ui-domain-ddd-design.md) |
| 53 | TUI Model/View 架构迁移 | [archived/053-tui-model-view-migration.md](archived/053-tui-model-view-migration.md) |
| 54 | 主动压缩触发：大上下文下防止 LiteLLM 代理拒绝 | [archived/054-proactive-compaction-trigger.md](archived/054-proactive-compaction-trigger.md) |
| 55 | TUI 架构收口 — render / adapter / app 三层落地 + 清理 legacy core | [archived/055-tui-render-adapter-app-layers.md](archived/055-tui-render-adapter-app-layers.md) |
| 56 | 输入单一真相约束 — 禁止直接改 input_area 并加架构 guard | [archived/056-input-single-source-guard.md](archived/056-input-single-source-guard.md) |
| 57 | TUI 目录物理收口 — 并入剩余 widget/service 目录、删 core shim | [archived/057-tui-toplevel-physical-cleanup.md](archived/057-tui-toplevel-physical-cleanup.md) |
| 58 | TUI 输出区渲染管线统一重构 | [archived/058-tui-output-render-pipeline.md](archived/058-tui-output-render-pipeline.md) |
| 59 | TUI Model/View 单源迁移收口（伞型 roadmap） | [archived/059-tui-single-source-roadmap.md](archived/059-tui-single-source-roadmap.md) |
| 60 | Auto-compact LLM 语义化压缩 | [archived/060-auto-compact-llm-summary.md](archived/060-auto-compact-llm-summary.md) |
| 61 | 架构债务收口（047 DDD 软约束落实） | [archived/061-ddd-architecture-debt-closure.md](archived/061-ddd-architecture-debt-closure.md) |
| 63 | TUI Block 抽象 trait 化 + 真正渲染树（嵌套规则）+ gutter | [archived/063-tui-block-trait-nesting.md](archived/063-tui-block-trait-nesting.md) |
| 64 | TUI tool call result 子块展示 output 内容预览 | [archived/064-tool-result-preview.md](archived/064-tool-result-preview.md) |
| 65 | Resume 模式输入历史 — 上下键翻阅过往输入 | [archived/065-resume-input-history.md](archived/065-resume-input-history.md) |
| 66 | 去除 mod.rs 旧写法 + 架构 guard | [archived/066-no-mod-rs-guard.md](archived/066-no-mod-rs-guard.md) |
| 67 | Task/project window 改为 ChangeSet 驱动刷新 | [archived/067-changeset-task-project-refresh.md](archived/067-changeset-task-project-refresh.md) |
| 70 | 统一 input_queue 到事件驱动 | [archived/070-unify-input-queue-event-driven.md](archived/070-unify-input-queue-event-driven.md) |
| 71 | Stop hook 日志输出项目目录上下文 | [archived/071-stop-hook-project-dir-context.md](archived/071-stop-hook-project-dir-context.md) |
| 72 | Edit diff 显示真实文件行号 | [archived/072-edit-diff-real-line-numbers.md](archived/072-edit-diff-real-line-numbers.md) |
| 73 | AGENTS.md 渐进式披露重构 | [archived/073-agents-md-progressive-disclosure.md](archived/073-agents-md-progressive-disclosure.md) |
| 74 | Guidance — 任务执行期间用户提问时同步更新 task list | [archived/074-task-list-scope-change-guidance.md](archived/074-task-list-scope-change-guidance.md) |
| 76 | TUI spinner 时长显示改进 | [archived/076-spinner-duration-display.md](archived/076-spinner-duration-display.md) |
