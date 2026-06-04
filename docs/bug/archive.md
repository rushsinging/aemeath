# 已归档 Bug

| # | 标题 | 归档文件 |
|---|------|----------|
| 3 | 优化 tool call TUI 显示 | [archived/003-tool-call-tui-display.md](archived/003-tool-call-tui-display.md) |
| 17 | 对话进行中无法粘贴 | [archived/017-paste-while-processing.md](archived/017-paste-while-processing.md) |
| 20 | aemeath run --provider litellm 无反应直接退出 | [archived/020-litellm-provider-exit.md](archived/020-litellm-provider-exit.md) |
| 21 | 对话中粘贴内容直接进 input queue（应进 input area） | [archived/021-paste-enters-input-queue.md](archived/021-paste-enters-input-queue.md) |
| 22 | Resume 时部分 tool call 信息显示不全 | [archived/022-resume-tool-call-display.md](archived/022-resume-tool-call-display.md) |
| 23 | Input queue 内容在 TUI 显示时未适配换行符 | [archived/023-input-queue-newlines.md](archived/023-input-queue-newlines.md) |
| 24 | Tool call 执行时 spinner 偶尔消失 | [archived/024-spinner-disappears-during-tool-call.md](archived/024-spinner-disappears-during-tool-call.md) |
| 25 | /clear 命令未清空 status line 数据 | [archived/025-clear-status-line.md](archived/025-clear-status-line.md) |
| 26 | 几乎每次对话都触发 superpowers skill 调用 | [archived/026-superpowers-skill-trigger.md](archived/026-superpowers-skill-trigger.md) |
| 27 | Sub-agent 已执行 tool call 但 task list 状态不更新 | [archived/027-sub-agent-task-status.md](archived/027-sub-agent-task-status.md) |
| 28 | Output Area 选中/渲染时 panic：slice/split_off 越界 | [archived/028-output-area-selection-render-panic.md](archived/028-output-area-selection-render-panic.md) |
| 29 | 主 agent tool call 执行后 task list 状态不更新 | [archived/029-main-agent-task-status.md](archived/029-main-agent-task-status.md) |
| 30 | 对话过程中 input queue 不被消费，消息卡在队列里 | [archived/030-input-queue-not-consumed.md](archived/030-input-queue-not-consumed.md) |
| 31 | WebSearch 工具返回空结果（DuckDuckGo HTML 结构变更） | [archived/031-web-search-empty-results.md](archived/031-web-search-empty-results.md) |
| 32 | Task list 窗口化显示异常 | [archived/032-task-window-shrink.md](archived/032-task-window-shrink.md) |
| 33 | Spinner 下方 task list 无法选中、复制和高亮 | [archived/033-task-list-selection.md](archived/033-task-list-selection.md) |
| 34 | Task reminder 干扰新用户请求 | [archived/034-task-reminder-interference.md](archived/034-task-reminder-interference.md) |
| 35 | Write tool 在 worktree 中写入错误分支 | [archived/035-write-tool-worktree-path.md](archived/035-write-tool-worktree-path.md) |
| 36 | TaskListCreate 后新任务编号未从 1 开始 | [archived/036-task-list-numbering.md](archived/036-task-list-numbering.md) |
| 37 | Task list 全部完成后切换对话仍显示旧 task | [archived/037-task-list-stale-after-complete.md](archived/037-task-list-stale-after-complete.md) |
| 38 | Assistant 空消息导致 API 400 invalid_request_error | [archived/038-assistant-empty-message-400.md](archived/038-assistant-empty-message-400.md) |
| 39 | 超大工具结果触发 API 400 string_above_max_length | [archived/039-tool-result-too-large.md](archived/039-tool-result-too-large.md) |
| 40 | DeepSeek 流式输出约 120 秒后 decode timeout | [archived/040-deepseek-streaming-timeout.md](archived/040-deepseek-streaming-timeout.md) |
| 41 | 执行 /reflect 时 TUI 短暂卡死后才出现 LLM 输出 | [archived/041-reflect-tui-freeze.md](archived/041-reflect-tui-freeze.md) |
| 42 | TUI 中 Bash 工具输出中文显示为乱码（M- 转义序列） | [archived/042-bash-chinese-garbled.md](archived/042-bash-chinese-garbled.md) |
| 43 | TaskUpdate 使用全局 id 但 TUI task list 显示局部编号，agent 引用编号不一致 | [archived/043-task-update-global-vs-local-id.md](archived/043-task-update-global-vs-local-id.md) |
| 44 | Bash 工具设置 600s timeout 仍被 120s 截断 | [archived/044-bash-timeout-truncated.md](archived/044-bash-timeout-truncated.md) |
| 45 | / 命令自动补全时上下键不能翻页选择候选 | [archived/045-suggestion-list-scroll.md](archived/045-suggestion-list-scroll.md) |
| 46 | Output area Markdown 表格行选中复制内容错位 | [archived/046-markdown-table-selection-offset.md](archived/046-markdown-table-selection-offset.md) |
| 47 | LLM 声称派发多个 reviewer 但 Agent 实际串行执行 | [archived/047-serial-agent-execution.md](archived/047-serial-agent-execution.md) |
| 48 | Output area 选中复制文本内容错位（含 CJK） | [archived/048-selection-offset-cjk.md](archived/048-selection-offset-cjk.md) |
| 49 | last turn 时用户提交的内容不会发给 LLM，留在 input queue 区域 | [archived/049-input-queue-last-turn.md](archived/049-input-queue-last-turn.md) |
| 50 | input area 为空时按上键进入历史翻看模式 | [archived/050-history-up-empty-input.md](archived/050-history-up-empty-input.md) |
| 51 | Output area 复制时复制出 Markdown 源码而非渲染后纯文本 | [archived/051-output-area-markdown-copy.md](archived/051-output-area-markdown-copy.md) |
| 52 | Tool call spinner 一直闪烁且 tool 结果未更新 | [archived/052-tool-call-spinner-result-not-updated.md](archived/052-tool-call-spinner-result-not-updated.md) |
| 53 | AskUserQuestion 选项未逐行显示，多个选项挤在一行 | [archived/053-ask-user-question-options-lines.md](archived/053-ask-user-question-options-lines.md) |
| 54 | LLM 过度使用 TaskListCreate，简单任务也创建 task list | [archived/054-task-list-overuse.md](archived/054-task-list-overuse.md) |
| 55 | 行内代码（`...`）自动换行处渲染异常 | [archived/055-inline-code-wrap.md](archived/055-inline-code-wrap.md) |
| 56 | Stop hook 返回 exit 2 后 LLM 仍结束 | [archived/056-stop-hook-exit2-ignored.md](archived/056-stop-hook-exit2-ignored.md) |
| 57 | Spinner 有时闪烁过快 | [archived/057-spinner-flicker.md](archived/057-spinner-flicker.md) |
| 58 | TUI 中 Markdown 多行代码块/表格滚出视口后渲染异常 | [archived/058-codeblock-table-scroll-state.md](archived/058-codeblock-table-scroll-state.md) |
| 59 | Input area 翻历史记录时丢失换行且文本超出渲染框 | [archived/059-input-history-multiline.md](archived/059-input-history-multiline.md) |
| 60 | TUI 中 Markdown code 块复制时下划线被吞掉 | [archived/060-markdown-code-underscore-copy.md](archived/060-markdown-code-underscore-copy.md) |
| 61 | Diff 渲染行号顶到最左破坏缩进，且选中后高亮丧失 | [archived/061-diff-indent-selection-highlight.md](archived/061-diff-indent-selection-highlight.md) |
| 62 | Grep 工具执行中标题文字不可见但复制可见 | [archived/062-grep-title-invisible.md](archived/062-grep-title-invisible.md) |
| 63 | AskUserQuestion options 模式上下选择未同步到 TUI 显示 | [archived/063-ask-user-options-selection-sync.md](archived/063-ask-user-options-selection-sync.md) |
| 64 | Agent 未绑定 taskId 仍启动导致 TaskList 无 doing 状态 | [archived/064-agent-missing-taskid.md](archived/064-agent-missing-taskid.md) |
| 65 | 工具结果 fenced code block 后续内容继续显示为 code 颜色 | [archived/065-tool-result-fence-leak.md](archived/065-tool-result-fence-leak.md) |
| 66 | ExitWorktree 带 path 参数报错"已在 worktree 中" | [archived/066-exit-worktree-path.md](archived/066-exit-worktree-path.md) |
| 67 | <code>--resume</code> 失效：进入 TUI 后未加载历史会话 | [archived/067-resume-history-not-loaded.md](archived/067-resume-history-not-loaded.md) |
| 68 | TUI 丢失 context window 用量显示 | [archived/068-context-window-display-missing.md](archived/068-context-window-display-missing.md) |
| 69 | worktree 中 LLM 仍尝试搜索主分支路径 | [archived/069-worktree-main-path-search.md](archived/069-worktree-main-path-search.md) |
| 70 | TaskListCreate 第一次调用会超时 | [archived/070-tasklistcreate-first-timeout.md](archived/070-tasklistcreate-first-timeout.md) |
| 71 | TUI 渲染缓存越界 panic（len 10000 / index 10000）+ unsafe string guard 覆盖不全 | [archived/071-render-cache-out-of-bounds.md](archived/071-render-cache-out-of-bounds.md) |
| 72 | agent 双层循环中一轮结束后不自动读取 input queue | [archived/072-agent-loop-input-queue-drain.md](archived/072-agent-loop-input-queue-drain.md) |
| 73 | EnterWorktree 不能创建 worktree 导致 LLM 回退到主工作区 checkout | [archived/073-enter-worktree-create.md](archived/073-enter-worktree-create.md) |
| 75 | 中文输入法下 input area 输入顺序错乱（查看 → 看查） | [archived/075-ime-cjk-input-order.md](archived/075-ime-cjk-input-order.md) |
| 76 | reasoning 模型 think 后 Grep 结果渲染成扁平原始行且滚动条失效 | [archived/076-reasoning-grep-flat-render.md](archived/076-reasoning-grep-flat-render.md) |
| 77 | @/ / 补全后按空格回退删除字符 | [archived/077-completion-model-stale-overwrite.md](archived/077-completion-model-stale-overwrite.md) |
| 78 | input area 粘贴后按空格清空粘贴内容 | [archived/078-paste-cleared-by-space.md](archived/078-paste-cleared-by-space.md) |
| 80 | 滚动条不跟随最新内容（全量替换时 scroll_offset 累加） | [archived/080-scroll-not-follow-latest.md](archived/080-scroll-not-follow-latest.md) |
| 81 | TUI 输出区中文按单字竖排显示 | [archived/081-cjk-vertical-render.md](archived/081-cjk-vertical-render.md) |
| 82 | TUI 渲染 tool call 时丢失 theme 颜色 | [archived/082-tool-call-theme-color-lost.md](archived/082-tool-call-theme-color-lost.md) |
| 83 | TUI 渲染 tool call 同时输出 summary 和完整内容，重复刷屏 | [archived/083-tool-result-duplicate-flood.md](archived/083-tool-result-duplicate-flood.md) |
| 84 | TUI 未渲染 TaskListCreate 工具调用 | [archived/084-tasklistcreate-not-rendered.md](archived/084-tasklistcreate-not-rendered.md) |
| 85 | Ollama provider 声明但工厂未接线（整模块死代码） | [archived/85-ollama-factory-wiring.md](archived/85-ollama-factory-wiring.md) |
| 86 | TUI tool call 顺序颠倒 | [archived/086-tool-call-order-reversed.md](archived/086-tool-call-order-reversed.md) |
| 87 | TUI tool call 显示完整 tool result 内容且不受 max output 限制，result 渲染格式错误 | [archived/087-tool-result-overflow.md](archived/087-tool-result-overflow.md) |
| 88 | TUI Read tool call 头部下重复显示一行 <code>Read /path</code> | [archived/088-read-path-duplicate.md](archived/088-read-path-duplicate.md) |
| 89 | TUI markdown 表格只渲染表头，分隔行与数据行原样泄漏 | [archived/089-markdown-table-header-only.md](archived/089-markdown-table-header-only.md) |
| 90 | TUI Edit 工具结果不渲染为 diff（只显示 ✓ Edit completed 摘要） | [archived/090-edit-result-not-diff.md](archived/090-edit-result-not-diff.md) |
| 91 | TUI thinking block 渲染时被缩进，破坏 markdown 表头与代码块 | [archived/091-thinking-indent-breaks-markdown.md](archived/091-thinking-indent-breaks-markdown.md) |
| 92 | --resume CLI 路径未为 Resume 模式加载 InputModel 输入历史 | [archived/092-resume-cli-input-history.md](archived/092-resume-cli-input-history.md) |
| 93 | TUI 工具结果块内重复显示工具名和图标 | [archived/093-tool-result-header-duplicate.md](archived/093-tool-result-header-duplicate.md) |
| 94 | Bash 工具运行时阻塞 LLM 流式渲染 | [archived/094-bash-blocks-streaming.md](archived/094-bash-blocks-streaming.md) |
| 95 | Agent tool result 被归为 orphan | [archived/095-agent-tool-result-orphan.md](archived/095-agent-tool-result-orphan.md) |
| 99 | input area 里上下键始终翻看历史，无法上下移动光标 | [archived/099-input-up-down-cursor.md](archived/099-input-up-down-cursor.md) |
| 101 | HookUi 只发一次 HookStart，多 hook 场景下 spinner 只显示第一个 hook 命令 | [archived/101-hook-start-per-hook.md](archived/101-hook-start-per-hook.md) |
| 103 | EnterWorktree/ExitWorktree 在 TUI 中显示原始 JSON 参数内容 | [archived/103-worktree-tool-display.md](archived/103-worktree-tool-display.md) |
| 104 | input queue drain 后没有在 TUI 中显示 | [archived/104-input-queue-drain-display.md](archived/104-input-queue-drain-display.md) |
| 105 | TUI 中 <code>```text</code> fenced block 被当作代码块显示而非 Markdown 渲染 | [archived/105-text-fence-markdown.md](archived/105-text-fence-markdown.md) |
| 106 | TUI 输出区渲染未预留滚动条列宽，右侧文字与滚动条重叠且长行不自动换行 | [archived/106-scrollbar-overlap.md](archived/106-scrollbar-overlap.md) |
| 107 | TUI Rust fenced code 使用 `rust` 语言名时没有 syntect 高亮 | [archived/107-rust-fence-highlight.md](archived/107-rust-fence-highlight.md) |
| 108 | TUI diff 代码块没有统一走 syntect 高亮 | [archived/108-diff-syntect-highlight.md](archived/108-diff-syntect-highlight.md) |
| 109 | TUI syntect 高亮主题使用 base16-ocean.dark，与 Catppuccin Macchiato UI 主题不一致 | [archived/109-syntect-theme-macchiato.md](archived/109-syntect-theme-macchiato.md) |
| 113 | AskUserQuestion 回答后 LLM 新输出渲染到 AskUser 块上方；AskUser 块本身固定在初始位置 | [archived/113-askuser-output-order.md](archived/113-askuser-output-order.md) |
| 114 | Stop hook blocked 缺少显式 chat loop 停止状态表达 | [archived/114-stop-hook-blocked-chat-loop-state.md](archived/114-stop-hook-blocked-chat-loop-state.md) |
