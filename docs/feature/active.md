# 活动中 Feature

| # | 标题 | 优先级 | 状态 | 确认结果 | 目标 |
|---|------|--------|------|----------|------|
| 8 | Memory 系统 | - | 已完成 | 未确认 | MVP 已落地：MemoryConfig、MemoryStore、/memory 命令、MemoryTool、system prompt 注入配置化，以及对话结束后的 session reminder recap；MemoryTool 存储参数已使用运行时 MemoryConfig，不再硬编码；Hook 兜底自动提取与淘汰确认暂缓。详见 [spec](specs/008-memory-system.md) |
| 9 | 反思系统 | - | 已完成 | 未确认 | 已接入真实 LLM `/reflect`、JSON 解析、pending 建议与 `/reflect apply` 写入 Memory、auto_apply_suggestions 自动写入、自动 N 轮触发；使用当前默认模型，不做独立 reflection model；不做 PostCompact 后反思，避免压缩后上下文损失。详见 [spec](specs/009-reflection-system.md) |
| 28 | MCP 系统完善 | 高 | 🔧 未完成 | 未确认 | P0+P1 已完成：stdio 可用配置、配置层、Manager/API、命令解析、工具注册/注销和默认 1MB tool result 限制已落地；SSE 传输已实现但存在可靠性问题（z.ai SSE server 响应在 tools/list 时经常超时/不完整），MCP 加载已暂时从启动流程中禁用，待修复后重新启用；Streamable HTTP 传输待后续补充 |
| 34 | Anthropic Claude 原生 Provider | 高 | ✅ 已完成 | 未确认 | 原生 Anthropic Claude API 适配（Messages API、流式/非流式、thinking budget、重试、tool use），作为独立 provider 与 OpenAI/OpenRouter 等并列；默认 provider |
| 36 | Multi-Agent 框架 | 高 | 暂停 | 未确认 | 后端分布式实现已按“server 太重”的判断从当前代码树移除：不再保留 `apps/server`、`apps/agents`、`packages/sdk`、`packages/proto`、`infra` 运行代码；仓库回到 CLI + core/llm/tools 为主。历史设计仅保留 spec 与 DDD 文档用于后续参考，不再维护 sprint plan。详见 [架构 spec](specs/036-02-spec-architecture.md) 与 [DDD](../superpowers/specs/2026-05-20-multi-agent-ddd-design.md) |
| 42 | 权限管控系统：交互式外部授权 + 统一权限评估 | 高 | 设计中 | 未确认 | 范围从 Allow All 外部路径访问升级为完整权限管控系统：采用交互式授权体验 + 统一 PermissionEngine 评估模型；权限模式为 AskMe / Auto / Plan / AllowAll，其中 AllowAll 保留 root/YOLO 语义，Auto 是带护栏的日常开发模式，Plan 只分析不执行副作用；Sandbox 仅预留未来扩展。详见 [spec](specs/042-permission-control-system.md) |
| 47 | 以 DDD 思路重新设计 Aemeath 架构 | 高 | ✅ 已完成 | 未确认 | DDD 架构设计已按讨论结果写入 [spec](specs/047-ddd-redesign.md)，并已纳入 [GLM review](specs/047-ddd-redesign-review-by-glm.md) 与 [DeepSeek review](specs/047-ddd-redesign-review-by-deepseek.md) 的修正意见。2026-05-25 合并 #50（CLI 目录整理）、#51（UI Domain DDD）入本 spec：UI 经讨论回归支撑域（薄入口），核心域保持单核（Agent Runtime），新增 AgentClient SDK（packages/sdk）作为唯一通信契约（§6.5）——完整包含 trait/impl 分层、Runtime::initialize() 编排、快照 + watch 变更通道、ChatStream mpsc、cancel AtomicBool。§6.6 CLI 边界重新设计：CLI 仅 parse args + load config + 启动，AgentClient::new() 吞掉全部 build_* 编排和 runtime.rs/prompt.rs 散落调用。Phase 1 持续推进中：模型选择已下沉到 core（ModelsConfig::select_for_run），移除 ModelSelector trait 和 CliModelSelector；本轮将 agent/core 彻底合并进 agent/share，share 成为 agent 下所有库的共享依赖层。后续 4 Phase 实施计划见 spec §12。 |
| 49 | AskUserQuestion 增加「All of the above」与「Chat about this...」选项 | 中 | ✅ 已完成 | 未确认 | 已实现：AskUserState 新增 llm_option_count/chat_input_active 字段；ui_event.rs 构建 AskUserState 时追加内建选项（仅 LLM options ≥ 1 时）；ask_user_key.rs Enter 键分支处理 All/Chat/普通选项；Chat about this 进入自由输入子态（Esc 回选项列表，Enter 提交）；Space 禁止在内建选项上切换 multi_select；default guidance 告知 LLM 不要重复定义内建选项文案。内建选项文案使用英文 "All of the above" / "Chat about this..." |
| 50 | CLI TUI 目录整理：收拢碎片、统一模块层级 | 中 | ✅ 已合并 | 已确认 | 已并入 #47。物理目录结构为 UI Domain 的 4 个 Context 提供物理基础。 |
| 51 | UI Domain DDD 设计 —— 将 apps/cli 提升为核心域 | 中 | ✅ 已合并 | 已确认 | 已并入 #47。经讨论回归支撑域（薄入口），AgentClient SDK 保留并纳入 #47 §6.5。 |
| 52 | Tool 描述英文化：所有 tool 给 LLM 的 description 统一为英文 | 中 | 未开始 | 未确认 | 当前 29 个内置 tool 中 27 个 description 已是英文，仅 EnterWorktree / ExitWorktree 两个 tool 的 description 和 input_schema 参数描述为中文。目标：将这两个 tool 的描述统一为英文，同时审查所有 tool 的 input_schema 参数描述是否也有中文残留。MCP tool 的 description 来自 MCP server 透传，不在本 feature 范围内。 |
| 54 | 主动压缩触发：大上下文下防止 LiteLLM 代理拒绝 | 高 | 活动中 | 未确认 | Session 019e4ea6 使用 gpt-5.5 经 LiteLLM 代理（platform.wanaka.fun）时，20+ 轮 tool calling 累积 356 tool_use + 356 tool_result、580+ 条消息、请求体 427KB+，模型返回 "I'm sorry, but I cannot assist with that request." 安全拒绝。需在上下文过大时主动触发 compaction（如消息数 / token 数超过阈值），减小请求体规模，避免触发代理端安全策略。 |
| 55 | TUI 架构收口：render / adapter / app 三层落地 + 清理 legacy core | 中 | 待确认 | 未确认 | feature #53 TUI Model/View 迁移遗留收口。本轮补完：`app/` 承接原 `core` 主实现，`core/` 仅保留弃用兼容 namespace；删除 legacy `core/update`、`core/state`；`model/session` 并入 runtime；output/status/theme/dialog/task_window/syntax/render cache/output view model 渲染实现收口到 `render/`；补齐 `adapter/`、`effect/executor.rs`、`view_state/cache.rs` 与架构 guard。 |
| 56 | 输入单一真相约束：禁止直接改 input_area 并加架构 guard | 中 | 待确认 | 未确认 | 强制"输入 text/cursor 真相只在 model.input.document，input_area 仅 View、由 model 单向派生"。本轮已完成：键盘、AskUserQuestion、粘贴、补全、图片 pending count 等输入修改路径改为 InputIntent → InputModel::apply → adapter/input_widget.rs；app/update 不再读取 input_area text/cursor 作为业务真相；新增 check-tui-input-single-source.sh 并接入架构 guard；InputArea text/cursor 可变方法收紧为内部/测试可见。 |
| 57 | TUI 目录物理收口：并入剩余 widget/service 目录、删 core shim | 低 | 待确认 | 未确认 | #55 后剩余顶层目录已物理收口：删除 `core/` 空 shim；`output_area/`、`input/`、`display/` 并入 `render/`；`completion/` 按架构文档拆入 `model/input/completion`（纯解析/状态）与 `effect/completion`（文件系统候选 IO）；`session/` 拆入 `effect/session`（spawn/load/save/resume 副作用编排），状态继续归 `model/runtime`；新增并接入 `.agents/hooks/check-tui-toplevel-layout.sh`，白名单锁定 TUI 顶层目录为 spec 9 层并禁止旧顶层模块路径回归。 |
| 58 | TUI 输出区渲染管线统一重构 | 高 | 待确认 | 未确认 | 统一为单一 ViewModel→Render 管线，恢复 markdown+theme，消除有损桥/双表示；详见 [plan](../superpowers/plans/2026-05-29-tui-output-render-pipeline.md) 与 [spec](../superpowers/specs/2026-05-29-tui-output-render-pipeline-design.md) |
| 59 | TUI Model/View 单源迁移收口（伞型 roadmap） | 中 | S1-S5 待确认（全部子项完成） | 未确认 | 对账 #53/#55/#56/#57/#58 已有迁移成果与 6 个现存 guard，列出真正未被守护的剩余单源缺口，定义收口顺序。采用单写入者/单向 + guard 范式（非"状态零留 widget"）。子项：S1 OutputArea live tail（spinner+task window 入 Model）（✅ 完成：SpinnerModel{active,phase}+SpinnerPhase 入 RuntimeModel、动画 frame/verb 归 view_state、LiveStatusViewModel+assembler+adapter 单向派生、30 处触发点转 RuntimeIntent、task lines 经 UpdateTaskLines 入 Model、check-tui-spinner-task-single-source.sh guard；详见 [spec](../superpowers/specs/2026-05-29-tui-s1-spinner-task-single-source.md)）、S2 OutputArea 滚动/follow-tail/选区入 Model（✅ 完成：scroll_offset/auto_scroll/选区锚点入 view_state::OutputViewState、adapter/output_view_widget.rs 每帧单向写回 widget 镜像、key_scroll/滚轮/鼠标选区改 view_state、保留 #63 gutter_cols 列补偿+plain 复制、删 widget scroll.rs/死选区方法/mouse_event.rs 脚手架、check-tui-output-scroll-selection-single-source.sh guard；详见 [spec](../superpowers/specs/2026-05-29-tui-s2-output-scroll-selection.md)）、S3 StatusBar 去镜像+单写入者（✅ 完成）、S4 选区统一+mouse_handler 走 intent（✅ 完成：input/status 选区迁入 view_state（InputSelectionViewState/StatusSelectionViewState，对齐 S2 output 范式），三区选区真相均在 view_state、adapter 每帧单向写回 widget 镜像、mouse_handler 三区+跨区清全走 view_state（零 widget 直驱选区）、widget 屏幕坐标→锚点折算保留为只读、删 model.input.document.selection 死桩 + widget 选区状态方法、resize/reset 清三区、check-tui-selection-single-source.sh guard + 收紧 output guard 豁免；详见 [spec](../superpowers/specs/2026-05-29-tui-s4-selection-unify.md)）、S5 Effect 化 tea-purity 豁免名单内副作用（✅ 完成：A 块——reflection/clipboard（前期）+ dialog/suggestions list_models 改预取缓存（cached_models）去 block_on、/save 复用 Effect::SaveSession{notify}、/memory 新增 Effect::FetchMemoryList、/paste 复用 Effect::ReadClipboardImage，相关文件移出 tea-purity EXEMPT；B 块 wontfix——slash.rs 主分发为 request-response + Option<String> 控制流，Effect 化需整管线状态机重写收益低成本高，连同 mod.rs/run_loop/runtime/slash_tests 标合理豁免并在 guard 注释文档化；详见 [spec](../superpowers/specs/2026-05-29-tui-s5-gap-slash-effect.md)）。每子项各走独立 spec→plan→实施并加对应 guard。详见 [roadmap](../superpowers/specs/2026-05-29-tui-single-source-completion-roadmap.md) |
| 60 | Auto-compact LLM 语义化压缩 | 中 | 实现中 | 未确认 | 将 auto-compact 的本地文本摘要（build_summary_text：每消息取前200字+工具名）替换为 LLM 调用（build_compact_request + COMPACT_PROMPT），生成结构化摘要（目标/进度/关键决策/相关文件/当前状态/下一步）。失败时回退到本地摘要。涉及：summary.rs 新增 compact_messages_with_llm（async + Arc<LlmClient>），looping/compact.rs 传参，trait_command.rs 走 LLM 路径 |
| 61 | 架构债务收口（047 DDD 软约束落实） | 中 | 活动中 | 未确认 | 047 DDD spec 的**硬约束**（依赖图/CLI 薄入口/share 无上游/COLA 纯度/forbidden import/行数，6 个 guard）已全部遵守且焊死；guard 不强制的**软约束**仍停留在 spec 已标注的「技术债/未实现」状态，docs 此前无落实计划。子项（按优先级）：**D1** `runtime::api` 去整体转发（现 `pub use audit/hook/policy/.../tools` + `share::*` 整体转发，违 §6.4.3 rule4；改为 use case 门面，最高优先、低风险）；**D2** share 瘦身（移出 `config`/`memory`/`task`/`tool` 等带行为/IO 模块到对应 domain，回归最小内核，§6.4.1 rule6，工作量大）；**D3** supporting domain Public API 收窄（8/10 crate 的 api.rs 仍是 Marker/转发，改 use case 门面，§6.4.3）；**D4** COLA 内部分层 core/business/utils（仅 runtime 完整，9/10 平铺，§6.4.3/§7.2，与 D3 协同）。（原 D5 audit / D6 policy 为新功能实现而非债务收口，已拆出至 #62。）详见 [spec](specs/047-ddd-redesign.md) 与下方详情段。 |
| 62 | audit / policy 域实现（047 DDD 未实现域落地） | 中 | 活动中 | 未确认 | 047 DDD spec §4.2/§4.3/§8/§9 定义但**当前未实现**的两个域（从 #61 拆出，因属新功能而非债务收口）：**audit**——现 `agent/audit` 仅 `AuditApiMarker` 空骨架（14 行），需实现 AuditTrail / correlation id（串联 Session·Chat·Turn·Agent·Tool·Resource）/ policy·hook·outcome 三分记录（§4.2/§8/§9）；**policy/PermissionEngine**——现 `agent/policy` 仅 `security` 内容扫描（149 行），需实现 PermissionRequest/Decision/Grant/Mode(AskMe·Auto·Plan·AllowAll)/Capability/RiskAssessment（§4.3/§9）。**强关联 #42 权限管控系统**（policy 主要设计来源），实施前应先与 #42 spec 对齐，避免重复设计。详见 [spec](specs/047-ddd-redesign.md)、[#42 spec](specs/042-permission-control-system.md)。 |
| 63 | TUI Block 抽象 trait 化 + 真正渲染树（嵌套规则）+ gutter | 高 | 待确认 | 未确认 | 已实现（feature/63-block-trait-nesting，472 测试通过 + clippy + 架构守卫）。①BlockComponent trait 取代自由函数+match 分发，cache_version 统一指纹；②OutputViewModel 升 BlockNode 树，document_renderer 递归 render_tree DFS 展平；③嵌套规则 nesting.rs（allowed_child + MAX_BLOCK_DEPTH=3）+ assembler push_child_checked 构造期校验 + check-tui-block-nesting.sh guard；④tool result 升为独立 OutputBlockKind::ToolResult 子块（render 逻辑从 tool_call 移入 blocks/tool_result.rs，复用 render_edit_diff/render_fenced_markdown），结构隔离 #65（result 自有缓存块，fence 状态不跨块泄漏）；⑤行首 gutter（render/output/gutter.rs）：每行 [depth 缩进 + marker 列]，marker 静态按 kind/status（●/✓/✗、UserMessage `>`、其余空），仅首行画、后续等宽空白，组合期注入、不进 plain；选区列偏移补偿 gutter_cols。删除 OutputViewModel.blocks 旧路径 + render_fenced_markdown indent 参数。视觉变化：所有 block 行首 2 列 gutter，tool result 作 depth-1 子块缩进 4 列。已知遗留：ToolCallBlockView.activity_summary 为死字段（后续清理）。注：#65 仅加固不认领修复；#76 属 #58 域不在范围。详见 [spec](../superpowers/specs/2026-05-29-tui-block-trait-nesting-design.md) 与 [plan](../superpowers/plans/2026-05-29-tui-block-trait-nesting.md) |
| 64 | TUI tool call result 子块展示 output 内容预览 | 中 | ✅ 已完成 | 未确认 | result 子块从纯 `✓ X completed` 摘要改为展示工具实际 output 的前 N 行预览（受各工具 `result_max_lines` 截断，默认 5 行 + `N lines omitted`）。承接 #87/#86：完整内容不刷屏由渲染层 `format_result_lines` 截断 + `bind_tool` 修复保证 id 不丢共同保证。改动：嵌入路径 `result_summary` 改携带实际 `call.result`（删除 assembler 的 `tool_result_summary`）；`format_result_lines` 对 `max_lines==0` 工具（AskUserQuestion/TaskListComplete，答案已 echo）整体不渲染。回归：`output_tests` 嵌入预览断言重写 + `test_render_tool_result_max_lines_zero_renders_nothing` + `state::tests` 端到端 Grep 预览。 |

### #61 架构债务收口（047 DDD 软约束落实）

**状态**：待确认（D1-D4 + Ollama 接线 #85 全部完成，合回 main `aa9f0d3`，`cargo clean` 后 1120 测试绿、clippy --workspace + 架构 guard 含 COLA layer purity 全过）

**背景**：2026-05-29 核对 047 DDD spec vs `agent/` 实现——**硬约束（6 个架构 guard：Cargo 依赖图/CLI 薄入口/share 无上游/COLA 纯度/forbidden import/行数）全部遵守且焊死**，经历 #58/#59 等大量并行开发后仍未破。但 guard 不强制的**软约束**仍停留在 spec 已标注的「技术债/未实现」状态，docs 此前未登记落实计划。本 feature 把这些债务转为可执行子项。

**子项（按优先级 / 风险）**：
1. **D1 `runtime::api` 去整体转发**（§6.4.3 rule4，**最高优先、低风险**）：`agent/runtime/src/api.rs` 当前 `pub use audit/hook/policy/project/prompt/provider/storage/tools` + `pub use share::*` 整体转发下游 crate，违反"api 只暴露 use case/DTO、不暴露内部"。改为只 re-export 业务 use case（chat/session/agent_runner 等）。
2. **D2 share 瘦身**（§6.4.1 rule6，**工作量大**）：`agent/share`（55 文件）含 `config`(19f)/`memory`(9f)/`task`(8f)/`tool`(1f，ToolRegistry/ToolContext) 等带行为/IO 类型，超出"最小共享内核"。移出到对应 domain，保留 message/session_types/error/string_idx。
3. **D3 supporting domain Public API 收窄**（§6.4.3）：project/prompt/storage/policy/hook 的 api.rs 仍是 `XxxApiMarker`/`pub use 内部::*`，tools/provider 无 api facade。收窄为 use case 门面、内部 crate-private。
4. **D4 COLA 内部分层 core/business/utils**（§6.4.3/§7.2）：仅 runtime 完整分层，其余 8 个平铺。与 D3 协同。

> 原 D5（audit 实现）/ D6（policy/PermissionEngine 实现）属**新功能而非债务收口**，已拆出至 **#62**。

**D2 进度 · skill loader 归位（fs IO 收尾）**：`share::skill_ops_loader`（`load_all_skills`/`load_all_skills_cached`/`load_and_filter_skills`/`load_skills_from_dir`，含 `std::fs::read_dir`/`metadata` 目录遍历 IO）已移入 `prompt::skill::loader`（prompt 成为 loader canonical，符合 rule6「prompt 承载 skills」）；`agent/share/src/skill_ops_loader.rs` 删除，`lib.rs` 移除 `pub mod skill_ops_loader`。`Skill` DTO 与单文件 parser（`parse_skill`/`read_skill_content`/`builtin_commit_skill`）**保留** `share::skill_ops`——`Skill` 与 `read_skill_content` 被 `tools`/`runtime`/`prompt` 多 crate 依赖，属合理共享内核契约；保留可避免新增 `tools→prompt` 横向边（tools 仅用 DTO+parser，不用 loader）。`prompt::skill` re-export DTO/parser 保持调用方接口一致；runtime 经 `prompt::skill::load_all_skills` 调用（`runtime→prompt` 已批准边，无新增边、无环）。loader 单测随迁至 `prompt::skill::loader::loader_tests`，行为不变。

**D4 进度 · supporting domain 内部 COLA 分层（待确认）**：7 个 supporting domain 已按 §6.4.3/§7.2 完成内部分层，api.rs 门面对外路径不变，纯目录/模块组织无业务逻辑改动。务实原则——只建实际有职责的层，不为凑满三层建空目录：
- **policy** → `business/security`（仅内容安全扫描一个领域模块）。
- **project** → `business/worktree`（仅 worktree 工作区上下文一个模块）。
- **storage** → `business/{memory,history,tool_result_storage}`（三者均为持久化领域规则；history 的 dead_code allow 随迁）。
- **hook** → `business/hook`（整体为单一 hook 执行引擎 data/result/runner/events；`crate::hook::` → `crate::business::hook::`）。
- **prompt** → `business/{guidance,skill,security}`（提示词领域规则；guidance/resolver 内 `crate::security` 同步更新）。
- **tools** → 三层：`core/registry`（register_all_tools 等注册编排，从 lib.rs 抽出）+ `business/*`（各 Tool 实现）+ `utils/path_security`（路径校验纯辅助）；内部 `crate::{bash,mcp,mcp_manager,path_security}` 改分层路径。
- **provider** → `core/{provider(端口),client,pool}` + `business/{providers,stream,types}`；内部 6 个模块引用全部改分层路径。
- **audit**（14 行 marker 骨架，无内部模块）：无可分层内容，免分层。
commit：policy `01d45e6`、project `319c4f0`、storage `8157219`、hook `fa17810`、prompt `bdb8187`、tools `adb67d4`、provider `773afe0`。验证：`cargo clean && cargo test --workspace` 1120 passed / 0 failed；`cargo clippy --workspace -- -D warnings` 通过；`check-architecture-guards.sh`（含 COLA layer purity）全绿。

**性质**：纯架构债务收口（D1-D4），不改业务行为。各子项 SHOULD 独立走 spec→plan→实施；D1/D2/D3 完成后 SHOULD 补对应 guard（如 api 收窄 guard）防回归。

**遗留技术债（#61 后续待结清，2026-05-30 登记）**：D1-D4 主体收口完成，下列由收口过程暴露/引入的债务待逐项结清：
1. **补 cross-crate api-boundary guard（已完成，2026-05-30）**：参考 wanaka core-api 的 `no-feature-cross-import`/`no-cross-feature-internals`——**domain 间横向依赖是合理的，只要经对方 published 门面（aemeath 的 `<crate>::api`，对应 core-api 的 `context/`），禁 reach into 内部**。`tools→{project,storage}` 已经经 `project::api`/`storage::api` 门面（合规），**本身非债务**——原「端口注入 / share re-export」判断系**过度设计，撤销**（share re-export 上游还会违反 share-no-upstream guard）。本轮新增 `.agents/hooks/check-crate-api-boundary.sh` 并接入 `check-architecture-guards.sh`：扫描 `agent/`/`apps/`/`packages/` Rust 源码，跨 domain crate 访问只允许 `<crate>::api::*`，阻断 `<crate>::{business,core,utils}` 等内部路径；`share` 作为共享内核例外，`provider::{ApiDriverKind,LlmError}` 作为既有 root public type 兼容例外；脚本内置 sanity 覆盖允许/禁止样例，架构 guard 已全绿。
2. **share 回归最小共享内核（「收口 pub」根本解，由 #1 认知修正解锁）**：既然 domain 间经 api 门面横向依赖合理，就**不必为「共享」把带行为/状态类型塞 share**——它们可回归各自 domain，消费者经 `<crate>::api` 横向依赖。**移出**（带行为/状态；移走后 share 不再需 `tokio`/`futures` 等，根除 `share_warn_io`）：`tool::ToolRegistry`→tools、`task::TaskStore`→storage/runtime。**保留纯 kernel**（协议无关 value object + 契约 trait）：`error`/`message`/`session_types`/`string_idx`/`tool::{Tool,AgentRunner}` trait + `ToolContext`/`ToolResult`/`Task`/`TaskSnapshot`/`MemoryEntry`/`Skill` DTO/`config`（纯 DTO 无 IO）。这是 D2 share 瘦身的**彻底版**（D2 仅移 fs IO 实现 worktree/MemoryStore/skill loader）。判断点：`Tool` trait `async fn call` 引入 `async-trait`（非 tokio，可接受）。**命名决策（已定，2026-05-30）**：瘦身后 share 名实相符地回归 spec 原 `core`「最小共享内核」语义；但因 Rust 内置 `core` crate（no_std）冲突（`use core::` 歧义，疑为 P11 用 share 而非 core 之由），**决定保持 `share` 命名不变**（瘦身后「最小共享内核」语义成立，零改名成本）。047 spec 维持 `share` 命名（2026-05-29 的 core→share 校订继续有效，无需反向改名）。
3. **D3/D4 暴露的 `#[allow(dead_code)]` 孤儿**（逐个确认接线遗漏 vs 移除）：Ollama builder（`with_max_retries`/`with_timeout_secs` 无配置来源，refs #85）、`storage::business::history` 整模块、MCP 资源 Tool（`ListMcpResourcesTool`/`ReadMcpResourceTool` 未注册）、prompt `load_all_skills_cached`/`load_and_filter_skills`、Anthropic builder、`StreamEvent`/`DeltaPayload` 反序列化字段。
4. **provider `core↔business` 双向引用**（D4 暴露）：`core/client` ↔ `business/providers` 模块级循环，COLA「core 不依赖 business」方向性被破；后续解耦 client↔providers。
5. **runtime 内部 100+ 处经 `crate::api::<crate>::` 引用下游**（D1 发现）：runtime 把自身对下游 crate 的引用路由经自己的 `api`，属架构异味；后续改直接 `use <crate>::` 或评估接受为现状。
6. **补回归 guard**：D1/D3 SHOULD 补「api 收窄」guard（防 supporting domain 内部模块重新 `pub` / `runtime::api` 重新整体转发），D2 SHOULD 补「share 禁含 fs/process IO」AST guard 防回归。

> 教训记录：`cargo test --workspace` 的**增量缓存会制造 unresolved import 假错**，多次误导本次收口判断——涉及可见性/import 改动的验证必须 `cargo clean` 后重跑才是真相。

**关联**：feature #47（DDD spec 基线）；#62（audit/policy 域实现，原 D5/D6）；#85（Ollama 接线，已修）。

### #59 S3 · StatusBar 去镜像 + 单写入者

**状态**：待确认

**问题**：`StatusBar` widget 自持 `input_tokens/output_tokens/last_input_tokens/session_id/api_calls/model/context_size/tps/context` 等字段，全是 `RuntimeModel`/`SessionModel` 的镜像；存在双写入者——`adapter/status_widget.rs` 从 Model 拷入的同时，`app/update/ui_event.rs`、`effect/session/{resume,session_lifecycle}.rs`、`app/slash.rs` 又直接 `status_bar.set_tps/set_tokens/set_model/set_session_id/set_context_paths/set_git_context(...)`，分歧风险；`view_assembler/status.rs` 已有 assembler 但闲置，`StatusBar::render` 读自身镜像字段而非 ViewModel。

**解决思路**：
1. 新增 `StatusRuntimeViewModel`/`StatusContextViewModel`（`view_model/status.rs`，自带视图层 `StatusWorktreeKind` 避免依赖 model 内部类型），`StatusBar` 把 model/session/tps + 工作目录上下文镜像收敛到内嵌 `vm`，`render` 改读 `vm`。
2. 启用闲置 assembler：新增 `StatusViewAssembler::assemble_runtime_view(runtime, session)` 单向派生 ViewModel；adapter `apply_runtime_status_to_widget` 经唯一写入口 `StatusBar::apply_runtime_view` 落地 widget。
3. 删除 `ui_event.rs`（Usage/LiveTps/ReflectionUsage/WorkingDirectoryChanged）、`resume.rs`、`session_lifecycle.rs`、`slash.rs` 对镜像 `set_*` 的直写；`UiEvent::Usage` 的 elapsed→tps 改在 `map_agent_event` 注入 `RuntimeIntent::RecordLiveTps`；resume/lifecycle/model-switch 改为 apply `SessionIntent::SetCurrentSession`/`RuntimeIntent::{WorkspaceSnapshotReceived,SetProviderModel}` 后调 adapter。
4. 镜像 `set_*` 收紧为 `pub(crate)`；新增 `.agents/hooks/check-tui-status-single-source.sh` 禁止 adapter 之外出现 `status_bar.set_model/set_session_id/set_tps/set_tokens/set_context_paths/set_git_context`，接入 `check-architecture-guards.sh`。

**边界**：token/api 总量真相仍在 `ChatState`（渲染期由 `StatusBar::draw` 从快照写回，作为 widget 渲染缓存）；`context_size`/`permission_mode` 为启动期一次性配置；status 文案（`set_success`/`set_warning`）、`set_thinking`、selection 不在本子项范围（选区属 S4），行为保持不变。

**TDD/验证**：assembler 三路径单测（正常/空 branch 边界/缺 model+session 错误）+ adapter 三测；443 cli 测试全绿；`cargo build/clippy -p cli -D warnings` 全绿；全部架构 guard（含新增）通过。

### #59 S5 · Effect 化 tea-purity 豁免名单内副作用

**状态**：待确认

**问题**：`check-tui-tea-purity.sh` 的 EXEMPT 名单豁免了一批仍在 `app/` 内联副作用的文件（reflection 的 `tokio::spawn`、util 的 `clipboard::copy_text` 等），属已知技术债，违反 TEA「update 纯 + 副作用经 Effect/executor」约束。

**解决思路**：
1. **reflection** — `slash/reflection.rs` 的 `/reflect` 与自动 reflection 不再内联 `tokio::spawn`：新增 `Effect::RunReflection { foreground }` / `Effect::ApplyReflection { output }`，由 `effect/executor` 后台 spawn 调用 SDK，结果经既有 `UiEvent`（ReflectionStarted/Usage/Done/Error）回流。`handle_reflect_command`/`maybe_auto_reflect`/`apply_reflection_output` 改为返回 Effect；`update/done.rs`、`update/ui_event.rs` 收集进 `effects` 向量。reflection.rs 现为纯函数。
2. **clipboard** — `util.rs` 的 `copy_to_clipboard`/`copy_selection_to_clipboard` 不再调用 `clipboard::copy_text`，改为返回 `Effect::CopyToClipboard { text }`；实际剪贴板写入与 status bar 成功/失败反馈搬到 `effect/executor`。`render/input/mouse_handler.rs::handle_mouse_event` 改为返回 `Vec<Effect>`，经 `update` 的 Mouse/TerminalMouse 分支收口由 runtime 执行。
3. **名单清理** — 移除 `slash/reflection.rs`、`util.rs`（已 Effect 化）；移除无任何副作用的 `slash/help.rs`、`slash/help_display.rs`；移除两条失效条目 `session/processing.rs`、`session/session_lifecycle.rs`（真实文件在 `effect/session/` 下，不在 tea-purity 受检目录）。

**剩余 gap（仍豁免，需后续子项）**：
- `slash.rs`（15 处 `.await`）、`slash/save.rs`、`slash/memory.rs`（各 2 处 `.await`）：均为 slash 命令异步分发层，深度耦合 `handle_slash_command_with_events` 的 async 调度，Effect 化需重塑整条异步派发管线（含 `[session saved]` 等同步反馈语义），改动面大，本批未动。
- `slash/dialog.rs`、`slash/suggestions.rs`（各 1 处 `block_in_place`+`block_on(list_models)`）：需要**同步**模型列表来即时构建对话框/补全候选；改异步会改变「打开即有列表」的交互语义，需要专门的 ListModels 预取/缓存设计，本批保留。
- `mod.rs`（`Command::new("git")`）、`run_loop.rs`、`runtime.rs`（runtime 编排层本身需要 `.await`）、`slash_tests.rs`（测试 mock 的 `.await`）：属 runtime/测试边界，合理豁免。

**边界**：不碰 S3 StatusBar；纯把副作用搬到 Effect，不引入新业务行为——`/reflect`、自动 reflection、`/reflect apply`、鼠标选区复制行为均保持原样。

**TDD/验证**：reflection 6 测（含 `handle_reflect_command` 返回 Effect、auto-reflect 触发返回 Effect 并经 `execute_effect` spawn、4 个不触发边界）+ util 6 测（copy_to_clipboard/copy_selection 返回 Effect、None 路径、find_token_end 三路径）全绿；452 cli 测试全绿；`cargo build/clippy -p cli -D warnings` 全绿；`check-architecture-guards.sh`（含缩小后的 tea-purity）全过。

### #58 Phase 5 · T1：迁移 push_system/push_error 到单一真相源

**状态**：进行中（T1 完成，等待后续 Task 删除 lines 字段/legacy 垫片）

**问题**：输出区主渲染已切换到 `ConversationModel → OutputViewModel → OutputDocumentRenderer → RenderedDocument`，但 `OutputArea::push_system`/`push_error` 仍直接写 `output_area.lines`，经 `sync_document_from_legacy_lines` 垫片兜底——构成「双表示」残留。且 `sync_document_from_legacy_lines` 仅在 `document` 为空时生效，一旦对话开始（document 非空），这些 push 的消息将不再显示（潜在显示丢失）。

**改法**：
- 新增 `App::append_system_notice / append_error_notice`：派发 `AppendSystemMessage/AppendError` intent 后 `refresh_output_widget_from_model`，作为系统/错误消息的唯一注入入口。
- 判别 12+ 处调用点：`UiEvent::Error/SystemMessage/ReminderRecap` 经 `map_agent_event` 已注入 ConversationModel → 属冗余双写，直接删除；其余 slash/effect/paste/key/resume 等为唯一显示路径 → 迁移到新入口。
- 新增 `ConversationModel::reset()`，并在 `App::reset_runtime_state` 中清空对话单一真相源 + 刷新文档，修复 `/clear` 后旧 block 残留。
- `push_error` 迁移后无调用者，删除其定义；`push_system` 仅保留给 `OutputArea::init()` 欢迎横幅（lines 字段与 legacy 垫片保留，留待后续 Task）。

**验证**：`cargo build/test(-p cli 374 passed)/clippy(-D warnings)` 与 `check-architecture-guards.sh` 全绿。

### #58 Phase 5 · T2：迁移 push_user_message 到单一真相源

**状态**：进行中（T2 完成，lines 字段/legacy 垫片仍待后续 Task 删除）

**问题**：用户输入回显仍由 `OutputArea::push_user_message` 直接写 `output_area.lines`，构成与 `ConversationModel` 的「双写/双表示」。document 非空后该路径不再显示，存在丢失风险。

**改法**：
- 新增 `ConversationIntent::AppendUserMessage` + `ConversationChange::UserMessageAppended` + `ConversationModel::append_user_message`：仅追加 `UserMessage` 回显块，**不新开 chat/turn、不改 active_chat_id**（区别于 `StartChat`），用于 ask_user 应答、队列输入冲刷等「已激活回合内回显」场景，避免破坏在途工具绑定。
- 新增 `App::append_user_echo`（notice.rs）作为回显唯一入口（派发 intent 后 `refresh_output_widget_from_model`）。
- 判别 4 处 `push_user_message` 调用点：
  - `ask_user_key.rs`×3（选项应答 / 自由输入 / Chat-about-this 子模式）— 唯一显示路径，迁移到 `append_user_echo`。
  - `ui_event.rs` 队列输入冲刷 — 唯一显示路径，迁移到 `append_user_echo`。
  - `run_loop.rs` review-prompt — 新建对话语义，改派 `StartChat` + `refresh`（与 `update_enter` 一致）。
- 迁移后 `push_user_message` 无调用者，删除其定义（`lines` 字段与 legacy 垫片保留，留待后续 Task）。
- 回显渲染确认：`UserMessage` block → `OutputBlockKind::UserMessage` → `render_user_message` 输出 `> ...` 前缀（已有单测覆盖）。

**验证**：`cargo build / test(-p cli 380 passed) / clippy(-D warnings)` 与 `check-architecture-guards.sh` 全绿。

### #58 Phase 5 · T3：清理 tool_display 旧命令式渲染

**状态**：进行中（T3 完成，lines 字段/legacy 垫片仍待后续 Task 删除）

**背景**：ToolCall 现经单一管线 `ConversationModel → OutputViewModel → OutputDocumentRenderer`，由 `render/output/blocks/tool_call.rs`（`OutputBlockKind::ToolCall`）渲染。`render/output/tool_display/` 中产出 `OutputLine` 的旧 OutputArea 命令式渲染路径已无活跃使用，本轮清理。

**判别结论**：
- 命令式显示方法 `push_tool_call_start` / `update_tool_call_pending` / `push_tool_call` / `format_todowrite`（mod.rs）、`push_tool_result_with_diff`（results.rs）均 `#[allow(dead_code)]`、仅被模块自身单测引用，**死代码 → 删**。
- `agent.rs` 的 `push_agent_progress` / `push_tool_progress` 仍被 `app/update/ui_event.rs` 调用（sub-agent 进度尚未迁移到 ConversationModel），**保留**。
- `tool_call.rs` 仅复用 `tool_display::format_tool_call`（纯格式化 header+detail），不依赖任何命令式渲染，删除死代码后保持复用。

**删除**：
- `tool_display/results.rs`（整文件死路径）。
- `mod.rs` 内 `impl OutputArea` 命令式块 + `debug_log` + 旧 `mod tests`。
- `common.rs` 内 `extract_tool_preview` / `extract_string_value`（仅服务于已删的 pending 预览）。
- `output_area/mod.rs` 中失去使用者的 `build_diff_lines` re-export。

**保留**：`ToolDisplay` trait（`result_*` / `format_result_summary` 默认方法标 `#[allow(dead_code)]` 供未来结果渲染迁移复用）、各工具 `ToolDisplay` impl（经 `format_tool_call` 使用）、`agent.rs` 全部、`common.rs` 其余纯格式化函数。

**边界遵守**：未删 `OutputLine` / `LineStyle` 类型、`lines` 字段与 legacy 垫片、streaming/queued/旧行级链。

**TDD/验证**：为 `tool_call.rs` 补充渲染测试（参数 detail 行来自 `format_tool_call`、result_summary 结果行），确保删除旧路径后行为不回退。`cargo build / test(-p cli 378 passed) / clippy(-D warnings)` 与 `check-architecture-guards.sh` 全绿。


### #58 Phase 5 · T4：删除 streaming 旧重渲与 legacy 排队机制

**状态**：进行中（T4 完成，`lines` 字段/`OutputLine`/`LineStyle`/legacy 垫片仍待后续 Task 删除）

**判别结论**：
- streaming：流式文本现由 `ConversationModel::append_assistant_text/append_thinking_text`（`ObserveAssistantText`/`ObserveThinkingText` intent，经 `adapter/agent_event.rs`）驱动 active block 渲染。`OutputArea` 的 `do_rerender`/`append_assistant_text`/`append_thinking_text`/`has_unclosed_think` 全标 `#[allow(dead_code)]`、无活跃调用者，**死代码 → 删**。
- queued：排队显示已由 `ConversationBlock::QueuedUserMessage → OutputBlockKind::QueuedSubmission`（`view_assembler/output.rs` + `blocks/queued_submission.rs`）经文档管线渲染；`render.rs` 调用 `append_status_lines` 时 `queued_lines` 恒传空向量，`build_queued_message_lines` 无活跃调用者，**死代码 → 删**。legacy `queued_messages` 仅作 `InputState::input_queue` 的镜像用于 drain 回显，**统一到 input_queue 后删除**。

**删除**：
- 文件 `output_area/streaming.rs`、`output_area/queued.rs`（整文件死路径）；`mod.rs` 中对应 `mod streaming;`/`mod queued;` 声明。
- `OutputArea` 字段 `streaming_buffer`/`streaming_start`/`synthetic_think_open`/`queued_line_count`/`queued_messages`，及 `content.rs`（`insert_lines_at` 的 `streaming_start` 调整、`reset_runtime_state` 清理）、`new()` 初始化中的对应项。
- `finish_streaming` 方法及其 no-op 调用点（`done.rs`、`agent.rs::push_agent_tool_calls/push_tool_progress`、`content.rs::push_cancelled/push_ask_user/push_system`）。
- `append_status_lines` 恒空的 `queued_lines` 参数。
- 队列回显统一：`key.rs` 不再 push `queued_messages`；`ui_event.rs::DrainQueuedInput` 直接回显已 `drain` 的 `queued`；诊断日志 `queued_messages_len` 改为 `queued_submissions_len`（取自 ConversationModel）。

**保留**：`OutputLine`/`LineStyle` 类型本体、`lines` 字段、legacy 垫片 `sync_document_from_legacy_lines`/`legacy_lines_to_rendered`、旧行级链 cache/line/block/span（后续 Task）；`agent.rs` 仍写 `lines` 的工具进度路径（sub-agent 进度尚未迁移）。

**TDD/验证**：复用已有 streaming 回归（`model_tests.rs::test_conversation_streams_text_and_thinking_into_blocks`）与 QueuedSubmission 渲染测试（`blocks/queued_submission.rs`）；新增 `model_tests.rs` 队列 block 回归（QueueSubmission 落 `QueuedUserMessage` 块、ClearQueuedSubmissions 清空、空队列清理 noop）。`cargo build / test(-p cli 381 passed) / clippy(-D warnings)` 与 `check-architecture-guards.sh` 全绿。


### #58 Phase 5 · T5：删除旧「行级渲染链」及其缓存

**状态**：进行中（T5 完成，`lines` 字段/`OutputLine`/`LineStyle`/legacy 垫片仍待 T6 删除）

**判别结论**：
- 主渲染（`output_area/render.rs`）只画 `self.document.iter_lines()` + `apply_selection_overlay`，**不再读** line_cache：`ensure_rendered` 无任何活跃调用，`RenderedCache.get` 无调用者。`render.rs`/`content.rs`/`resize.rs` 中所有 `line_cache.invalidate/content_changed/mark_clean` 都是「维护一个不被读的缓存」→ 死维护，随链删除。
- `ViewRenderCache` 的 `dirty_blocks`/`mark_output_dirty`/`mark_block_dirty`/`clear_dirty_blocks` 仅被自身单测引用，无生产调用者；`OutputRenderCacheState.line_cache` 同为死维护。故 `render_with_cache(area, buf, cache)` 的 `cache` 参数整体为死代码 → 删除参数，方法重命名为 `render(area, buf)`。

**删除**：
- 文件 `render/output/{line.rs,block.rs,span.rs,cache.rs,block_tests.rs}`（旧行级链 `render_range`/`collect_table_ranges`/`scan_code_blocks`/`scan_table_blocks`/`CodeBlockInfo`/`slice_spans`/旧 `RenderedCache`/旧 `RenderedLine{line,screen_entries,rendered_text}`）。
- 文件 `view_state/cache.rs`（`OutputRenderCacheState`、`ViewRenderCache`）；`view_state/mod.rs` 的 `pub mod cache`/`pub use ViewRenderCache` 及 `AppViewState.cache` 字段。
- `OutputArea.rendered_cache` 字段及 `content.rs`（4 处）/`resize.rs`（1 处）对 `line_cache` 的维护调用；`mod.rs` 的 legacy `draw()`（仅转调旧 `render`）；`render.rs` 的 legacy `render()` 方法（仅 swap line_cache）与 `wrap_output_line`（仅旧链使用）。
- `render_with_cache` → `render`，去掉 `cache` 参数；`app/mod.rs` 调用点同步；`output_area/mod.rs` 去掉随之失效的 `markdown` 顶层 re-export。
- `output/mod.rs` 删 `pub use cache::{RenderedCache, RenderedLine}` 与 `pub mod {line,block,span,cache}`、`mod block_tests`。

**RenderedLine 提升**：`output/mod.rs` 新增 `pub use rendered::{RenderCtx, RenderedBlock, RenderedDocument, RenderedLine}`；全仓对 `RenderedLine` 的引用均指向新 `rendered::RenderedLine{spans, plain}`，无命名歧义。

**保留（留 T6）**：`OutputLine`/`LineStyle` 类型本体、`lines` 字段、legacy 垫片 `sync_document_from_legacy_lines`/`legacy_lines_to_rendered`；`document` 为空时仍经垫片渲染。

**TDD/验证**：保留 document 渲染回归 `render.rs::test_render_document_paints_spans_and_overlays_selection`（改用无 cache 参数的 `render`）；`resize.rs` 两个断言旧 cache dirty 的测试改为断言 `term_width` 行为（缓存已删，行为不变由 document 渲染覆盖）。`cargo build -p cli`、`cargo test -p cli`（362 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh` 全绿。

### #58 Phase 5 · T6a：迁移 agent_progress / cancelled / init 横幅到 ConversationModel

**状态**：进行中（T6a 完成，AskUser 命令式路径留 T6b、`lines` 镜像/`OutputLine`/`LineStyle`/legacy 类型留 T6c）

**判别结论**：三类内容此前由 `update/ui_event.rs` 命令式直写 `output_area.lines`，但 `update_agent_event` 在 `update_ui` 之后立即 `refresh_output_widget_from_model()`（内部 `lines.clear()` 重建），这些写入当帧即被镜像清空 → 死写。其中 agent_progress / SystemMessage / Cancelled 均已由 `adapter/agent_event.rs::map_agent_event` 映射为对应 intent 并注入 ConversationModel。

**改法**：
- **agent_progress**：复用既有 `RecordAgentProgress` intent / `AgentProgress` block（已映射 DiagnosticNotice(Running)）。`ui_event.rs` 删除命令式 `push_agent_progress` 调用，仅保留 spinner。删除 `render/output/tool_display/agent.rs`（`push_agent_progress`/`push_agent_tool_calls`/`push_tool_progress`/`tool_insert_position`）及其 `agent_tests.rs`、`common.rs` 中仅服务它的 `format_agent_tool_calls`/`format_tool_group`。
- **cancelled**：`map_agent_event` 将 `Cancelled` 映射为 `CompleteChat`（不产生可见提示）。`ui_event.rs` 把 `push_cancelled()` 改为 `append_system_notice("已取消")`，经 System block 渲染。删除 `OutputArea::push_cancelled`。
- **init 横幅**：新增 `ConversationModel::seed_banner()`，在 `App::new()` 注入 4 行 System block（横幅纳入单一真相源，`/clear` reset 会清除，已确认接受）。删除 `OutputArea::init()` 及无调用者的 `OutputArea::push_system`。`run_loop` 增加首帧 `draw + refresh_output_widget_from_model`，让横幅按真实宽度渲染。

**model.rs 拆分**：因 model.rs 已 398/400 行，先把 `append_system_message`/`append_error` 迁入新 `model/conversation/notice.rs`（含 `seed_banner` 与 `BANNER_LINES` 常量），model.rs 降至 373 行、notice.rs 120 行。

**TDD/验证**：notice.rs 新增 seed_banner / append_system_message / append_error 单测；修正 `state/tests.rs`（用户消息不再是首行，改 `.any` 断言并校验横幅）与 `update/reminder.rs`（System block 计数加上 `BANNER_LINES.len()` 偏移）。`cargo build -p cli`、`cargo test -p cli`（362 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh` 全绿。

### #58 Phase 5 · T6b：迁移 AskUser 选项交互到 ConversationModel 单一真相

**状态**：进行中（T6b 完成，`lines` 镜像/`OutputLine`/`LineStyle`/legacy 类型留 T6c）

**判别结论**：AskUser 选项渲染是最后一条命令式直写 `output_area.lines` 的路径。旧实现 `ui_event.rs` 调 `OutputArea::push_ask_user` 渲染「问题 + 选项列表」，靠 `ask_user_block_start`/`option_line_start`/`option_line_ranges` 记坐标，选项导航（↑↓/Space）靠 `update_ask_user_options` 原地改写对应行高亮，提交后 `dismiss_ask_user_block` 按起始坐标回卷 `lines`。该路径绕过 ViewModel→document 管线，是删 `OutputLine`/`lines`（T6c）的最后障碍，也与 bug #63「options 选择同步」「渲染缓存随 selected 失效」相关。

**selected 归属决定**：选项导航的可变状态（`cursor`/`selected`/`chat_input_active`）此前存于 `input.ask_user_state`，与 `reply_tx`/键处理深度耦合。本次将其**迁入 ConversationModel 的 `AskUser` 块**作为渲染与导航高亮的单一真相；`AskUserState` 只保留应答回传所需的静态元数据（`reply_tx`/`options`/`llm_option_count`/`multi_select`/`allow_free_input`）。键处理通过 `ConversationModel::ask_user_snapshot()` 读回 cursor/selected/chat_input_active，导航/勾选/子态切换均派发 intent 更新块。避免双真相：渲染与提交都从块读取同一份状态。

**改法**：
- **model 层**：`ConversationBlock::AskUser { id, question, options, llm_option_count, multi_select, cursor, selected, chat_input_active }`（固定 id `ask-user`，同一时刻至多一个）。新增 intents `ShowAskUser`/`SetAskUserCursor`/`ToggleAskUserSelected`/`SetAskUserChatInput`/`DismissAskUser` 与对应 changes；逻辑落在新 `model/conversation/ask_user.rs`（含 `ask_user_snapshot()` 与 cursor 夹取、内建选项不可勾选校验），避免 model.rs 超 400 行（model.rs 388 行）。
- **view 层**：`OutputBlockKind::AskUser(AskUserBlockView)`；`view_assembler` 映射块字段；新增 `render/output/blocks/ask_user.rs` 渲染问题 + 操作提示 + 选项列表，按 `cursor`（❯/反白）与 `selected`（multi_select 勾选框 [✓]）高亮当前项，`chat_input_active` 子态下不高亮。高亮属「选项导航高亮」，与文本选区 overlay 无关。
- **controller 层**：`notice.rs` 新增 App 端 `show_ask_user_block`/`set_ask_user_cursor`/`toggle_ask_user_selected`/`set_ask_user_chat_input`/`dismiss_ask_user_block`（派发 intent + `refresh_output_widget_from_model`，无 IO/spawn）。`ui_event.rs::AskUser` 改为 `show_ask_user_block`；`ask_user_key.rs` 导航/勾选/子态改派发 intent，提交读 snapshot。
- **删除项**：`OutputArea::push_ask_user`、`update_ask_user_options`、`dismiss_ask_user_block`、`format_ask_user_option_lines`、`ask_user_block_start` 字段、`update/ask_user_options.rs`（`build_option_line_ranges`）整模块、`AskUserState` 的 `cursor`/`selected`/`option_line_ranges`/`chat_input_active` 字段、`output_area/content_tests.rs`（仅测已删函数，回归改由 `blocks/ask_user.rs` 单测覆盖）。

**行为兼容**：↑↓ 导航、Space 勾选（内建项不可勾）、Enter 提交（单选/多选/「All of the above」结构化列表/「Chat about this...」进自由输入子态）、Esc 取消、无选项自由输入模式均保持原行为；内建选项（#49）与 selected→高亮同步（#63）由块单一真相天然保证。

**TDD/验证**：model 层 8 个单测（show 替换、cursor 越界夹取、无块 noop、toggle LLM/内建、dismiss、chat_input、snapshot）；render 层 6 个单测（cursor 高亮、空选项提示、单选项标记、多选勾选框、chat 子态抑制高亮、多行选项缩进）。`cargo build -p cli`、`cargo test -p cli`（372 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh` 全绿。

### #58 Phase 5 · T6c：删除 OutputLine/LineStyle/lines 字段/legacy 垫片/镜像

**状态**：待确认（T6c 完成，输出区只剩 `document: RenderedDocument` 单一表示）

**判别结论**：T6b 后所有命令式写 `OutputArea.lines` 的业务路径已迁完，`lines` 仅剩两类残留：①`adapter/output_widget.rs::render_document_from_view_model` 每帧把 document 各行 plain 镜像写回 lines；②少数逻辑仍读 lines（scroll/resize 算 max_offset、selection 虚拟行映射、status_line task 基址、render 兜底垫片）。`OutputLine`/`LineStyle` 另有两类非 legacy 使用：`blocks/tool_call.rs` 与 `output/diff.rs` 把它们当作行的着色中间单元；`tool_display` trait 的 `detail_style`/`result_style` 是从未被调用的死代码。

**改法**：
- **删镜像**：`render_document_from_view_model` 去掉 `lines.clear()` + 写 OutputLine 循环，只 `set_document` + clamp scroll。
- **读 lines 改读 document**：`scroll.rs`（`scroll_up` max_offset、`line_count`、`get_visible_range`）、`resize.rs`（max_offset）、`selection.rs`（`total_virtual_line_count`、`get_line_content` 虚拟行基址）、`status_line.rs`（task 基址）全部改读 `document.total_lines()`/`iter_lines()`。
- **删 legacy 垫片**：`render.rs` 的 `sync_document_from_legacy_lines`、`legacy_lines_to_rendered` 及调用；`render.rs::render` 不再调 `color_tool_call_dots`（dot 着色已由 `blocks/tool_call.rs` 的 `semantic_color` 在 document 内完成），`status_line.rs::color_tool_call_dots`/`set_running_tool_dot` 整体删除。
- **迁 push_done**：`done.rs` 原经 `OutputArea::push_done` 写 lines 的「✻ Sautéed for Xs」完成提示改派发 `append_system_notice`（ConversationModel → document），并把动词/耗时格式化抽为纯函数 `done_notice`（3 个单测）。
- **删字段/方法/类型**：`OutputArea.lines` 字段及 `new()` 初始化；`content.rs` 的 `insert_lines_at`（无调用者）、`push_line`、`push_done`（`clear()` 改清 document）；`tool_display` trait 的 `detail_style`/`result_style` 及各 impl 重写；`output_area/types.rs` 的 `OutputLine`、`LineStyle` 类型本体与 `mod.rs` re-export。
- **重写非 legacy 着色中间单元**：`output/diff.rs::build_diff_lines` 由产出 `Vec<OutputLine>` 改为 `Vec<Vec<SpanPart>>`（style 字段本就被 spans 覆盖、未实际使用）；`primitives/diff.rs` 适配；`blocks/tool_call.rs::format_result_lines` 直接产出 `RenderedLine`（System/Normal 两种语义色映射 `theme::TEXT_DIM`/`theme::TEXT`）。

**SpanPart 保留**：`SpanPart` 是 `render/syntax.rs` 与 `output/diff.rs` 着色原语的中间单元（`primitives/convert.rs::spanparts_to_spans` 消费），与 OutputLine/LineStyle 解耦，定义留在 `output_area/types.rs`，re-export 保留。

**行为兼容**：滚动 max_offset、resize 夹取、选区（含 markdown 表格/内联格式 plain 偏移）、复制、task_status 虚拟行选择均保持原行为；markdown 选区回归测试改由真实 `blocks/assistant_message::render_assistant_message` 渲染 document 后断言（取代旧 legacy_lines_to_rendered 路径）。tool-call dot 闪烁（旧 `color_tool_call_dots` 在镜像回写 lines 时 style 恒为 `LineStyle::Normal`，而 dots 的 match 仅对 `ToolCallRunning`/`Success`/`Error` 着色，故对 Normal 行恒返回 None、实际早已不生效）随之移除，dot 颜色由 document 静态语义色提供。

**TDD/验证**：`output/diff.rs` 4 个单测改断言 SpanPart 文本/标记；`selection_tests.rs` 11 个用例改用 document 填充辅助（plain + assistant_message markdown）；`resize.rs`/`status_line.rs`/`app/state/tests.rs`/`output_widget.rs` 测试改读 document；`done.rs` 新增 3 个 `done_notice` 单测。`cargo build -p cli`、`cargo test -p cli`（374 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh`（含 ≤400 行）全绿；`OutputLine`/`LineStyle` 全仓零残留。

### #58 Phase 6 · T7：渲染隔离守卫 + 删除门禁 + 回归收尾

**状态**：待确认（Phase 5 删除全部完成、Phase 6 守卫落地，仍有后续接线 gap，故 #58 整体维持「待确认」而非「完成」）

**渲染隔离守卫**：新增 `.agents/hooks/check-render-isolation.sh` 并接入 `check-architecture-guards.sh`，扫描 `apps/cli/src/tui/render/output/`：①禁止 `use crate::tui::model::`（render 层不得依赖 Model 可变类型，引用 `view_model::` 允许）；②禁止 IO（`std::fs::`/`std::process::`/`tokio::`，注释与 `#[cfg(test)]` 测试代码豁免）；③选区上色唯一路径——除 `selection_overlay.rs` 外不得出现 `SELECTION_BG`（防 #61/#62 旁路上色回归，测试断言行豁免）。注入临时违规验证三类规则均能命中（RC=1），现状全绿。

**删除验证门禁**：`grep -rE 'OutputLine\b|LineStyle\b|replace_lines_from_view_model|output_view_model_lines|build_queued_message_lines|queued_messages|queued_line_count|render_range|slice_spans|RenderedCache|do_rerender' apps/cli/src` 仅剩 `model/conversation/model_tests.rs` 一条历史注释，零真实残留。`adapter/output_widget.rs` 已收敛为 `set_document` + `clamp_scroll_state`，不再镜像写 `lines`。

**allow(dead_code) 处理**：移除 `render/syntax.rs` 文件级 `#![allow(dead_code)]`（其 `language_by_extension`/`highlight_line`/`extension_from_path` 已被 `blocks/assistant_message.rs`（fence 高亮）与 `output/diff.rs` 实际调用，clippy 验证活跃）。保留 `render/mod.rs` 模块级 `#![allow(dead_code)]`：它仍在屏蔽尚未接线的 diff 渲染链（`output/diff.rs::build_diff_lines`/`build_delete_line`/`build_insert_line`/`build_context_line`/`line_num_width`、`primitives/diff.rs::diff`、相关 DIFF_* 常量）以及 `tool_display` trait 的 `result_max_lines`/`format_result_summary` 默认方法——这些原语已带回归测试但尚未在 AssistantMessage/ToolCall block 中接通，强行去 allow 会触发 dead_code 警告。

**关联 bug 回归测试现状**：
- #61：`primitives/diff.rs::test_diff_line_keeps_left_indent_not_flush_left`（diff 行保留两空格左缩进）+ `selection_overlay.rs::test_overlay_sets_bg_keeps_fg`（选中行只设 bg、保留原 fg）。
- #62：`blocks/tool_call.rs::test_tool_call_title_visible_not_background_color`（标题 span fg ≠ bg 且 ≠ SURFACE）。
- #65：`blocks/assistant_message.rs::test_assistant_fence_does_not_leak_style_after_close`（fence 结束后普通行不继续 CODE 色）——fence 已在 assistant 组件接线，非 gap。
- #71：结构性消除，输出区无 `render_range`/行下标越界路径（grep 零残留 + `clamp_scroll_state` 用 `total_lines().saturating_sub` 夹取）。
- #74：新增 `blocks/mod.rs::test_render_block_assistant_after_system_does_not_inherit_dark`（System(Muted) block 后 Assistant block 独立用 ASSISTANT 色，不继承暗色）。
- #80：`adapter/output_widget.rs::test_render_document_from_view_model_clamps_stale_scroll_offset`/`_preserves_valid_scroll_offset`（auto_scroll clamp 行为）。

**后续 gap（#58 待办，不阻塞本 task 的结构性修复）**：
1. **diff 渲染完整接线**：`output/diff.rs` + `primitives/diff.rs::diff` 原语已就绪并带测试，但尚未在 ToolCall（Edit/Write 工具结果）或 AssistantMessage 中接通。
2. **markdown 完整接线**：~~markdown/table 原语已接 AssistantMessage，但更复杂 markdown（嵌套列表、引用块等）覆盖待补。~~（G4 项1 收口：`primitives/markdown.rs` 按行补嵌套列表圆点/序号 + 引用块竖线，assistant 与工具结果共享路径自动受益。）
3. **syntax 高亮接线广度**：fence 已接，diff 内语法高亮原语就绪但随 diff 接线一并待办。
4. **tool 结果摘要渲染**：~~`tool_display` trait 的 `result_max_lines`/`format_result_summary` 默认方法未被 `format_tool_call` 调用。~~（G2 收口：`result_max_lines` 已接入 `format_result_lines`；`format_result_summary` 判定无价值已删除。）
5. **排队中即时显示**：~~排队提交需 `key.rs` → `QueueSubmission` 即时反馈~~（G3 收口：`key.rs` 入队同时派发 `QueueSubmission` 即时显示「⏳ 排队中」块，drain 时 `ClearQueuedSubmissions` 清块再正式回显，无双显示）；~~`(default:)` 行、done 间距等交互细节仍待补。~~（G4 项2/3 收口：AskUser 自由输入补回 `(default:)` 行、done 提示恢复尾随空行间距。）

**TDD/验证**：新增 #74 回归测试；`cargo build -p cli`、`cargo test -p cli`（375 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh`（含新增 check-render-isolation.sh）全绿。

### #58 G1 · diff 完整接线（含 #61 原始场景修复）

**状态**：待确认（接续 Phase 6 后续 gap 第 1/3 项）

**Edit 工具结果 diff 接线**（commit f0bd1f5）：新增 `blocks/edit_diff.rs`，解析 Edit 结果的 `---DIFF---\n{old}\n---DIFF---\n{new}` → `primitives/diff::diff` 渲染（行号 + 加减语义色 + INDENT 缩进 + 新增行语法高亮）；`tool_call.rs` 路由。

**必修 1 · assistant markdown unified diff**（#61 原始症状）：LLM 输出含 unified diff 的 ` ```diff ` 代码块此前只当通用代码块（syntect 单色），diff 行贴最左、无加减语义。新增原语 `primitives/unified_diff.rs::render_unified_diff`：识别 `@@` hunk 头 / `+++`·`---`·`diff `·`index ` 文件头与元信息（Meta，不误判加减）/ `+` 新增 / `-` 删除 / 其它 context 五类行；复用 `output/diff.rs` 的 INDENT 缩进 + `DIFF_ADD_FG`/`DIFF_REMOVE_FG` 语义色；新增行去前导 `+` 后可选 syntect 高亮再补回 `+`；不重算行号（@@ 原文自带）。`blocks/assistant_message.rs` 在 fence 语言为 `diff` 时路由到此原语。回归测试覆盖缩进/加减语义色/新增行高亮/选中叠加保留 fg（#61）/空 diff/无 hunk 头纯 context/文件头不误判。

**必修 2（M1）· Edit 路径语法高亮运行时失效**：运行时 `view.title` 是裸工具名 `"Edit"`（`view_assembler/output.rs:224 title: call.name.clone()`，无路径括号），原 `file_ext_from_title("Edit")` 恒返回 None → 语法高亮永不激活，旧测试因人工注入 `title="Edit(...)"` 掩盖。改为 `file_ext_for_edit(summary, result)`：优先从 `summary`（工具入参 JSON，含 `file_path`，由 `adapter` 将 `input.to_string()` 存入）取扩展名；退而从 Edit 结果 header `replaced N occurrence(s) ... in {path}` 解析。`render_edit_diff` 签名改为接收 `summary` 而非 `title`，`tool_call.rs` 传 `view.summary`。新增真实裸 `title="Edit"` + summary 含 file_path 的测试，断言 ext 被推断、语法高亮激活（with-ext span 数 > 无 ext 基线），锁死 M1 回归。

**allow(dead_code)**：`render/mod.rs` 模块级 `#![allow(dead_code)]` 仍保留——经验证其余未接线 gap 原语（`safe_text::safe_char_at`、`block_cache::contains`、`output_area/display` 换行族、`selection_render::apply_selection_to_line`、theme `ACCENT_BRIGHT`/`DIFF_ADD_BG`/`DIFF_REMOVE_BG`）仍依赖它（去除会触发 9 条 dead_code 警告），不可收窄。本次接线的 `render_unified_diff`/`file_ext_for_edit` 等均已活跃，无需 allow。

**TDD/验证**：先写暴露 M1 的真实 title 测试 + assistant unified diff 测试见红再实现；`cargo build -p cli`、`cargo test -p cli`（394 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh` 全绿。

### #58 G2 · 工具结果 fence/markdown 接线（修 #65）+ 结果摘要 gap 收口

**状态**：待确认（接续 Phase 6 后续 gap 第 2/4 项 + #65）

**修 #65（共享 fence 渲染）**：把原 `blocks/assistant_message.rs` 内联的 fence/markdown/table 状态机提取为共享原语 `primitives/fenced.rs::render_fenced_markdown(text, base_style, indent, width) -> Vec<RenderedLine>`（fence 切换着 TEXT_DIM、` ```diff ` 走 `render_unified_diff`、其余按 fence_lang 语法高亮或 CODE 单色、fence 外走 markdown/table；`indent` 前缀加在每行首并保持 plain 一致）。`assistant_message.rs` 改为单行调用（indent=""），`tool_call.rs::format_result_lines` 改为调用它（indent=INDENT）后再按行数截断。状态机随调用销毁，fence 结束后普通行恢复 base 色，结构上隔离 #65。Edit 工具 `---DIFF---` diff 渲染路径（G1）保持优先——`render_tool_call` 先判 `render_edit_diff`，否则才走 fence/markdown。回归：`primitives/fenced.rs` 8 个单测（正常/空/无闭合 fence/多 fence 交替/indent/diff）+ `tool_call.rs::test_tool_call_result_fence_does_not_leak_code_color_after_close`（先红：旧 `format_result_lines` 单色套行，代码行不为 CODE 色）+ 无闭合 fence/max_lines=0/空结果三个边界测试。

**结果摘要 gap（Phase 6 gap 4）择一处理**：
- **`result_max_lines` → 接入**：新增 `tool_display::result_max_lines(name)` 查注册的 `ToolDisplay::result_max_lines`（未注册回退 `TOOL_RESULT_MAX_LINES`），`format_result_lines` 用它替换原硬编码 `if tool_name=="TaskListComplete" {0} else {5}`，把行数策略收敛到 `ToolDisplay`（DRY，AskUserQuestion/TaskListCreate/TaskListComplete 的 0 行抑制现已生效），移除其 `allow(dead_code)`。回归 `test_tool_call_result_max_lines_uses_tool_display_zero`。
- **`format_result_summary` → 删除**：该方法合成「✓ {name} completed」类摘要字符串，但 G1/G2 管线渲染的是 `call.result` 真实结果文本，从未调用它；返回 `vec![]` 的 override 等价于 `result_max_lines()==0`（已覆盖），AskUserQuestion 的「✓ 已回答」在 `result_max_lines==0` 下不渲染亦无意义。故删除 trait 默认方法 + 全部 5 处 override（tool_impls AskUserQuestion、task_impls TaskListCreate/TaskListComplete/EnterPlanMode/ExitPlanMode）+ 其 `allow(dead_code)`，避免接入会以合成文本覆盖真实结果。

**测试副作用修正**：`app/state/tests.rs::test_thinking_then_grep_renders_tool_block_in_output_area` 此前未设 `layout.output_area_rect`，宽度回退为 1，工具结果走 markdown 换行后逐字拆行。设 `output_area_rect = Rect::new(0,0,100,40)` 提供真实宽度（与实际终端行为一致）。

**TDD/验证**：先写 #65 工具结果 fence 回归见红（旧代码代码行非 CODE 色）再实现；`cargo build -p cli`、`cargo test -p cli`（406 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh` 全绿。

### #58 G3 · 排队中输入即时显示接线（收口 Phase 6 gap 5）

**状态**：待确认（接续 Phase 6 后续 gap 第 5 项）

**问题**：model 层 `QueueSubmission`/`ClearQueuedSubmissions` intent + `ConversationBlock::QueuedUserMessage → OutputBlockKind::QueuedSubmission`（暗色「⏳ 排队中: ...」）+ view_assembler 映射均已就绪并带测试，但**无 live 调用者**：活跃排队入口 `key.rs` 处理中提交仅 `input.push_queue`（写 `InputState::input_queue`），从不派发 `QueueSubmission`，故排队消息在等待期间不显示，仅在 drain 后作为已发送消息显示。`root_reducer` 虽路由 `QueueSubmission` 但其顶层 update 未接入运行时。

**实现**：
- 新增 `notice.rs::App::enqueue_submission_echo(text)`（派发 `QueueSubmission` + `refresh_output_widget_from_model`）与 `clear_queued_submission_echo()`（派发 `ClearQueuedSubmissions` + refresh），沿用既有 `append_user_echo`/`append_system_notice` 的「model.apply → refresh」单向模式，副作用不在 update 直接 IO。
- 入队：`key.rs` 处理中提交分支在 `input.push_queue(input)` 同时调 `enqueue_submission_echo(input)`，即时显示「⏳ 排队中」块。
- drain：`ui_event.rs::DrainQueuedInput` 在 `drain_queue` 后、`append_user_echo` 前调 `clear_queued_submission_echo()`，先清排队块再以正式 `UserMessage` 回显，避免「排队块 + 已发送回显」双显示。

**一致性策略（避免双真相）**：`InputState::input_queue`（VecDeque）为权威发送队列（drain 决定实际注入 agent 的内容），`QueuedUserMessage` 块为其显示投影。二者仅在两处成对维护——入队（key.rs：push_queue + QueueSubmission）与出队（ui_event.rs drain：drain_queue + ClearQueuedSubmissions）——故始终同步。slash 命令路径（`enter.rs`/`key_nav.rs`）的 `push_queue` 是 `pending_slash` 传输载体、立即 drain，**不**派发 `QueueSubmission`（不应显示为排队消息）；drain 时 `ClearQueuedSubmissions` 对其为 `count=0` 无副作用 no-op，一致性不受影响。

**TDD/验证**：notice.rs 新增 3 测试见红再实现——入队→QueuedUserMessage 块出现并经 document 渲染「⏳ 排队中」、清除→排队块全清 + 仅一条正式 UserMessage（无双显示）、空队列清除 no-op；`cargo build -p cli`、`cargo test -p cli`（409 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh` 全绿。提交 `7e022b0`。

> 注：`app/state/tests.rs::test_thinking_then_grep_renders_tool_block_in_output_area` 为依赖 `/tmp` 实际文件 + 测试并行调度的既有非确定性 flake（与本改动逻辑无关），单测/重跑均通过。

### #58 G4 · markdown 完整性（嵌套列表 + 引用块）+ AskUser default 行 + done 间距

**状态**：待确认（收口 Phase 6 后续 gap 第 2 项 + Phase 5 T6b/T6c 迁移遗漏的两处交互细节）

**项 1 · markdown 嵌套列表 + 引用块**（commit b96c6fb + 5af541f）：原 `primitives/markdown.rs::markdown` 仅做 inline 解析（bold/italic/code/link/strikethrough），无块级处理；引用块 `> ` 原样显示、列表标记 `- `/`1. ` 无视觉区分（缩进虽因 `text.lines()` 保留但无 marker 样式）。在 inline 之上按行补块级装饰（与 `fenced.rs` 逐行喂入契合，assistant 与工具结果共享 `render_fenced_markdown → markdown` 路径自动受益，DRY）：
- 引用块：`> ` / 嵌套 `> > ` → 弱化色（TEXT_DIM）竖线 `│ `（每层一根），正文走 inline（保留 bold/code/link），正文着 TEXT_MUTED。
- 列表：`- `/`* `/`+ ` 无序统一渲染 ACCENT 色圆点 `• `，`N. `/`N) ` 有序保留序号 `N. `，保留行首缩进层级；续行按 marker 宽度对齐缩进、不重复 marker。
- `plain` 与可见 spans 保持一致；`-notalist`（无空格）等非列表行原样走 inline。
- 有序列表前缀提取用 `split_at(digit_len)` 而非裸索引切片，规避 unsafe text op 守卫（digit_len 为 ASCII 数字字节数，合法字符边界）。
- 回归：11 个新增单测（引用竖线+弱化色/嵌套两竖线/引用内 bold/无序圆点+ACCENT/嵌套缩进/有序序号/列表内 code/无空格非列表/多行混合）。

**项 2 · AskUser 自由输入 `(default: ...)` 行**（commit 0a75cb5）：Phase 5 T6b 把 AskUser 迁到 `ConversationModel` 时，空选项自由输入分支丢失了旧 `push_ask_user` 的 `(default: {d})` 提示行（block 未携带 default）。沿 intent→block→view→render 链补回 `default: Option<String>` 字段：`ConversationIntent::ShowAskUser` / `ConversationBlock::AskUser` / `AskUserBlockView` 均加该字段；`ui_event.rs` 空选项分支传入 `default`（有选项分支 default 仅用于定位 cursor，传 `None`）；`blocks/ask_user.rs` 空选项分支渲染弱化色 `  (default: {d})` 行。回归：携带 default 渲染该行 / 无 default 省略该行两测试。

**项 3 · done 完成提示尾随空行间距**（commit 7d9bb29）：Phase 5 T6c 把 done 提示迁到 `append_system_notice(done_notice)` 时丢失了旧 `push_done` 的尾随空行间距。按「间距由块组件承担」设计恢复：`done_notice` 文案末尾保留 `\n`，`render_diagnostic` 检测 `text.ends_with('\n')` 时追加一行尾随空行（System block 文本经 model→assembler 全程不 trim，换行保真到达渲染层）。该语义对普通单行 notice 无影响（不以换行结尾不追加）。回归：`done_notice` 改 `trim_end` 断言 + 新增换行断言；`diagnostic.rs` 新增有/无尾随换行两测试。

**TDD/验证**：三项均先写见红再实现；`cargo build -p cli`、`cargo test -p cli`（423 passed）、`cargo clippy -p cli -- -D warnings`、`check-architecture-guards.sh`（含 ≤400 行、unsafe text op）全绿。markdown 原语 268 行、model.rs 396 行均 < 400。

### #57 TUI 目录物理收口：并入剩余 widget/service 目录、删 core shim

**状态**：待确认

**背景**：feature #55 完成三层(render/adapter/app)落地与 core/state·core/update·model/session 清理后，对照架构 spec `2026-05-27-tui-model-view-architecture.md` 的目标目录，`apps/cli/src/tui/` 仍多出 6 个顶层目录。#55 文档已把这部分声明为"深层重复后续可继续拆"，本 feature 专门收尾。

**spec 目标 9 个目录均已存在**：`app/ update/ model/ view_assembler/ view_model/ view_state/ render/ effect/ adapter/`。

**多出的 6 个顶层目录（2026-05-29 核实）**：
1. `core/` — 仅含一句弃用说明的空 `mod.rs`，无 re-export。**纯遗留，可直接删**（确认无 `crate::tui::core` 引用后移除）。
2. `output_area/`（~1973 行）— 输出区 ratatui widget（markdown/tool_display/rendered_lines/cache）。spec 归属 `render/output/`。
3. `input/`（~1355 行）— 输入 widget（InputArea + mouse/paste/clipboard handler）。spec 归属 `render/input` + widget；注意与 `model/input/` 区分。
4. `display/`（~1282 行）— status_bar/theme/safe_text/task_window/syntax/dialog。spec 归属 `render/{status,dialog,theme}`；`safe_text` 无 spec 归属，需定（可入 render 或 util）。
5. `completion/`（~628 行）— 补全数据源（files/commands/models/sessions/parser）。spec tui 树未规划，需定为 service 层或并入相关模块。
6. `session/`（~408 行）— 会话生命周期（processing/resume/session_lifecycle）。spec 未规划，最接近 effect executor / runtime model，需定归属。

**目标**：
1. 删除 `core/` 空 shim（确认无引用）。
2. 将 `output_area/`、`input/`、`display/` 物理并入 `render/`（或明确标注为 render 的 widget 子层），消除"中间态"。
3. 为 `completion/`、`session/` 定位（render/service 层或并入相关 model/effect）。
4. 收口后顶层目录与 spec 目标结构对齐。
5. **加顶层目录白名单 guard**：新增 `.agents/hooks/check-tui-toplevel-layout.sh`，扫 `apps/cli/src/tui/*/`，只允许 spec 认可的目录集合（`app update model view_assembler view_model view_state render effect adapter` + 收口后最终保留项）；出现集合外目录即 fail，提示按 spec 归入对应层。接入 `check-architecture-guards.sh`（Stop hook）。必须在迁移完成的同一步加上（先迁移、迁完立刻加 guard 锁住，否则会被存量目录挡住）。
6. **可选**：扩展 `check-forbidden-imports.sh`，禁止 `crate::tui::core` / `::output_area` / `::display` / `::completion` / `::session` 等被并入/删除的旧模块路径导入，防止目录改名后 import 仍引用旧位置。

**为什么要 guard**：纯目录整理若不锁住会逐步漂回（有人又在顶层新建 `output_area/` 或 `use crate::tui::core::...`）。白名单 guard 让结构退化在 Stop hook 即被拦截。

**完成情况（2026-05-28）**：
1. ✅ 删除 `apps/cli/src/tui/core/` 空 shim。
2. ✅ `output_area/` 迁入 `render/output_area/`，旧输出 widget 作为 render 层子模块保留，避免顶层漂移。
3. ✅ `input/` 迁入 `render/input/`，与 `model/input/` 的业务状态层物理分离。
4. ✅ `display/` 迁入 `render/display/`，status bar 相关 `#[path]` 测试引用同步调整。
5. ✅ `completion/` 未作为独立 layer 保留：纯解析、类型、命令/模型/session 候选组装与补全状态收口到 `model/input/completion/`；文件系统扫描候选生成收口到 `effect/completion/`，避免 Input Model 直接执行 IO。
6. ✅ `session/` 未作为独立 layer 保留：`spawn_processing`、resume/load/save/run lifecycle 等副作用编排收口到 `effect/session/`；processing job、session metadata 等状态继续归 `model/runtime/`。
7. ✅ 新增 `.agents/hooks/check-tui-toplevel-layout.sh` 并接入 `check-architecture-guards.sh`：只允许 `adapter/app/effect/model/render/update/view_assembler/view_model/view_state` 顶层目录，禁止旧 `tui::core/output_area/input/display/completion/session` 路径回归。
8. ✅ 同步修正既有架构 guard：`check-tui-model-view-boundaries.sh`、`check-tui-tea-purity.sh`、`check-unsafe-text-ops.sh`。
9. ✅ 验证：`cargo test -p cli`、`cargo clippy -p cli -- -D warnings`、`cargo check`、`check-architecture-guards.sh`、`check-unit-tests.sh` 通过。

**归档前待确认**：用户确认 `completion -> model/input + effect/completion`、`session -> effect/session + model/runtime` 的拆分符合架构文档后归档。

**性质**：低优先级、纯结构整理，不改业务行为；可分目录小步迁移，每步保证编译/测试通过。

**关联**：feature #53、#55；spec `docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md`。

### #56 输入单一真相约束：禁止直接改 input_area 并加架构 guard

**状态**：待确认

**背景**：用户 2026-05-28 指出 `input_area` 所有直接修改（text 或 cursor）后未同步 `model.input.document`，突破了架构约束。约定先等 feature #55 输入路径收口后再做，故独立成 feature 跟踪。

**约束**：输入的 text/cursor 业务真相只在 `model.input.document`（InputModel）。`InputArea` 是 View，必须由 model 单向派生（`adapter/input_widget.rs` / `core/input_adapter.rs`）。update 层 **不得**直接调 widget 可变方法（`set_text`/`move_*`/`delete_word`/`backspace`/`input`/`enter`/`clear`/`cut`）再手动 sync——"改完再 sync"本身就是反模式（spec 目标 #2、Milestone 3）。

**当前违规点**（2026-05-28 核实）：`apps/cli/src/tui/core/util.rs`、`core/update.rs`、`core/update/key.rs`、`core/update/ask_user_key.rs`，以及 `input/mouse_handler.rs`、`input/paste_handler.rs` 仍直接改 input_area，部分跟一行 `model.input.document.set_cursor_col(...)`，很多未同步。

**为什么重要**：widget 与 model 漂移正是 bug #79（`7c2639e` Ctrl+A/E/Left/Right/End 输入位置错位、`bcaca91` 上下翻历史 + Ctrl+W delete_word 未同步）以及 #75（CJK 顺序）、#77（@ 补全回删）的同源根因；"改完手动 sync" 持续产 bug。

**目标（两层保障）**：
1. **静态 guard**：新增 `.agents/hooks/check-tui-input-single-source.sh`，禁止 adapter 之外对 `input_area` 调可变方法；仿 `check-tui-tea-purity.sh` 的 `EXEMPT_FILES` + `// allow input_single_source` 逃逸，豁免随迁移逐项清零；接入 `check-architecture-guards.sh`（Stop hook）。
2. **类型级**：迁移后将 `InputArea` 可变方法可见性收紧为 `pub(in crate::tui::...)`，只对 adapter 模块可见，越界即编译失败。

**依赖**：feature #55（先把键盘输入收口为 InputIntent→InputModel→InputChange→adapter→widget，否则 guard 会被大量 legacy 违规挡住）。

**本轮完成（feature/56）**：
- 键盘输入、AskUserQuestion 自由输入、粘贴、补全确认、图片 pending count 更新均改为走 `InputIntent` → `InputModel::apply` → `adapter/input_widget.rs`，不再由 app/update 直接修改 `input_area` 后手动 sync。
- `InputModel` 补齐 `ReplaceText`、`InsertNewline`、`DeleteWordBeforeCursor`、`AcceptCompletionValue`、`SetAttachmentCount` 等 intent；`InputDocument` 补齐 replace/delete-word/is-empty 等纯逻辑。
- `app/update`、`app/util`、`app/slash/suggestions` 改读 `model.input.document` 作为 text/cursor 业务真相；`input_area` 仅作为 widget/view，由 adapter 应用变化。
- 新增 `.agents/hooks/check-tui-input-single-source.sh` 并接入 `check-architecture-guards.sh`，禁止 adapter/input_area 外直接改 `input_area` text/cursor，禁止 model 外直接改 `model.input.document`，禁止 app/update 读 `input_area` text/cursor 作为业务真相。
- `InputArea` 的 text/cursor 可变方法收紧为内部或测试可见，外部越界调用将触发编译或 guard 失败。

**验证**：`cargo check -p cli`、`cargo test -p cli`、`.agents/hooks/check-architecture-guards.sh`。

**关联**：bug #79、#75、#77；feature #53、#55；spec `docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md`。

### #55 TUI 架构收口：render / adapter / app 三层落地 + 清理 legacy core

**状态**：待确认

**背景**：feature #53（TUI Model/View 迁移）的遗留收口。按 `docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md` 的目标模块结构审计 `apps/cli/src/tui/`，新 Model/View 骨架已建立且依赖方向大体正确，但仍有以下偏差未落地（#53 状态"待确认"，legacy shell 作为兼容 facade 保留）。

**已对齐 spec**：`model/{conversation,input,runtime,diagnostic}`、`view_assembler/`、`view_model/`、`view_state/`、`effect/`（部分）、`update/`（mapper + coordinator + root_reducer）。

**待收口的偏差**：
1. **缺 `render/` 层**：spec 要求统一 `render/{layout,output/{block,markdown,cache,line},status,input,dialog,theme}`；当前渲染散在 legacy `output_area/`（markdown/tool_display/rendered_lines/rendered_cache）+ `display/`（status_bar/theme/render/safe_text/task_window/syntax）+ `input/` widget + `view_model/render.rs`。
2. **缺 `adapter/` 层**：spec 要求 `adapter/{agent_event,terminal_event,task_event,hook_event}`；当前 adapter 散在 `update/*_mapper.rs` 与 `core/*_adapter.rs`。
3. **缺 `app/` 薄壳**：被臃肿的 `core/` 顶替，`core/` 还塞了 `core/update/`、`core/state/`、`effect_runtime.rs`、`slash` 等。
4. **`effect/` 缺 `executor.rs`**：executor 现在在 `core/effect_runtime.rs`。
5. **`view_state/` 缺 `cache.rs`**：render cache 在 `output_area/rendered_cache`。
6. **legacy 重复并存**：`core/update/`（旧 ui_event/key/ask_user handler）vs 新 `update/`；`core/state/`（旧 AppState）vs 新 `model/`；顶层 `session/` vs `model/session/`、顶层 `completion/` vs `model/input/completion.rs`、顶层 `input/` vs `model/input/`。
7. **多出第 5 个 model context `model/session/`**：spec 只定义 Conversation/Input/Runtime/Diagnostic 四个，需决定是否纳入正式设计或并回 Runtime。
8. **细节**：`update/` 无 `change_router.rs`（由 `root_reducer.rs`+`dirty.rs` 承担，需确认是否符合设计）；`model/conversation/` 无 `message_snapshot.rs`。

**目标**：
1. 建立统一 `render/` 层，将 `output_area/`、`display/` 渲染逻辑、`view_model/render.rs` 收口进去。
2. 建立 `adapter/` 层，归拢 agent/terminal/task/hook event 适配。
3. 将 `core/` 收薄为 `app/`（或保留 core 命名但去除 legacy update/state/adapters）。
4. 清理 legacy 重复目录，迁移完成后删除旧 `core/update/`、`core/state/` 等。
5. 补齐 `effect/executor.rs`、`view_state/cache.rs`。
6. 决定 `model/session/` 去留。
7. 落地 spec §834 起的剩余架构 guard（render isolation / viewassembler boundary / adapter guard / output line legacy guard / effect boundary）。

**本轮补完（fix/feature-55-complete）**：
- `app/` 不再只是 re-export：原 `core/mod.rs`、`event`、`resize`、`run_loop`、`runtime`、`state`、`update`、`slash`、`util` 已迁入 `app/`；`core/` 仅保留弃用兼容 namespace，legacy `core/update`、`core/state` 已删除。
- `model/session/` 已并入 `model/runtime/session_*`，去掉第 5 个 model context。
- `render/` 继续收口输出渲染实现：`output_area/markdown`、`rendered_lines`、`render_blocks`、`render_spans`、`render_status`、`diff`、`tool_display` 已迁入 `render/output/`；`display/syntax`、`display/task_window` 已迁入 `render/`。
- 新增并接入 `.agents/hooks/check-tui-model-view-boundaries.sh`，覆盖 model 纯度、render legacy fallback、view_assembler、view_model、SDK/runtime event 边界与 #55 物理 legacy 目录约束；更新现有 TEA/effect/output guards。

**关联**：feature #53（TUI Model/View 迁移）；spec `docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md`。

### #49 AskUserQuestion 增加「以上全是」与「chat about this」选项

**状态**：已完成（02e8986），显示修复待确认

**背景**：当前 AskUserQuestion 选择模式只允许选择 LLM 给出的某一个 option，或在允许 free_input 时手动输入；无法批量同意「以上全是」，也无法在保持原问题上下文的前提下补充自定义说明。当 LLM 给出多个可选项且用户想同时表达「这些都同意 + 我还想补一句」时，只能多轮往返，影响交互效率。

**目标**：在 AskUserQuestion options 末尾追加两个内建选项：
1. 「以上全是」：用户选中后，回传内容为前面所有 LLM 提供 option 的集合（按显示顺序），让 LLM 知道用户认可全部选项。
2. 「chat about this」：用户选中后，进入自定义输入态，提交内容为用户输入的自由文本。该选项不要求原问题开启 free_input，是 options 模式下也可用的统一补充入口。

**核心要求**：
1. 两个内建选项**必须**显示在 LLM 提供的 options 之后，并与原 options 视觉一致，但要让用户清楚这是工具内建项。
2. 两个内建选项**必须**在 options 数量 ≥ 1 时才出现；options 为空（仅 free_input）时不出现「以上全是」，但「chat about this」可考虑保留以承担补充语义（待 spec 讨论）。
3. 「以上全是」回传给 LLM 的内容**必须**是结构化的全部选项集合，不能只回传字面字符串「以上全是」，否则 LLM 无法判断到底选了哪些。
4. 「chat about this」**必须**进入与 free_input 等价的输入态：输入区获得焦点、Enter 提交、Esc 取消回到选择态；不应直接把字面「chat about this」当作回答提交。
5. 两个内建选项**不得**与 LLM 提供的 option label 冲突；如果发生重名，**必须**有明确的去重或避让策略。
6. 既有「上下键移动 → Enter 确认」交互**必须**保持；内建选项参与同一 selected index 序列，不能引入第二条选择状态机。

**推荐设计方向**：
- 在 AskUserQuestion options 渲染层把内建选项作为合成项追加到列表末尾，selected index 仍是单一来源；
- 内建选项类型化（例如枚举 `BuiltinOption::All` / `BuiltinOption::Chat`），在 Enter 时分支处理：`All` 直接构造全部 option 的回答，`Chat` 切换 UI 到自由输入态再提交；
- 回答提交层区分「来自 options 的选择」和「来自 chat 自由输入」两种结构，避免 LLM 收到无法解析的混合字符串；
- 多语言/i18n 文案以中文为主，参考 `[Enter] 确认 / [Esc] 取消 / [Tab] 切换选项` 现有风格，统一文案常量；
- 与 bug #63（AskUserQuestion options 选择同步）协同：渲染缓存必须随 selected index 变化失效，确保内建选项也能正确高亮。

**涉及路径（预计）**：
- AskUserQuestion options 渲染与 selected index 状态（TUI options 列表）
- AskUserQuestion 输入态切换（chat about this 进入 free_input 子态）
- AskUserQuestion 回答构造与回传（结构化「以上全是」与自定义文本）
- AskUserQuestion 文案常量与 i18n
- 既有 free_input 模式与新增 chat about this 的关系澄清

**验收标准**：
1. 任意 options 模式下，选项列表末尾稳定出现「以上全是」与「chat about this」两项；options 为空场景按 spec 决定是否出现。
2. 选择「以上全是」后，工具回答能让 LLM 准确还原所有 LLM 提供 option（按显示顺序），不会被误识别为某一个 option。
3. 选择「chat about this」后，TUI 进入自定义输入态，Enter 提交内容、Esc 返回选项列表；不会把字面文案当作回答。
4. 内建选项与 bug #63 的修复方向一致，上下键高亮和回车确认始终对应同一项，不出现 TUI 显示与实际回答不一致。
5. 单元测试覆盖：仅 options、options + free_input、options 为空、内建选项与 LLM option 重名、内建选项被高亮后 Enter 行为。

**明确不做**：
1. 不引入多选机制；「以上全是」只是一次性全选回答的语义糖，不改变 options 单选模型。
2. 不为「chat about this」额外引入富文本/Markdown 渲染；输入态仍以单行/多行纯文本为主。
3. 不在本 feature 中重做 AskUserQuestion 渲染缓存，相关修复归 bug #63 范围。
4. 不替换或废弃既有 free_input：原 `allow_free_input=true` 行为保持，「chat about this」是 options 模式下的补充入口，不是替代。

### #50 CLI 目录整理：收拢碎片、统一模块层级

**状态**：实施中

**问题**：
1. **同名文件 + 目录并存** — `tui/input_area.rs` 与 `tui/input_area/`、`tui/key_hints.rs` 与 `tui/key_hints/`，模块层级混乱
2. **render 三层散落** — `src/render/`（diff/markdown/progress/theme）、`src/tui/render/`（chat 薄壳）、`src/tui/widgets/`（diff/markdown/tool_display），职责重叠，diff/markdown 可能重复
3. **status_bar 碎片化** — `status_bar.rs` + `status_bar_format.rs` + `status_bar_selection.rs` + 两个 test 文件，全摊在 `tui/` 根下
4. **application/ 废弃骨架** — `application/mod.rs` + `application/chat/mod.rs`，功能已在 runtime + TUI app 中实现
5. **repl/ 残留** — 旧版 rustyline REPL ~10 文件仍占位

**目标**：
- 删除 `tui/app/`，按功能聚合：

```
tui/
├── core/       ← TEA 核心 (mod/App, msg, event, update, run_loop, runtime, cmd_exec, util)
├── input/      ← 输入处理 (input_handler, mouse_handler, paste_handler) + input_area/
├── display/    ← 渲染 (render, stream, task_window, resize, status_bar*)
├── session/    ← 会话 (session_lifecycle, resume, processing)
├── slash/      ← 不变
├── state/      ← 不变
├── widgets/    ← 不变
└── completion/ ← 不变
```

- 同名文件+目录统一为目录形式（`input_area.rs`→`input_area/mod.rs`，`key_hints.rs`→`key_hints/mod.rs`）
- 顶层 `src/render/` 内容合并到 `tui/widgets/`，删除 `src/render/`
- 删除 `application/` 废弃骨架
- 标记 repl/ 为 deprecated 或评估是否可删

### #47 以 DDD 思路重新设计 Aemeath 架构

**状态**：✅ 已完成

**背景**：Aemeath 已从单一 CLI 演进为包含 TUI、LLM provider、工具系统、hook、skill、memory、task、worktree、权限与会话管理的 AI 编程助手。当前代码仍主要按技术分层和 crate 边界组织，随着功能增加，领域概念之间的边界容易变得模糊，例如 Agent 会话、工具执行、权限评估、上下文压缩、项目配置、skills 与 hooks 之间的职责交叉。用户希望以 DDD（领域驱动设计）的思路重新设计项目，让后续重构先有清晰的领域模型和边界，而不是直接按文件/模块做局部移动。

**设计结论**：核心域为 Runtime；Agent 是配置化实体；Runtime 使用 Session / Chat / Agent Looping / Turn / TaskBoard 作为统一语言；Task 属于 Runtime，由 Agent Looping 推进，持久化投影进入 Storage；CLI/TUI/HTTP/SDK 等入口保持薄，TUI/业务代码逐步只通过 `packages/sdk` 的 `AgentClient` 契约接入核心域；当前单二进制 Rust 部署下，`apps/cli` 的 composition root 仍依赖 `agent/runtime` 装配真实实现，但不得直接依赖 supporting domains 或 share/core；Runtime 作为唯一编排者调度 Project、Policy、Prompt、Provider、Tools、Storage、Hook、Audit；Cargo dependency graph、forbidden import、public API visibility 和 Stop hook 必须共同防止双向依赖与边界绕过；COLA 作为工程分层参考，要求 Adapter / Application / Domain / Infrastructure / Client 职责分离；Audit 独立；PermissionDecision 与 HookDecision 分离；Prompt 独立承载 Skill / Guidance / instruction；完整设计见 [spec](specs/047-ddd-redesign.md)。

**当前推进**：已按 GLM/DeepSeek review 修正 Phase 4 方向：`ChatApplicationService` 现阶段保持薄校验/分发，避免与目标态 application service 编排产生表述矛盾；`ChatLaunchOptions` 只保留共同启动选项，`max_agent_concurrency` 归入 `TuiChatLaunch`。Phase 4 修正版已将 `run_orchestration::run_chat` 的启动准备逻辑收束为 `ChatBootstrap`，并通过纯 helper `ChatModeSelection` / `permission_env_override` 降低入口分支与 env 副作用；随后继续把 `bootstrap_chat` 中的 setup 细节拆成 `concurrency`、`permissions`、`model_runtime`、`provider_client`、`prompt_bundle`、`runtime_support`、`tooling` 等 helper，最后在 `runtime_support` 中收束 hook/session 初始化。严格方案 B 首轮实施已完成：workspace 顶层收敛为 `apps/` + `crates/`，`shared/kernel`、`contexts/provider`、`contexts/tool` 已迁移为 `crates/core`、`crates/provider`、`crates/tools`，并建立 `crates/runtime` 与 `crates/{project,policy,prompt,storage,hook,audit}` skeleton；`apps/cli` 的 Cargo 依赖已收束为只直接依赖 `runtime`，入口层通过 `runtime::api` 访问过渡 re-export；新增 dependency graph、forbidden import、thin CLI、core upstream dependency 架构守卫并接入 `.agents/hooks/check-architecture-guards.sh` 与 Stop hook。Phase 2 持续瘦身：chat application 契约、sub-agent runner，以及 runtime bootstrap 中的 concurrency、permissions、model_runtime、provider_client、runtime_support 已迁移到 `crates/runtime`；Guidance 已从 `core` 拆入 `crates/prompt`，CLI 通过 `runtime::api::prompt::guidance` 使用，HookRunner 由 runtime 提供适配器。Feature #47 Chat runtime stream loop 已收束到 `runtime::api::chat`，内部落点为 `crates/runtime/src/chat/looping`：tool round、hook、AskUser、auto compact、task reminder、reflection 与日志输入构造由 runtime 承担；runtime 不知道 TUI，只暴露 Chat 事件与 queue port，CLI/TUI/HTTP 等入口通过 adapter 接入。CLI 侧继续保留 thin adapter、事件映射、渲染状态更新、启动参数解析和 logging adapter。下一步不再调整顶层目录，而是逐步把仍停留在 `core` 的配置、hook、storage、policy 等职责拆入对应 support domain。P9 已完成 CLI 非 UI 模块抽取：`reflection`、`mcp_loader`、`prompt`（prompt_build）、`image`、`logging_setup` 已迁入 `crates/runtime`，CLI 只保留纯 UI/入口层（TUI、REPL、render、CLI 参数解析、run_orchestration 薄壳）。后续从 `core` 继续拆分 support domain。P10 Batch 2 已完成 Storage domain：`logging/`、`history.rs`、`tool_result_storage.rs` 已从 `core` 迁入 `crates/storage`。后续继续 Batch 3-6。P10 Batch 3 已完成 Project domain：`worktree.rs` 从 `core` 迁入 `crates/project`，`WorkingContext` 保留在 `core::tool` 避免循环依赖。P11 已完成：`crates/` 重命名为 `agent/`，新增 `agent/share` 跨 service 公共抽象层，worktree 工具通过 `share::worktree_ops` 间接调用 `project`，门禁更新为 `tools→{core,share}`、`share→{core,project}`。P10 Batch 4 已完成：skill→prompt、hook→hook、scheduler→runtime、mcp+mcp_manager→tools、compact→runtime、agent+command+cost+reflection+session+state→runtime。task 和 memory 保留在 core（被 tools/project 跨 crate 引用），WorkspaceContext 类型保留在 core::session_types。core 最终只保留：config、error、memory、message、provider、session_types、string_idx、task、token_estimation、tool。Phase 0 已完成：创建 `packages/sdk`（AgentClient trait + ChatStream/SessionSnapshot/ChangeSet 等公共类型），`agent/runtime` 提供 AgentClientImpl 骨架实现（RuntimeHandle 薄代理），门禁允许 runtime 依赖 sdk。Phase 1 已完成：`AgentClientImpl::from_args()` 吞掉 setup.rs 的全部 build_* 编排（config→logging→model→API key→LLM client→tooling→hooks→session→prompt），CLI `run_orchestration.rs` 瘦身为 ~120 行（删除 setup.rs/runtime.rs/prompt.rs/application/，共 -678 行），通过 `ModelSelector` trait 分离模型选择关注点。Phase 5 边界修正已完成：原“CLI Cargo 只依赖 sdk、切断 runtime 直接依赖”目标在当前单二进制 Rust workspace 中缺少真实实现装配点，已改为可执行边界：`apps/cli` 同时依赖 `packages/sdk` 与 `agent/runtime`，其中 sdk 提供 `AgentClient` 契约，runtime 仅供 composition root 构造真实实现；architecture guard 同步要求 CLI 声明 sdk 依赖，并继续禁止 CLI 直接依赖 supporting domains/share/core。Phase 5 后续 SDK 投影已推进：`packages/sdk` 新增 `ChatBootstrapArgs`、`ModelSummary`，`SessionSummary` 补齐 CLI 展示字段；`agent/runtime::AgentClientImpl` 实现真实 `list_sessions` / `delete_session` / `list_models`，并新增 `TuiLaunchContext` 过渡投影集中封装 TUI 启动所需 runtime 内部对象；`apps/cli` 的 `models`、`sessions` 子命令已改为通过 `sdk::AgentClient` 调用，`Args` 转换改为输出 SDK DTO，新增 `runtime_adapter` 作为 CLI composition root 装配口；architecture guard 进一步限制非 TUI/非 composition root 文件不得直接使用 `runtime::api`。TUI 统一 SDK 解耦已继续推进：`packages/sdk::AgentClient` 新增真实 `chat(ChatRequest) -> ChatStream` 契约、`ChatEvent` 投影、`task_status()`、`save_current_session()`、`sync_current_messages()`；runtime `AgentClientImpl::chat` 已接入现有 `process_chat_loop`，负责 RuntimeStreamEvent→SDK ChatEvent 投影、当前 messages/workspace 同步和 cancel token 管理；TUI chat turn、Ctrl+C/Esc cancel、`/save`、MessagesSync/退出自动保存、task status 均改为通过 SDK 调用，TUI 不再直接创建 ChatLoopContext 或调用 runtime chat loop/session save；守卫新增 TUI chat/cancel 禁止项。P12 已完成：`packages/sdk` 补齐 TUI 可消费的强类型 DTO（图片、Agent progress、Workspace、Memory/Reflection/Skill view），`agent/runtime::AgentClientImpl` 统一承担 runtime domain → SDK DTO 投影，`apps/cli/src/tui/**` 已禁止并移除 `runtime::api` / `::runtime` 直接依赖；图片读取、reflection、TUI launch context、pending image/reflection、skill/memory 状态均改走 SDK 契约。P13 已继续推进 TUI runtime/domain 解耦：`CurrentTurnChanged` 改为 SDK ChatEvent/CLI Cmd 边界转发，paste 图片文件分类下沉到 `packages/sdk::classify_paste`，`output_area` 字符/字节索引改用 SDK 值对象；上一轮清空 TUI 直接 runtime::api 引用；本轮继续收口 6 个残留文件：`run_loop.rs` 移除 12 个未使用 runtime/domain 参数，`suggestions.rs` 改用 `sdk::builtin_commands` 与 `AgentClient::list_models`，删除废弃 `input_handler.rs`，`session_lifecycle.rs` 不再向 `CmdExecutor` 写入 TaskStore/WorkspaceContext，也移除直接 SessionStart/SessionEnd hook 调用；剩余高复杂度残留集中在 `slash.rs` 与 session 启动签名中的 runtime 编排参数，后续需迁移为 SDK 启动 DTO/命令端口。P14 已完成：`client.rs`（1349 行）拆分为 10 个子模块（from_args、chat、session、command、mapping、accessors 等），`api.rs` 收口为零直接引用。P15 已完成 (6d3a423)：runtime 内部按职责分层为 core/（核心流程：client、command、port、service、tui_launch）、business/（业务规则：agent、chat、compact、cost、prompt、reflection、scheduler、session、state）、utils/（工具：bootstrap、image），843 测试全部通过。P16 已完成 (ad29dba)：core/ 层端口隔离——新增 TaskStorePort、ProviderInfoPort、HookNotificationPort 三个细粒度 port trait，适配器实现位于 utils/adapter/；core/client/ 层的方法调用（thinking、notify hook、provider/model name、task snapshot/list）已改为通过 port trait，business/ 层残留 provider import 已统一；core/ 层零直接 use provider::/use tools::/use hook::/use prompt::；架构守卫全通过，201 测试通过。P17 已完成 (e827ecc)：share/core 瘦身——迁出 config/manager→runtime、message/integrity→runtime、token_estimation→runtime、config/paths I/O→runtime、config/hooks 转换逻辑→runtime、provider.rs→provider crate；skill_ops/worktree_ops 审计后保留在 share 作为 canonical 定义（tools/project 等通过 share re-export 访问）；tool.rs 被 4 crate 引用确认为真正共享内核保留；share 从 63→55 文件；架构守卫全通过，全部测试通过。P18 已完成 (当前提交)：架构守卫固化为 8 个：Cargo 依赖图、CLI 薄入口、Share 向上引用、COLA 层间纯度、禁止导入、文件行数、TUI TEA 纯度、不安全文本操作；新增 COLA 层间守卫（share 不得依赖严格 I/O、supporting domain 隔离）；强化 forbidden-import 正则覆盖 use registry:: 路径；扩展 TEA 守卫到 tui/core/ 全目录（slash/等）；TUI 白名单清零；全量编译/689 测试/cargo clippy 通过；#47 DDD 重构标记为已完成。

**DDD 概念参考**：
1. Strategic DDD：用统一语言（Ubiquitous Language）和限界上下文（Bounded Context）拆分业务语义边界；不同上下文之间通过 Context Map 明确关系。
2. Tactical DDD：在上下文内部识别实体、值对象、聚合根、领域服务、仓储与领域事件，确保业务不变量集中在聚合边界内。
3. Anti-Corruption Layer：对外部模型或兼容层建立防腐层，避免 Claude/OpenAI/MCP/Git/终端等外部语义污染核心领域模型。
4. 参考来源包括 Martin Fowler 对 Bounded Context 的说明、Microsoft Learn Tactical DDD 资料，以及本仓库已有 [#36 Multi-Agent DDD 设计](../superpowers/specs/2026-05-20-multi-agent-ddd-design.md)。

**目标**：新增一条架构级设计 feature，后续展开为完整 DDD 重设计 spec。该 spec 应回答：Aemeath 的核心域、支撑域、通用域分别是什么；哪些概念属于同一 Bounded Context；哪些 crate/module 是当前技术实现而不是领域边界；哪些地方需要端口/适配器、防腐层或领域事件；以及如何分阶段重构而不破坏现有 CLI/TUI 行为。初版见 [spec](specs/047-ddd-redesign.md)。

**建议范围**：
1. 建立统一语言词汇表：Session、Turn、AgentRun、ToolCall、ToolResult、PermissionDecision、WorktreeContext、Skill、Hook、Memory、Compaction、Provider、Model、Cost 等术语必须定义清楚。
2. 划分候选 Bounded Context：Conversation/Agent Runtime、Tool Execution、Permission & Policy、Project Workspace、Model Gateway、Skill & Guidance、Memory & Reflection、Hook & Automation、TUI Interaction、Persistence/Session History。
3. 绘制 Context Map：明确 upstream/downstream、customer/supplier、conformist、防腐层、shared kernel 等关系；特别关注 provider API、MCP、Claude Code 兼容、Git/worktree 和终端 UI。
4. 在每个核心上下文内识别聚合根和不变量，例如 AgentRun 聚合负责 turn 状态与 tool batch 生命周期，PermissionGrant 聚合负责授权 scope 与过期策略，WorkspaceContext 聚合负责 cwd/path_base/working_root 一致性。
5. 区分领域服务、应用服务和基础设施适配器，避免把工具执行、provider 调用、文件系统、hook shell 命令等 I/O 逻辑直接混入领域规则。
6. 定义重构路线：先补设计文档和领域词汇，再抽离纯领域类型与端口，最后迁移现有模块；每一步都必须保持现有行为与测试可验证。

**明确不做**：
1. 本条 feature 只登记设计方向，不直接移动 crate、拆文件或改运行逻辑。
2. 不恢复 #36 已移除的 server/agents/proto/infra 运行代码；#36 DDD 只能作为参考，不能把当前 CLI 项目重新扩大成分布式平台。
3. 不引入数据库、消息队列或微服务化作为默认目标；DDD 用于厘清模型与边界，不等同于上微服务。
4. 不以 Clean Architecture 分层命名替代 DDD 建模；可以借鉴端口/适配器，但必须先从领域语言和上下文出发。

**后续 spec 验收标准**：
1. spec 包含统一语言表，且每个术语有唯一含义和所属上下文。
2. spec 给出当前架构到目标 DDD 架构的 Context Map，并标出防腐层和共享内核边界。
3. spec 至少定义 3 个核心聚合及其不变量，并说明现有代码中的候选落点。
4. spec 给出分阶段重构计划，每阶段可独立验证，不要求一次性大重写。
5. spec 明确与 #36、#40、#42、#43、#45、#46 的关系，避免与已暂停或已完成 feature 目标冲突。

**涉及路径（后续预计）**：
- `docs/feature/specs/047-ddd-redesign.md`：完整 DDD 重设计 spec。
- `docs/superpowers/specs/2026-05-20-multi-agent-ddd-design.md`：既有 DDD 参考。
- `shared/kernel/src/`：领域类型、会话、工具契约、权限、skill、hook、worktree 等候选边界。
- `contexts/tool/src/`：工具执行上下文与基础设施适配器候选边界。
- `contexts/provider/src/`：Provider Anti-Corruption Layer 候选边界。
- `apps/cli/src/tui/` 与 `apps/cli/src/repl/`：Interface/Application Service 边界。

### #42 权限管控系统：交互式外部授权 + 统一权限评估

**状态**：设计中

**背景**：原始问题是在 allow all 模式下，用户明确要求访问 workspace 外路径时，Glob/Grep 仍被安全边界拦截。例如：

```text
Glob(**/*)
Search path '/Users/guoyuqi/Nextcloud/work/wanaka/wanakadeploy/cicdserver' is outside the workspace '/Users/guoyuqi/Nextcloud/work/wanaka/wanaka-platform'.

Grep /gha/notify|deployments|actor|pusher|github-actions|执行人/
in /Users/guoyuqi/Nextcloud/work/wanaka/wanakadeploy/cicdserver
Search path '/Users/guoyuqi/Nextcloud/work/wanaka/wanakadeploy/cicdserver' is outside the workspace '/Users/guoyuqi/Nextcloud/work/wanaka/wanaka-platform'.
```

**目标**：升级为完整权限管控系统。采用交互式授权体验与统一 `PermissionEngine` 评估模型：所有工具调用先归一化为 action / resource / risk / profile，再输出 Allow / Ask / Deny；权限模式包含 AskMe、Auto、Plan、AllowAll，其中 AllowAll 保留 root / YOLO 语义，Auto 是带护栏的日常开发模式，Plan 只允许规划和分析。完整设计见 [spec](specs/042-permission-control-system.md)。

**建议范围**：
1. 新增 `PermissionEngine` 与统一权限模型（request/action/resource/risk/decision/grant）。
2. 将外部路径授权做成 TUI 交互式选择，而不是以 slash command 为主入口。
3. `ToolContext` 或等价上下文保存 session 级授权 scope，路径必须 canonicalize 后保存。
4. Read / Glob / Grep 先接入统一权限评估，解决原始外部路径访问场景。
5. Edit / Write 后续接入 capability 检查，外部写入必须有 Write capability，AllowAll 除外。
6. AllowAll 下默认允许 workspace 内外读写执行，仅记录审计，不再被 workspace 边界拦截。
7. Sandbox 仅预留未来设计，本轮不实现；不引入复杂 modifier。

**涉及路径**：
- `shared/kernel/src/permission.rs`：权限模式、授权记录、决策模型与 engine。
- `shared/kernel/src/tool.rs`：`ToolContext` / session grants / path_base 与权限上下文。
- `contexts/tool/src/path_security.rs`：路径 canonicalize 与 scope 判断。
- `contexts/tool/src/file_read.rs`、`glob_tool.rs`、`grep.rs`、`file_edit.rs`、`file_write.rs`：工具接入统一权限评估。
- `apps/cli/src/tui/app/stream/permissions.rs`、`tools.rs`：TUI 权限 prompt、用户选择、approved/denied tool call 流程。


**目标**：把 Aemeath 的配置、项目指令和 skills 发现机制调整为 Codex 风格，统一围绕 `~/.agents` 与项目内 `.agents` 目录组织，降低与 Claude 专属路径的耦合，并支持把当前用户已有配置迁移到最新模式。

**核心要求**：

**实现结果（2026-05-21）**：运行时读取已切换到新路径并加入 Claude Code 兼容：全局配置 `~/.agents/aemeath.json`，项目配置优先 `{cwd}/.agents/aemeath.json`，不存在或不覆盖的字段可由 `{cwd}/.claude/settings.json` 补充，其中 Claude Code hooks 数组结构会转换为 Aemeath hooks；全局指令读取 `~/.agents/AGENTS.md`，项目指令优先 `{cwd}/CLAUDE.md`，不存在时 fallback 到 `{cwd}/AGENTS.md`；项目 skills 优先 `{cwd}/.claude/skills`，其次 `{cwd}/.agents/skills`，全局 skills 读取 `~/.agents/skills`；Hook 执行环境同时注入 `AEMEATH_PROJECT_DIR` 与 `CLAUDE_PROJECT_DIR`，兼容 Claude Code 脚本。应用主日志 `~/.agents/logs/aemeath.log`；guidance、memory、sessions、history、cost_history、mcp、settings 等运行数据统一派生自 `~/.agents`。程序不提供 `/config migrate` 且启动时不自动迁移；现有 `~/.aemeath` 与 `~/.claude/CLAUDE.md` 由发布/部署阶段手动复制到新路径。Worktree 下以启动 `cwd` 为边界读取，不跨 checkout 共享项目配置。

1. **全局配置目录**：默认使用 `~/.agents` 作为全局配置根，且该根目录本身必须可配置。
2. **Agent 配置文件**：Aemeath 的主配置文件改为 `aemeath.json`，默认路径为 `~/.agents/aemeath.json`；项目级配置后续可按 Codex 风格放在项目 `.agents` / `.codex` 同类目录中评估，但本 feature 的明确目标是先统一全局配置与 agent 配置命名。
3. **AGENTS.md 指令读取**：读取 `~/.agents/AGENTS.md` 与 `{cwd}/AGENTS.md`，用于替代当前 `CLAUDE.md` 方向；拼接顺序、冲突处理和 prompt injection 扫描需在实现前明确。
4. **skills 读取目录**：skills 固定优先读取 `~/.agents/skills` 与 `{cwd}/.agents/skills`，保持 skill 包与命名空间机制可用。
5. **现有配置迁移**：实现时必须提供从当前模式迁移到新模式的路径，包括但不限于 `~/.aemeath/config.json`、`{cwd}/.aemeath/config.json`、`~/.aemeath/skills`、`{cwd}/.aemeath/skills`、`{cwd}/CLAUDE.md`、`~/.claude/CLAUDE.md` 到新路径的迁移、兼容读取或提示方案。
6. **Worktree 场景**：必须考虑 git worktree。读取项目指令和 repo skills 时，不能只假设 `cwd` 就是主 checkout；需要明确 cwd、worktree checkout root、git common dir / repo root 的关系，避免 linked worktree 中漏读项目级 `.agents/skills` 或重复读取同一份配置。

**推荐设计方向**：

- 新增统一的配置根解析逻辑：默认 `~/.agents`，允许通过 CLI 参数、环境变量或现有配置 bootstrap 指定其他目录；解析后统一供配置、skills、AGENTS.md 使用，避免各模块重复拼路径。
- 新增迁移/兼容层：首次发现旧配置但新配置不存在时，提示或自动迁移到 `~/.agents/aemeath.json`；迁移完成前可保留只读兼容窗口，但新写入必须写到新路径。
- 项目指令读取从 `CLAUDE.md` 改为 `AGENTS.md`：全局 `~/.agents/AGENTS.md` + 当前工作目录 `{cwd}/AGENTS.md`。后续如需 Codex 的父目录链式读取，应单独拆 feature，避免本轮范围失控。
- Skills 读取收敛到 `~/.agents/skills` 与 `{cwd}/.agents/skills`；旧目录仅作为迁移来源，不应继续作为长期主路径。
- Worktree 处理应优先基于 git 信息确定 worktree checkout root，并在 cwd 与 checkout root 不同时定义清楚读取策略：至少保证当前 cwd 的 `{cwd}/AGENTS.md`、`{cwd}/.agents/skills` 可用；如后续引入 checkout root 级读取，必须做路径 canonicalize 与去重。

**涉及路径（预计）**：

- `shared/kernel/src/config/manager/mod.rs`：配置文件路径与加载层级调整。
- `shared/kernel/src/config/mod.rs`：配置 schema / 默认路径常量调整。
- `apps/cli/src/prompt.rs`：`CLAUDE.md` 读取迁移到 `AGENTS.md`。
- `shared/kernel/src/skill/loader.rs`：skill root 调整为 `~/.agents/skills` 与 `{cwd}/.agents/skills`。
- `shared/kernel/src/state/settings.rs`：若仍保留 settings 写入，需迁移到新配置根或明确废弃。
- `docs/feature/active.md`：实现进度同步更新。

**验收标准**：

1. 新安装用户默认读取 `~/.agents/aemeath.json`、`~/.agents/AGENTS.md`、`~/.agents/skills`。
2. 项目内默认读取 `{cwd}/AGENTS.md` 与 `{cwd}/.agents/skills`。
3. 旧 `~/.aemeath/config.json` / `.aemeath/config.json` / `.aemeath/skills` / `CLAUDE.md` 存在时，有清晰迁移或兼容提示，不静默丢配置。
4. 在 linked worktree 中启动时，项目级 AGENTS.md 与 skills 读取行为稳定、可预测，且不会重复加载同一路径。
5. 新写入的配置落到新模式，不再写回旧路径。

---

### #36 Multi-Agent 框架

详见 [设计文档](specs/036-multi-agent-framework.md)

---

### #33 优化 TaskListCreate / TaskListComplete 工具调用显示（已归档 2026-05-14）
用户确认完成。详见 `docs/feature/archived/033-task-list-display-optimization.md`。

### #30 Agent loop 收尾工作

**目标**：把 agent loop 的所有退出路径收敛到统一 finalize，避免正常结束、用户打断、API 错误、超时、达到 max turns 等分支各自手写清理逻辑，导致 task 状态、hook、日志、session 持久化行为不一致。

**与 #27/#29/#34 的关系**：
- #27 已先在 `Agent` tool 层补 taskId 状态桥接，但子代理内部取消/超时/API error 等结果仍应由统一 `AgentRunOutcome` 表达，避免继续依赖字符串结果判断。
- #29 已先通过 prompt 和工具描述强化主 agent 必须 `TaskUpdate`，后续 #30 只做收尾检查和日志提示，**不应**启发式自动替用户推进 task 状态。
- #34 已先引入 task batch summary、`TaskListCreate` / `TaskListComplete` 和 reminder 隔离；后续 #30 可在 finalize 中检查 active list 是否仍有 pending/in_progress，并决定记录摘要或提示关闭，不自动误归档。

**推荐 P0 范围**：
1. 新增 `AgentRunStatus` / `AgentRunOutcome`，覆盖 completed、cancelled、timed_out、api_error、max_turns。
2. 将 `CliAgentRunner::run_agent()` 多个 early return 收敛为统一 finalize 函数/guard。
3. finalize 统一执行：恢复 client 设置、调用 `SubagentStop` hook、写结构化日志摘要（status、turns、duration、role、model）。
4. 保持当前对外行为不变；不在本 feature 中自动完成 pending task。
5. 补单元测试覆盖各类 outcome 的 finalize 行为，至少覆盖正常完成、错误、max turns。

**后续 P1/P2 可选**：
- session 持久化接入 finalize。
- tool 资源释放/取消 token 清理接入 finalize。
- task list 收尾检查结果写入日志或 reminder 状态。

**明确不做**：
- 不自动把所有 pending task 标记 completed。
- 不按工具调用启发式猜测应该更新哪个 task。
- 不在当前 #27/#29/#34 修复分支里扩大实现该 feature；本轮只记录设计，后续单独在 #30 中统一完成。

---

**目标**：对齐 Claude Code 的 plugin/skill 加载机制，降低启动开销，支持 skill 包（如 superpowers）的自动发现和命名空间隔离。

**已完成的改动**：

1. **启动只读 frontmatter**：`parse_skill()` 不再读取 SKILL.md 的 body content，`Skill.content` 启动时为空字符串。新增 `read_skill_content()` 函数，由 Skill 工具调用时按需读取全文。
2. **Skill 工具延迟加载**：`aemeath-tools/src/skill_tool.rs` 调用时通过 `read_skill_content()` 从 `source_path` 读取完整内容返回给 LLM。
3. **命名空间前缀**：`load_skills_from_dir()` 自动识别 skill 包（含 `skills/` 子目录的目录），包内 skill 自动加 `<pkg_name>:` 前缀（如 `superpowers:brainstorming`），原始名保留为 alias。顶层 skill 和普通目录下的 skill 无前缀。
4. **HookJsonOutput 修复**：`aemeath-core/src/hook.rs` 的 `HookJsonOutput` 加了 `#[serde(rename_all = "camelCase")]`，修复 hook 脚本输出的 `additionalContext`（camelCase）无法被反序列化的问题。
5. **SessionStart hook 精简**：`superpowers-inject.sh` 从注入全文（~5500 字符/每次 API 调用）改为简短提示（113 字符），提醒 LLM 检查可用 skill 并通过 Skill 工具按需加载。
6. **Skill 目录扫描优化**：自动发现 skill 包内的 `skills/` 子目录，跳过 `agents/`、`.github/` 等无关目录。

**涉及路径**：
- `aemeath-core/src/skill.rs`（parse_skill 延迟加载、load_skills_from_dir 命名空间、read_skill_content）
- `aemeath-tools/src/skill_tool.rs`（Skill 工具调用时读取全文）
- `aemeath-core/src/hook.rs`（HookJsonOutput camelCase 支持）
- `~/.aemeath/hooks/superpowers-inject.sh`（SessionStart hook 精简）

**测试**：7 个单元测试覆盖命名空间前缀、延迟加载、忽略非 skills 目录、常规 skill 目录。

---

### #18 Task list 跨轮次 batch 机制

**目标**：Task list 跨轮次持久化，不再每次用户消息清空。通过 batch 机制区分不同 turn 的 task list，旧 batch 自动隐藏。

**已完成的改动**：

1. **移除自动清空**：`stream.rs` 不再在每次进入时调用 `_task_store.clear()`，task 跟随 session 生命周期。
2. **Batch ID 机制**：`Task` 新增 `batch` 字段，`TaskStore` 新增 `current_batch` 计数器。`create()` 时检测上一 batch 是否全部 completed/deleted，如果是则递增 batch。
3. **当前 batch 显示**：新增 `list_current_batch()` 方法，TUI 只显示最新 batch 的 task（含 Completed）。
4. **Completed 可见**：当前 batch 内 Completed 的 task 继续显示（✓ 图标），摘要行 `━━ Tasks: 3/5 ━━` 反映完成进度。

**涉及路径**：
- `aemeath-core/src/task.rs`（batch 字段、current_batch 计数器、list_current_batch）
- `aemeath-cli/src/tui/app/mod.rs`（update_task_status 使用 list_current_batch）
- `aemeath-cli/src/tui/app/stream.rs`（移除 clear 调用）

---

### #21 TUI 优化 Agent 调用输出展示

**目标**：优化 Agent 子任务每个 turn 的工具调用进度展示，避免只显示 `Read, Read, Grep` 这类无目标列表。

**已完成的改动**：

1. **结构化事件协议**：Agent progress 从 `Sender<String>` 升级为 `Sender<AgentProgressEvent>`，不再依赖 TUI 解析 `[Turn N]` 文本。
2. **工具调用摘要**：Agent runner 根据 tool call input 生成 `AgentToolCallProgress.summary`，例如 `Read ×2: src/lib.rs, src/main.rs | Grep: "AgentProgress" in src`。
3. **同工具分组**：TUI 根据结构化 calls 按工具名合并，并显示调用次数；turn/sequence 仅用于内部定位，默认不展示。
4. **当前进度单行更新**：同一个 Agent tool 的 `ToolCalls` 进度只保留一行，新事件替换旧行，不重复刷屏。
5. **兼容保留**：`AgentProgressKind::Message` 用于普通文本 progress，仍按原逻辑追加和去重。

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（Agent tool call progress 摘要生成）
- `aemeath-cli/src/tui/output_area/tool_display.rs`（同 turn progress 替换）

**测试**：新增单元测试覆盖结构化事件构造、目标摘要生成、同 Agent 当前进度替换、不同 Agent 互不覆盖、普通 Message progress 兼容。

---

### #23 TUI 字符串/切片安全索引收口

**目标**：把 TUI 路径中"按字符索引、按字节切片、按宽度截断、按显示列号定位"等容易越界的操作收口到一个统一的工具模块，业务路径全部走该模块的 API，禁止直接 `chars[from..to]`、`s[i..j]`、`chars().nth(n)`、`text.len()` 当字符长度等高风险写法。配合单元测试覆盖边界条件，根治"TUI streaming/选中/复制/渲染"路径反复出现的越界 panic。

**已完成的改动**：

1. 新增 `aemeath-cli/src/tui/safe_text.rs`，统一提供 panic-free 字符范围、字符串切片、显示宽度截断、列号转换、split index clamp，并补充 `str_display_width`。
2. `selection.rs` 的复制选中文本路径迁移到 `safe_char_slice` / `safe_str_slice_by_char`。
3. `output_area/mod.rs` 的 `screen_line_map.split_off` 迁移到 `clamp_split_index`。
4. `output_area/display.rs` 的宽度截断和列号转换委托给 `safe_text`。
5. `input_area.rs` 自动换行后缀提取改为 `safe_char_slice`。
6. 新增 `scripts/check-unsafe-text-ops.sh` 门禁，阻止 TUI 业务路径重新出现高风险切片/索引写法，当前 guard findings 已清零。
7. 补充 safe_text/display 相关边界测试，以及 markdown CJK link 渲染测试，覆盖 CJK 宽字符与安全索引场景。

**为什么要做（已踩过的坑）**：

| Bug | 路径 | 越界类型 |
|-----|------|----------|
| #4（archived）| Output area 渲染 | `screen_line_map` 索引越界 / CharIdx 运算溢出 / wrap 计算与 screen_line_map 不一致 |
| #5（archived）| 鼠标选中位置 | `screen_line_map` 滚动裁剪未同步 |
| #8（archived）| 字符串索引 | 字节/字符长度混淆 |
| #16（archived）| `/resume` 列表 CJK | `chars().nth(x_usize)` 用屏幕列号当字符索引 + `text.len()` 当显示宽度 |
| streaming.rs | thinking block | UTF-8 字节 boundary panic（4636/4636 修复） |
| #28（已代码修复，待确认归档）| 复制选中文本 | `chars[from..to]` 中 `from` 未做 `chars.len()` 裁剪；代码层已修复，仍在 `docs/bug/active.md` 等待用户确认归档 |

每次出 bug 各自修各自的，没有共享防御层 → 同样的"index 越界 / 字节-字符混淆 / CJK 宽字符当 1 列"模式会换个文件再出现。

**实际设计 / API**：

#### 1. 新增 `aemeath-cli/src/tui/safe_text.rs` 模块

实际 API（全部 panic-free，越界返回空切片、空字符串、`None` 或 clamp 后的位置）：

```rust
/// 按字符（不是字节）安全切片，from/to 都被 clamp 到 chars 长度，
/// 如 from >= to 视为空。
pub fn safe_char_slice(chars: &[char], from: usize, to: usize) -> &[char];

/// 按字符 index 安全取一个字符。
pub fn safe_char_at(s: &str, idx: usize) -> Option<char>;

/// 按字符 range 进行 clamp；空区间或反向区间返回 None。
pub fn clamp_char_range(from: usize, to: usize, chars_len: usize) -> Option<Range<usize>>;

/// 按字符范围安全切 `&str`，返回借用切片而非新分配 String。
pub fn safe_str_slice_by_char(s: &str, from: usize, to: usize) -> &str;

/// 按 unicode 显示宽度截断（CJK 占 2 列），返回 (substring, width_used)。
pub fn truncate_unicode_width(s: &str, max_cols: usize) -> (&str, usize);

/// 计算 unicode 显示宽度。
pub fn str_display_width(s: &str) -> usize;

/// 按 unicode 显示宽度从屏幕列号定位字符索引（鼠标点击/选中用）。
pub fn col_to_char_idx(s: &str, col: usize) -> usize;

/// clamp Vec::split_off 的 split index。
pub fn clamp_split_index(offset: usize, len: usize) -> usize;
```

实际实现偏差：
- `safe_str_slice_by_char` 返回 `&str`，不是早期草案里的 `String`。
- 未引入 `SafeChars` 包装类型；当前以函数式 helper 收口高风险操作。
- 未保留 `safe_byte_slice` / `safe_char_truncate` 命名；对应能力由 `safe_str_slice_by_char`、`truncate_unicode_width` 覆盖。

#### 2. 业务路径迁移范围

- `selection.rs::get_selected_text` → 改用 `safe_char_slice` / `safe_str_slice_by_char`。
- `output_area/mod.rs` 中 `screen_line_map.split_off` → 改用 `clamp_split_index`。
- `output_area/display.rs` 的宽度截断、显示宽度计算、列号转换 → 改用 `truncate_unicode_width` / `str_display_width` / `col_to_char_idx`。
- `input_area.rs` 自动换行后缀提取 → 改用 `safe_char_slice`。
- `markdown.rs` link 解析仍保留 `.get(byte_range)`：byte range 来自 `find()`，由 `get()` 验证 UTF-8 boundary；这些位置通过 `allow unsafe_text_op` 注释白名单，不代表全部直接切片都已消除。
- `streaming.rs` 继续使用 `aemeath-core/src/string_idx/` 的 `ByteIdx` / `StrSlice`，因为 thinking block 解析是字节级协议/标签扫描操作，不适合强行改成 TUI 字符切片 helper。

#### 3. caveat / 边界说明

- `safe_text` 是 TUI 字符索引 / 显示宽度安全层；`aemeath-core::string_idx` 是字节 / 字符强类型索引层。两者当前并存，未来可评估统一或抽象边界。
- `safe_text` 收口的是 TUI 业务路径里的高风险字符切片、显示宽度截断、列号换算和 split index；并非要求所有字节级解析都改成字符级 API。
- 验证 caveat：`cargo fmt --check` 仍有与 #23 无关的 `aemeath-core` 预存格式差异，因此本次文档 follow-up 以 `git diff --check` 作为必跑验证。

#### 4. lint / 测试门禁

- `safe_text` 模块每个函数至少 5 个测试：空输入、from=to、from>to、from>len、to>len、CJK 宽字符
- 加 clippy 自定义 lint 或 grep 检查脚本：`tui/` 目录下出现 `chars\[.+\.\..+\]` / `\.chars\(\)\.nth\(` / `s\[\d+\.\.\d+\]` 时 fail，强制走 `safe_text`
- CI 增加 panic stress test：构造各种边界输入（空字符串、纯 CJK、超长行、滚动裁剪后选中等）

#### 5. 实施分两阶段

**Phase 1（先止血）**：
- 修复当前 #28（最小修复 + 加 `if from > to { continue; }`）
- 新建 `safe_text.rs` 骨架，把 `safe_char_slice` / `clamp_char_range` 实现 + 测试
- `selection.rs` 迁移到新 API，作为示范

**Phase 2（全面收口）**：
- 把所有 TUI 路径的字符串索引/切片改为 `safe_text` API
- 加 grep 门禁脚本（CI 跑）
- 补 panic stress test

**为什么不简单"加 if 保护"了事**：
- 防御代码会被反复忘记加（#28 就是 #5 / #8 修过同类问题后又出现）
- 类型层面表达不出"这是字符 index 还是字节 index"，只能靠人脑追
- 单点保护无法覆盖未来新增的索引点

**涉及路径**：
- 新增：`aemeath-cli/src/tui/safe_text.rs`
- 重构：`aemeath-cli/src/tui/output_area/selection.rs` / `markdown.rs` / `streaming.rs` / `mod.rs`
- 重构：`aemeath-cli/src/tui/input_area.rs`
- 新增：`scripts/check-unsafe-text-ops.sh`（grep 门禁）
- CI：`.github/workflows/` 或本地 `Justfile` / `Makefile` 加调用

**关联**：
- Bug #4 / #5 / #8 / #16 / #28（全部是字符串/索引越界类）
- streaming.rs UTF-8 boundary 修复（已修，可作为 case 1 验证）

**开放问题**：
- `safe_text` 放在 `aemeath-cli/src/tui/` 还是提升到 `aemeath-core/src/utils/`（如果 core 也有类似需求）
- 是否引入 `unicode-segmentation` crate（按 grapheme cluster 而非 char 计算，更贴合"用户感知字符"）
- grep 门禁误报怎么处理（比如测试文件、`safe_text` 自己内部使用切片、经 `.get(byte_range)` 验证的字节范围）—— 可以加 `allow unsafe_text_op` 注释跳过

---

### #24 Spinner 下方 task list 限量显示（最多 7 条）

**目标**：当 task 数量较多（10+）时，spinner 下方的 task list 占据屏幕大量空间，把主对话/输出挤到看不见。改为按"前后文相关性"窗口化显示，固定上限 7 条左右，让用户能快速看到"刚做完什么、正在做什么、接下来做什么"，而不是被一长串 ☐ pending 淹没。

**当前现状**（`aemeath-cli/src/tui/app/mod.rs:639-672`）：
- `update_task_status()` 把当前 batch 内**所有**非 deleted 的 task 全部 push 到 `task_status_lines`
- 摘要行 `━━ Tasks: x/y ━━` + 每个 task 一行（`✓` / `■` / `□` + 编号 + 标题 + owner）
- 7 条 task → 占 8 行；20 条 task → 占 21 行；输出区域所剩无几

**预期窗口化策略**：

显示顺序（completed → in_progress → pending）：

```
━━ Tasks: 3/15 ━━              ← 摘要行始终反映全量
✓ #3 拆分 mod.rs                ← 上一条 completed，仅显示 1 条
■ #4 拆分 hook.rs               ← 所有 in_progress 全显示
■ #5 拆分 task.rs
□ #6 拆分 scheduler.rs           ← 后续 pending，按余量填充
□ #7 拆分 state.rs
□ #8 拆分 guidance.rs
… +7 more pending               ← 折叠提示
```

具体规则：
1. **摘要行保持全量**：`Tasks: x/y` 不受窗口限制
2. **窗口按优先级填充**（默认上限 7 条）：
   - 上一条 completed（最近完成的 1 条）
   - 所有 in_progress（一般 1~3 条）
   - 后续 pending 按 task id 升序填充剩余配额
3. **超出部分**：`… +N more pending` 单行折叠提示
4. **没有 in_progress 时**：第一条 pending 视为"接下来要做"，显示前 6 条 + `… +N more`
5. **全部 completed 时**：显示最后 5~7 条 completed
6. **空 task list**：不显示窗口

**配置项**：
```json
{
  "ui": {
    "task_list": {
      "max_lines": 7,
      "show_last_completed": 1,
      "fold_hint_format": "… +{n} more {status}"
    }
  }
}
```

**实施分解**：
1. `update_task_status()` 增加窗口化逻辑（分桶 → 按规则取窗口 + 折叠提示）
2. 拆出纯函数 `build_task_window(tasks, max_lines, last_completed_count) -> Vec<String>`，单独测试
3. 单元测试覆盖：0 / 1 / max / max+1 / 远超 max 各档；全 pending / 全 in_progress / 全 completed / 混合；in_progress 数量超过 max 时 pending 全部隐藏

**涉及路径**：
- `aemeath-cli/src/tui/app/mod.rs`（`update_task_status` 窗口化）
- 新增：`aemeath-cli/src/tui/app/task_window.rs`（纯函数 + 单元测试）
- `aemeath-core/src/config/`（`ui.task_list.max_lines` 等配置字段）

**关联**：
- Feature #18（task batch 机制）—— 在 batch 之上做窗口化，正交
- Feature #25（task 跨轮次生命周期）—— 限量解决"显示太多"，#25 解决"显示太久"
- Bug #29（主 agent task 不更新）—— 修复后窗口化逻辑会更频繁触发

**开放问题**：
- max 默认 7 是否合适？高分屏 vs 小屏权衡
- 折叠提示是否可点击展开？留作后续 polish
- 全部 completed 时显示 last 5 vs 折叠成 `Tasks: 15/15 ✓ all done`

---

### #25 Task list 跨轮次生命周期策略

**目标**：在同一 session 内，处理"上一轮的 task list 在新对话开始时还会显示"的问题。当前 Feature #18 的 batch 机制只是"新 turn 切到新 batch"，但没规定旧 batch 怎么收尾、怎么提示用户、何时归档。本 feature 补齐三种典型场景的明确策略。

**用户痛点**：「同一个 session 中，新的对话开始时还会显示上次的 task list」

具体场景：
- 上轮 task 全做完了 → 新对话开头还看到一长串 ✓，没价值还占地方
- 上轮 task 没做完用户主动问别的 → 旧 task 状态尴尬，是继续？是放弃？没出路
- 上轮 task 多轮没推进（用户跑题、agent 偏题）→ 沉默积压在 batch 里没人理

---

#### 场景 1：上一轮 task 全部完成

**触发**：上一 batch 内所有 task 都是 `Completed`（或 `Cancelled`），且用户输入新对话。

**策略**：
- 新 turn 开始时检测上一 batch 是否 100% 完成
- 是 → 自动隐藏旧 batch（保留在 TaskStore 历史中，可通过 `/task history` 回看）
- 显示一行 toast（1~2 秒）：`✓ 上一组 task 已完成（5/5）`
- 新 batch 在用户新 task 出现时才创建

#### 场景 2：上一轮 task 中断、用户开新话题

**触发**：上一 batch 内有 `InProgress` / `Pending` task，用户输入了一条**与未完成 task 主题不相关**的新消息。

**判断"主题不相关"**（启发式，不调 LLM）：
- 关键词重叠率低（task 标题与新消息分词后 cosine 相似度 < 0.2）
- 或：用户消息以 `/` 开头（slash 命令通常是控制流）
- 或：消息含明显切换语气（"先放一下"、"换个话题"、"另外"、"对了"等）

**策略**：弹 inline 提示（不阻塞输入）：
```
⚠ 上一组 task 还有 3 项未完成（#4 #5 #6），是否：
  [c] 继续上次任务   [p] 暂存稍后回来   [d] 丢弃这组任务
  （直接回车默认 [p] 暂存）
```

- `[c]` 继续：保留旧 batch 为当前 batch，新消息作为"补充指令"附加
- `[p]` 暂存：旧 batch 标记为 `paused`，从视图隐藏，可 `/task resume <batch_id>` 恢复
- `[d]` 丢弃：旧未完成全部 `Cancelled`，归档

#### 场景 3：旧 task 沉默积压

**触发**：某 batch 内有 `InProgress` / `Pending`，连续 N 轮（默认 3）用户对话没推进它（没 TaskUpdate 涉及它，没 tool call 修改了 task 涉及的文件等）。

**策略**：
```
ℹ 以下 task 已沉默 3 轮：
  ■ #4 拆分 hook.rs
  □ #5 拆分 task.rs
  仍要继续吗？回 /task keep 保留 / /task drop 丢弃 / /task pause 暂存
```

- 不打断当前对话，提示出现一次后不重复（直到再过 N 轮或用户回复）
- 提示文本不入 LLM context（仅 UI 可见，避免污染对话）

---

**配置项**：
```json
{
  "ui": {
    "task_lifecycle": {
      "auto_clear_completed_on_new_turn": true,
      "interrupt_prompt_enabled": true,
      "interrupt_default_action": "pause",
      "stale_remind_after_turns": 3,
      "stale_remind_repeat_interval": 5
    }
  }
}
```

**新增命令 / 状态**：
- `Task.batch_status`: `Active | Paused | Archived`
- `/task pause` —— 当前 batch → Paused
- `/task resume [batch_id]` —— 恢复指定 batch
- `/task keep` —— 沉默提示中确认保留
- `/task drop` —— 当前未完成全部 Cancelled
- `/task history` —— 列出本 session 内所有 batch

**实施分解**：
1. **TaskStore 扩展**：`batch_status` 字段、`Batch` 结构（id / created_at / last_active_turn / status）
2. **场景 1 检测**：`update_task_status()` 调用前 check 上一 batch → 全 completed 隐藏 + toast
3. **场景 2 启发式 + 提示 UI**：新增 `topic_relevance_check(prev_tasks, new_message)`，触发时 push `UiEvent::TaskInterruptPrompt`
4. **场景 3 沉默检测**：turn 结束 hook 中递增每个未完成 task 的 `silence_turns`；达阈值 push `UiEvent::TaskStaleReminder`
5. **命令实现**：`commands/task.rs` 增加 pause / resume / keep / drop / history

**涉及路径**：
- `aemeath-core/src/task.rs`（Batch 结构、batch_status、silence_turns）
- 新增：`aemeath-core/src/task/lifecycle.rs`（场景判定纯逻辑 + 单元测试）
- `aemeath-cli/src/tui/app/mod.rs`（update_task_status 触发场景检测）
- `aemeath-cli/src/tui/app/update.rs`（处理 TaskInterruptPrompt / TaskStaleReminder UI 事件）
- 新增：`aemeath-core/src/command/commands/task.rs`（pause / resume / keep / drop / history）
- `aemeath-core/src/config/`（`ui.task_lifecycle` 配置）

**关联**：
- Feature #18（task batch 机制）—— 本 feature 在 batch 之上加生命周期状态
- Feature #24（task list 限量显示）—— 限量解决"显示太多"，本 feature 解决"显示太久"
- Bug #29（主 agent task 不更新）—— 修好后场景 1/3 才能准确触发

**开放问题**：
- 主题相关性判断用关键词重叠率够吗？误判率 vs 复杂度（要不要直接调 LLM？太重）
- 场景 2 提示 inline vs ask_user？倾向 inline，但要确认默认 `[p] pause` 不会让用户莫名其妙
- batch 归档保留多久？session 结束时持久化，session resume 时是否复活？
- `/task history` 输出格式：表格 vs 树形？

---

### #27 日志分化：input.log / output.log / tool.log

**目标**：在 Feature 27（日志系统职责分层）基础上，将 agent 交互日志从 `aemeath.log` 中分离为三个 JSON 格式文件，`aemeath.log` 收窄为应用诊断日志。

**文件布局**：
- `~/.aemeath/logs/input.log` — LLM 输入快照（新增 messages + system_blocks 摘要）
- `~/.aemeath/logs/output.log` — LLM 完整输出（content blocks + token 用量 + 耗时）
- `~/.aemeath/logs/tool.log` — 工具调用请求 + 结果（完整参数和输出）
- `~/.aemeath/logs/aemeath.log` — 应用诊断日志（MCP、hook、session、技能、UI 调试）
- `~/.aemeath/logs/panic.log` — panic 信息

**JSON 统一格式**：
```json
{"ts":"2026-05-09T10:30:00+08:00","session":"abc123","turn":3,"role":"searcher","model":"gpt-5.5","type":"input","data":{...}}
```
- `role` 为 agent role（主 agent 固定 `"default"`），`type` 为 `"input"` / `"output"` / `"tool_call"` / `"tool_result"`

**移除的旧代码**：`log_agent_loop_event()`、`log_llm_request_messages()`、`log_tool_result_event()`

**新增组件**：`JsonLogger`（`aemeath-core/src/logging.rs`），直接 `BufWriter<File>` 写入，不经 `env_logger`

**配置**：`logging.logs_dir`（默认 `~/.aemeath/logs/`）、`logging.role_logs_enabled`（默认 `true`）

**当前状态**：修复中；已修复 TUI `input.log` 只写 injected messages 导致用户输入缺失的问题，并补齐 REPL 的 input/output/tool 分化日志写入。验证：`cargo test -p aemeath-cli test_logged_input_messages`、`cargo test -p aemeath-core test_json_logger_log_input_happy_path_writes_user_message`、`cargo check -p aemeath-cli` 通过（仅既有 `Cmd::Batch` dead_code warning）。

详见 [design spec](../superpowers/specs/2026-05-09-log-split-design.md)

**实施完成（2026-05-11）**：
- `aemeath-core/src/logging.rs`：新增 `LogFile::Input/Output/Tool`、`JsonLogger` struct（含 `log_input/log_output/log_tool_call/log_tool_result`）、`logs_base_dir()`、日志轮转逻辑
- `aemeath-core/src/config/logging.rs`：新增 `logs_dir`、`role_logs_enabled` 配置字段
- `aemeath-cli/src/main.rs`：全局 `JSON_LOGGER` OnceLock + `get_json_logger()` 访问器，在 `init_logging()` 中初始化
- `aemeath-cli/src/tui/app/stream.rs`：`log_agent_loop_event` / `log_llm_request_messages` 内部增加 JsonLogger 写入（input/output/tool call/tool result）；`log_tool_result_event` 委托到 `log_agent_loop_event`
- `aemeath-cli/src/agent_runner.rs`：子 agent 的 `log_request_messages` 闭包 + LLM 响应处 + tool call 批量处 + tool result 处均已接入 JsonLogger
- 测试：`aemeath-core` 368 测试全部通过

---

### #28 MCP 系统完善

**目标**：完善 MCP 系统的配置、连接管理、工具发现注册和 CLI 操作路径，覆盖本轮 P0+P1 基础能力；P2 不在本轮范围，待后续单独规划。

#### 本轮已完成的 P0+P1 改动

- `McpServerConfig` 支持 stdio 可用配置；SSE/Streamable HTTP 仅完成配置解析与 URL 安全校验，传输实现仍为占位存根；header 脱敏已完成。
- McpConnectionManager 接入启动加载路径，统一管理连接/tool 发现/注册。
- ToolRegistry 注销与查询接口。
- Manager tool diff、snapshot refresh、`health_check_once` API 和状态转换；未暗示后台定时 health check loop 已启动。
- `/mcp` 独立命令模块已落地，支持 list/tools/restart/add/remove 的解析与预览输出；实际运行时操作待 runtime bridge 接入。
- MCP tool result 默认 1MB 响应大小限制。

#### 已知限制

- SSE 传输存在可靠性问题：z.ai 等远程 MCP server 的 SSE response 在 tools/list 时经常出现超时或不完整（2890 bytes 后 SSE stream 停止发送，缺少 `\n\n` 终结符）。已尝试 POST body 消费、独立 client、incomplete event fallback 等方案，均未根本解决。
- **MCP 加载已从 TUI 启动流程中禁用**（`run_orchestration.rs` 中 `spawn_mcp_connect` 调用已注释），避免启动时因 MCP 连接超时阻塞。
- `/mcp` restart/add/remove/list/tools 暂未接入 runtime manager bridge，仅返回状态/预览提示。
- Streamable HTTP 传输未实现。
- 健康检查已有单次 API，后台定时 loop 尚未接入。

#### 待修复方向

- SSE stream 可靠性：考虑改用 Streamable HTTP 传输（单 POST 请求返回完整 JSON-RPC response，不依赖 SSE event stream）。
- 或者在 SSE transport 中增加更健壮的重试机制和超时策略。
- 需要测试更多 MCP SSE server 以确认是 z.ai 特定问题还是通用问题。

#### 关联

- Feature #28 MCP P0+P1 实施任务 1-9 已完成并 review 通过
- 用户确认后再归档；确认前保留在 active.md

---

### #8 Memory 系统

**目标**：跨会话持久化记忆，让 agent 在不同会话间积累项目知识、用户偏好和决策上下文，避免每次从零开始。

**存储设计**：

```
~/.aemeath/memory/
├── _global.json          # 全局记忆（跨项目）
├── <project-hash>/       # 项目级记忆
│   ├── _index.json       # 记忆索引（id → metadata）
│   ├── <id>.json         # 单条记忆
│   └── _archive/         # 过期/合并后的归档
```

**记忆条目结构**：

```rust
struct MemoryEntry {
    id: String,             // UUIDv7
    category: MemoryCategory,
    content: String,        // 记忆正文
    source: String,         // 来源：session id / reflection / user
    project: Option<String>,// 项目标识（None = 全局）
    relevance_tags: Vec<String>,  // 检索标签
    created_at: u64,
    accessed_at: u64,       // 最后一次被检索注入的时间
    access_count: u32,      // 被检索次数（用于优先级排序）
    expires_at: Option<u64>,// 过期时间（None = 永久）
}
```

**分类**：

```rust
enum MemoryCategory {
    ProjectStructure,  // 项目架构、文件组织
    Decision,          // 重要设计决策及其理由
    Preference,        // 用户偏好（语言、风格、框架选择等）
    Pattern,           // 项目特定模式（命名规范、错误处理方式）
    Pitfall,           // 已知坑点/踩坑记录
    Context,           // 一般上下文知识
}
```

**写入时机**（通过 Hook 触发）：

| 时机 | HookEvent | 写入策略 |
|------|-----------|---------|
| 会话结束时 | `SessionEnd` | LLM 总结本会话关键决策和发现，写入 memory |
| 压缩后 | `PostCompact` | 提取被压缩掉的重要上下文到 memory |
| 用户主动 | `/memory add <content>` 命令 | 直接写入 |
| 反思系统 | `ReflectionGenerated`（新事件） | 反思结果写入 |

**检索注入**（System Prompt 构建阶段）：

1. `build_system_prompt_parts()` 中新增 memory 检索步骤
2. 基于当前 cwd 定位项目 memory 目录
3. 按 `access_count` + `created_at` 加权排序，取 top-N（默认 10 条）
4. 注入到 system prompt 的 dynamic_part 中：
   ```
   # Project Memory
   - [Decision] 使用 tokio channel 而非 mpsc，因为需要跨 async task 通信
   - [Pattern] 错误处理统一用 AemeathError，thiserror derive
   - [Pitfall] bash.rs 中 check_command_safety 不受 allow_all 控制，已修复
   ```
5. 更新被注入条目的 `accessed_at` 和 `access_count`

**新增模块**：

- `aemeath-core/src/memory.rs` — MemoryStore（CRUD + 索引 + 检索 + 淘汰）
- `aemeath-core/src/command/commands/memory.rs` — `/memory` 命令

**新增命令**：

| 命令 | 说明 |
|------|------|
| `/memory` | 显示当前项目的 memory 摘要 |
| `/memory add <content>` | 添加一条记忆 |
| `/memory search <query>` | 搜索记忆 |
| `/memory delete <id>` | 删除一条记忆 |
| `/memory clear` | 清空项目记忆 |

**淘汰策略**：
- 单条记忆超过 90 天未被访问（`accessed_at`）且 `access_count < 3` → 归档
- 单项目记忆超过 100 条 → 触发合并：将相近 tag 的记忆用 LLM 合并为一条摘要
- 归档文件不删除，可通过 `/memory search` 搜索

**配置**（`config.json`）：

```json
{
  "memory": {
    "enabled": true,
    "max_entries_per_project": 100,
    "max_inject_count": 10,
    "auto_summary_on_session_end": true,
    "archive_after_days": 90
  }
}
```

**依赖**：无外部依赖，纯文件系统存储 + JSON 序列化。

#### 完成度评估（2026-05-19）：待确认

**已实现**：

| 组件 | 位置 |
|------|------|
| 存储层 `MemoryStore`（CRUD + 搜索 + 去重 + 归档） | `memory/store.rs` |
| `MemoryEntry` 结构（id/layer/category/tags/pin/ttl/outdated/access_count） | `memory/entry.rs` |
| 分类：Fact/Decision/Preference/Pattern/Pitfall | `memory/entry.rs` |
| 两层存储：Global + Project | `memory/store.rs` |
| 去重（Jaccard 相似度） | `memory/dedup.rs` |
| 评分（injection_score + eviction_score） | `memory/scoring.rs` |
| System Prompt 注入（`top_for_inject` → `# Project Memory`，受 `MemoryConfig.max_inject_count` 控制） | `prompt.rs` |
| `/memory` 命令（add/delete/pin/search/compact/stats） | `command/commands/memory.rs` |
| `MemoryTool`（LLM 可通过 tool call 操作 memory） | `memory_tool.rs` |
| `SessionReminders`（会话级提醒） | `memory/session_reminder.rs` |
| TUI/REPL 对话结束后展示 session reminder recap，`/clear` 清空 | `tui/app/update/reminder_recap.rs`、`repl/mod.rs` |
| 配置（`MemoryConfig` + `ReflectionConfig`） | `config/memory.rs` |

**暂缓/未实现**：

| 需求 | 说明 |
|------|------|
| `auto_summary_on_session_end` | 配置项存在但无调用代码，SessionEnd 时没有 LLM 总结写入 memory |
| `ReflectionGenerated` Hook 事件 | spec 中提到的 hook 事件不在 `HookEvent` 枚举中 |
| PostCompact 提取记忆 | PostCompact hook 只记录日志，没有提取被压缩内容到 memory |
| 淘汰策略定时触发 | 有 `compact()` + `eviction_candidates()`，但无定时触发逻辑，`archive_after_days` 配置项不在 `MemoryConfig` 中 |
| LLM 合并相近记忆 | spec 中"超过 100 条时用 LLM 合并"未落地 |
| SessionReminders 持久化 | `SessionReminders` 仅内存态，不写入文件，会话结束即丢失 |

---

### #9 反思系统（初版设计）

**目标**：在关键节点自动触发反思，让 agent 从过去的行为中提炼经验，写入 Memory 系统，避免重复犯错。

**反思触发时机**：

| 触发点 | 条件 | 反思内容 |
|--------|------|---------|
| 连续工具失败 | 同一 turn 内 ≥2 次工具调用失败 | 失败原因分析 + 正确做法 |
| 会话结束 | `SessionEnd` hook | 整体会话总结 + 关键决策 |
| 子代理结束 | `SubagentStop` hook | 子代理执行摘要 |
| 用户中断 | 用户按 Escape 取消 | 当前进度快照 + 未完成原因 |
| 重试后成功 | API 错误后重试成功 | 错误类型 + 重试策略有效性 |

**反思流程**：

```
触发条件满足
  → 构造反思 prompt（含近期对话片段）
  → 调用 LLM 生成反思摘要（用轻量模型，如 deepseek-chat）
  → 解析反思结果为结构化 MemoryEntry
  → 写入 MemoryStore
```

**反思 Prompt 模板**：

```
你是一个反思助手。请分析以下对话片段，提炼出对未来会话有价值的信息。

要求：
1. 只记录客观事实和有效经验，不要记录临时状态
2. 每条不超过 200 字
3. 标注分类：Decision / Pattern / Pitfall / Preference

对话片段：
{recent_messages}

请输出 JSON 数组：
[{"category": "...", "content": "...", "tags": ["..."]}]
```

**反思结果结构**：

```rust
struct ReflectionResult {
    entries: Vec<ReflectionEntry>,
}

struct ReflectionEntry {
    category: MemoryCategory,
    content: String,
    tags: Vec<String>,
}
```

**实现策略**：

1. 反思调用使用**独立轻量 LLM 调用**（非主对话），避免干扰上下文
2. 反思在后台异步执行（tokio::spawn），不阻塞主循环
3. 反思结果静默写入 MemoryStore，不显示在对话中
4. 仅在 `memory.enabled = true` 且有有效反思内容时触发

**配置**（`config.json`）：

```json
{
  "reflection": {
    "enabled": true,
    "model": "deepseek/deepseek-chat",
    "max_entries_per_reflection": 3,
    "min_turns_for_session_summary": 5,
    "consecutive_failures_threshold": 2
  }
}
```

**依赖**：
- Feature #8（Memory 系统）— 反思结果写入 MemoryStore
- Hook 系统 — 通过 HookEvent 触发反思

**实施阶段**：
- P0：会话结束反思（最核心，收益最大）
- P1：连续工具失败反思
- P2：子代理反思、用户中断反思

**开放问题**：
- 反思是否消耗当前 session 的 model 调用，还是用独立的轻量 model（成本权衡）
- 反思失败（如 LLM 返回空）时是否静默丢弃 vs 提示用户
- Memory 容量上限策略：何时压缩 / 淘汰旧反思

---

### #9 反思系统（实施版）

#### 完成度评估（2026-05-19）：待确认

**已实现**：

| 组件 | 位置 |
|------|------|
| `ReflectionEngine`（解析 JSON、格式化输出） | `reflection/` |
| Prompt 模板（偏差检测 + 建议记忆 + 过时记忆） | `reflection/prompt.rs` |
| `ReflectionOutput` 结构 | `reflection/types.rs` |
| `/reflect` 命令（后台异步触发 + apply/stats/history 子命令占位） | `tui/app/slash/reflection.rs` |
| pending 建议与 `/reflect apply` 写入 MemoryStore | `tui/app/slash/reflection.rs` |
| `auto_apply_suggestions` 自动应用 suggested memories 与 outdated markers | `tui/app/update/ui_event.rs` |
| `apply_outdated()` 标记过时记忆 | `reflection/apply.rs` |
| `recent_messages_summary()` 提取对话摘要 | `reflection/format.rs` |
| 自动 N 轮触发（`reflection.interval_turns`，0 表示禁用自动触发；后台静默执行，不锁住输入） | `tui/app/slash/reflection.rs`、`tui/app/update/ui_event.rs` |
| 配置（`ReflectionConfig`） | `config/memory.rs` |

**暂缓/未实现**：

| 需求 | 说明 |
|------|------|
| 连续工具失败触发反思 | spec P1 |
| SessionEnd 反思 | agent loop 结束时暂不触发反思 |
| SubagentStop 反思 | spec P2 |
| 用户中断反思 | spec P2 |
| 错误恢复后反思 | spec P2 |
| 独立 reflection model | `ReflectionConfig.model` 字段保留，但本轮使用当前 client，避免新增模型路由复杂度 |
| PostCompact 后反思 | 暂缓，避免压缩后上下文损失 |

**行为取舍**：

| 原 spec | 本轮实现 |
|-----------|----------|
| 反思结果静默写入 MemoryStore，不显示在对话中 | TUI 中展示 Reflection 摘要；若 `auto_apply_suggestions = false`，提示用户运行 `/reflect apply` |
| 使用独立轻量模型（成本优化） | 使用当前默认模型 |
| 后台异步执行（不阻塞主循环） | `/reflect` 与自动 N 轮触发均通过后台任务回传 `ReflectionDone` |

**目标**：在关键节点（任务完成、Stop、错误恢复后、用户显式触发）执行反思流程，对最近的行为、决策、失败、用户反馈做结构化总结，将有价值的经验写入 Memory 系统（#8），让 agent 在未来会话中能够基于历史经验做更好的决策。

**依赖**：Feature #8 Memory 系统（反思的输出目标）

**设计草案**：

#### 触发时机
- **任务完成后**：TaskUpdate 将 task 置为 `completed` 时，对该 task 的执行过程做总结
- **Stop 事件**：会话结束 / agent 主动停止时，对整段会话做反思
- **错误恢复后**：tool call 失败 → 修复 → 成功 的链路上，提炼"哪种修复有效"
- **用户显式触发**：`/reflect` slash 命令，对最近 N 轮做即时反思
- **PostCompact 钩子**：上下文压缩前抢救关键经验

#### 反思维度
- **成功模式**：哪些工具组合 / 推理路径达成了目标
- **失败教训**：哪些假设错了、哪些 tool call 走了弯路
- **用户偏好**：用户在本次会话中的纠正、拒绝、确认（参考 superpowers `feedback` 类型）
- **未解决问题**：本次会话中悬而未决的事项（提示下次继续）

#### 输出格式
- 结构化条目（type / title / body / scope），写入 Memory 系统
- 每条反思 must 标注来源会话 ID + 时间戳，便于追溯
- 避免重复：写入前检索 Memory，相似条目优先 update 而非 insert

#### 实施阶段
1. **Phase 1**：实现 `/reflect` 命令 + 基础反思 prompt 模板（依赖 #8 已落地的 Memory 接口）
2. **Phase 2**：接入 Stop / TaskUpdate(completed) 自动触发
3. **Phase 3**：错误恢复链路反思 + PostCompact 钩子

**涉及路径**（待实施）：
- `aemeath-core/src/reflection/` — 反思引擎、prompt 模板、写入策略
- `aemeath-core/src/command/commands/reflect.rs` — `/reflect` 命令
- `aemeath-cli/src/tui/app/update.rs` — Stop 事件触发钩子
- `aemeath-cli/src/tui/app/stream.rs` — TaskUpdate / 错误恢复触发钩子

---

### #12 Input Queue 双层循环优化

**目标**：让 LLM 在一个 user turn 内部（API call → tool calls → 下一次 API call → tool calls ...）的细粒度节点上**主动检查 input queue**，把用户排队的反馈尽早注入对话流，而不是等整个 agent loop 跑完才"看到"用户的新输入。让用户感受到"agent 听得见我"，而不是"agent 必须把这一摊事干完才理我"。

**背景**：
- Feature #7 已实现多消息 input queue（VecDeque），processing 期间用户可连续排队多条输入
- 当前消费时机是**外层 user-turn 循环**末尾——agent 完成所有 tool call、模型给出最终 stop_reason=EndTurn 后才 pop 一条 queue 进入下一轮
- 痛点：当 agent 进入长链路（连续 N 个 tool call、长 thinking、子 agent 嵌套）时，用户中途看到方向跑偏想纠正，目前必须等整轮结束才能让 agent 看到——体验上像"AI 自顾自跑"，用户反馈延迟极高
- Bug #21（粘贴入队语义）和 Feature #11（reasoning_effort）都是输入控制相关，本 feature 解决"何时让 agent 看到输入"

**设计**：

#### 1. 双层循环模型

```
outer loop: per user turn (现状)
  └─ inner loop: per agent step（API call + tool exec）
     ├─ 每次 inner 迭代开始前：检查 input queue
     ├─ 若 queue 非空：把队列内容作为 user message 注入 messages，跳过本轮原计划，继续 inner loop
     └─ 若 queue 为空：照常发起下一次 API call / 工具执行
```

inner loop 退出条件（沿用现状）：模型返回 `stop_reason = EndTurn` 且无 tool call。

#### 2. 检查点（粗到细）

按介入成本递增分级：

| 检查点 | 介入成本 | 说明 |
|--------|----------|------|
| **A. 每次 API call 前** | 低（必做） | 下一轮请求构造前 pop 全部 queue，作为 user message 拼到 messages 末尾。模型在下次回复时就能看到 |
| **B. tool call 批次完成后** | 低（必做） | 一批并行/顺序 tool call 跑完、准备发回 LLM 前，先 pop queue。最自然的"让 LLM 看到用户新指令"时机 |
| **C. tool call 之间（顺序）** | 中（可选） | 如果 tool call 改顺序执行（Bug #3 的修复方向），可在两个 tool call 之间检查；带"用户已发声"信号意味着后续 tool call 可能被取消 |
| **D. streaming 期间** | 高（不做） | 中断正在进行的 API call。语义复杂、provider 兼容性差，**不在本期范围** |

本期落地 **A + B**。C 留作后续扩展，需要 Bug #3 完成顺序执行后再做。

#### 3. 注入语义

用户排队消息进入 messages 时怎么标记？两种方案：

- **方案 1（普通 user message）**：直接 `Message::user(content)` 拼到末尾，模型自然继续对话
- **方案 2（带元数据的 system note）**：包成 `<user_interrupt>...</user_interrupt>` 或类似标签，提示模型"这是用户中途追加的反馈，请优先采纳"

推荐**方案 1 默认 + 方案 2 配置开关**。普通方案足够大部分场景；标签包裹在 agent 自主决策长链路被纠偏时有用。

#### 4. 取消进行中工作的策略

用户中途插话时，已经 in-flight 的 tool call 怎么办？

- **本期**：让进行中的 tool call **跑完**（不取消），跑完后注入用户消息，下一轮 API call 前模型自己决定要不要采纳
- **后期**（依赖 CancellationToken 基础设施）：选项化的"温柔取消"——给 in-flight tool 发取消信号，taken-effect 后注入用户消息

#### 5. 队列读取并发安全

- 当前 input queue 是 `VecDeque<String>` 包在 App 状态里，UI 线程 push、agent loop 主线程 pop
- 已有共享访问机制（具体待 grep 确认 `Arc<Mutex<...>>` / `tokio::sync::Mutex` / channel）
- 双层循环本期只是**多次调用同一个 pop 接口**，不改并发模型

#### 6. UI 反馈

- 用户在 processing 中输入并 Enter 后：input queue 区显示新条目（已有）
- inner loop 在 A/B 检查点 pop 到消息时：在 output area 注入一条 system 提示行 `[Injected from queue: "..."]`，让用户**看到**"我的反馈被吃进去了"，而不是默默并入下一轮 prompt
- 状态栏可临时高亮 1s 表示"queue 已消费"

#### 7. 配置

`config.json` 新增：
```json
{
  "input_queue": {
    "interrupt_mode": "between_calls",  // off | between_calls | between_tools
    "wrap_with_metadata": false          // 是否用 <user_interrupt> 标签包裹
  }
}
```

CLI 不暴露（属于体验设置，slash 命令 `/queue mode <...>` 切换）。

#### 8. 实施阶段

1. **Phase 1**（本期）：在 `agent_runner.rs` / `processing.rs` 的 inner loop A/B 检查点加 `pop_all_queued()` 调用 + UI 注入提示
2. **Phase 2**：增加 `<user_interrupt>` 包裹选项 + `/queue` slash 命令
3. **Phase 3**（依赖 Bug #3 顺序执行 + cancel 基础设施）：tool call 之间检查（C 检查点）、温柔取消进行中的 tool

**测试场景**：
- 用户 send 消息 → agent 进入 5 个 tool call 链 → 用户在第 2 个 tool 执行时排队 "stop, focus on X" → 期望：第 2 个 tool 跑完后，下一次 API call 前模型立即看到 "stop, focus on X" 并改变方向
- 用户连续排队 3 条 → 一次 pop 全部 → 拼成 3 条 user message 一起注入
- 队列在 inner loop 跑完都没消费过 → 退到 outer loop 时按原逻辑 pop（保持兼容）
- agent 在 ask_user 等待中（Bug #19 已修复）→ queue 不消费，等 ask_user 走完
- subagent 嵌套时：父 agent 的 queue 不应被子 agent 消费；子 agent 自己有独立 inbox（待决策，建议本期父子 agent 都不互通）

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（agent 主循环 inner step）
- `aemeath-cli/src/tui/app/processing.rs`（user turn 顶层循环）
- `aemeath-cli/src/tui/app/mod.rs`（input queue 数据结构 + pop 接口）
- `aemeath-cli/src/tui/app/update.rs`（UI 注入提示）
- `aemeath-core/src/config/mod.rs`（`input_queue` 配置）
- 新增（Phase 2）：`aemeath-core/src/command/commands/queue.rs`

**关联**：
- Feature #7（input queue 基础实现，已完成）
- Bug #21（粘贴入队语义）— 必须先确保入队来源干净
- Bug #3（tool call 流式 + 顺序执行）— Phase 3 的 C 检查点依赖
- Bug #19（ask_user 等待态独占 input，已修复）— queue 消费时需绕开 ask_user 状态

**开放问题**：
- 子 agent 是否共享父 agent 的 input queue？默认不共享，但 deeply nested agent 时父用户反馈如何透传？
- 标签包裹 `<user_interrupt>` 是否 model-agnostic？某些模型可能把它当 XML 字面量解析
- 用户排队"取消当前 tool call"语义如何表达？需要一个特殊关键字 / 命令前缀（例如 `/cancel`）还是 LLM 自行从语义判断？

---

### #30 Agent loop 收尾工作

**实施完成（2026-05-11）**：

**新增类型**（`aemeath-cli/src/agent_runner.rs`）：
- `AgentRunStatus`：`Completed | Cancelled | TimedOut | ApiError(String) | MaxTurns`
- `AgentRunOutcome`：`{ status, turns, duration, role, model }`
- `log_agent_outcome()`：统一写 `[agent_loop_finished]` 结构化日志
- `finalize_and_return!` 宏：子 agent 5 个退出路径统一收口
- `finalize_sub_agent()`：恢复 client 设置 + SubagentStop hook + 日志

**主 loop 收口**（`aemeath-cli/src/tui/app/stream.rs`）：
- 所有退出路径改为 `break` + `outcome`，替代 `return`
- `finalize_main_loop()`：根据 outcome.status 统一触发 Stop/StopFailure hook、日志、task batch 归档
- 打断、API 错误路径现在也走 `agent_loop_finished` 日志

**Task reminder 注入优化**（`aemeath-cli/src/task_reminder.rs`）：
- 首次注入豁免间隔检查，对话第 1 个 turn 即注入 reminder
- 后续每 5 turn 保持原规则

**Task batch 自动归档**：
- Completed/MaxTurns 退出时检查活跃 batch 是否全部完成
- 全部完成 → 归档（`BatchStatus::Archived`），下次对话不再提醒

**已实现 vs 原 P0 范围**：
1. ✅ `AgentRunStatus` / `AgentRunOutcome`
2. ✅ 统一 finalize 函数（main loop + sub agent）
3. ✅ 恢复 client 设置、SubagentStop hook、结构化日志摘要
4. ✅ 保持对外行为不变
5. ✅ Task batch 归档逻辑、首次 reminder 注入（超出原 P0）

**未做（后续 P1/P2）**：
- session 持久化接入 finalize
- tool 资源释放/取消 token 清理
- Task `Interrupted` 状态（当前 Cancelled 时 task 保持原状）
- Ctrl+C 外层处理（仍在 update.rs，未收口到 finalize）

**涉及路径**：
- `aemeath-cli/src/agent_runner.rs`（AgentRunStatus/Outcome、log_agent_outcome、finalize_sub_agent、宏）
- `aemeath-cli/src/tui/app/stream.rs`（finalize_main_loop、退出路径改造）
- `aemeath-cli/src/task_reminder.rs`（首次注入豁免）
- `docs/superpowers/specs/2026-05-11-agent-loop-finalize-design.md`（设计文档）

**测试**：cargo test 506 passed, 0 failed

---

### #31 TUI 架构守卫脚本（TEA 纯度 + 400 行限制）

**目标**：通过两个 lint 脚本 + pre-commit/pre-push hook，在 CI 本地阶段就拦截两类架构偏移：

1. **TUI 层偏离 TEA 架构**：TUI 的 `update()` 应只返回 `Cmd` 描述，由 runtime 执行副作用。禁止在 update 路径中直接 `tokio::spawn`、写文件、发网络请求、操作 clipboard/image 等。
2. **文件超过 400 行**：CLAUDE.md 规定单个 `.rs` 文件不超过 400 行（含测试代码），超出时应拆分职责。

#### 脚本 1：`scripts/check-tea-architecture.sh`

**检查范围**：`aemeath-cli/src/tui/` 目录下所有 `.rs` 文件。

**检查规则**：

| 规则 | 模式 | 说明 |
|------|------|------|
| 禁止直接 tokio::spawn | `tokio::spawn` | 应通过 `Cmd::SpawnTask` 等 Cmd 描述 |
| 禁止直接文件写入 | `std::fs::write\|File::create\|OpenOptions::new().write` | 应通过 `Cmd::WriteFile` |
| 禁止直接网络请求 | `reqwest::Client\|HttpConnector` | 应通过 `Cmd::HttpRequest` |
| 禁止直接 clipboard 操作 | `arboard::Clipboard\|set_clipboard` | 应通过 `Cmd::SetClipboard` |
| 禁止直接 image 操作 | `image::open\|image::load_from` | 应通过 `Cmd` 描述 |

**白名单机制**：行尾注释 `// allow-tea: <reason>` 可豁免（如 `Cmd` runtime 执行层本身需要调用这些 API）。

**退出码**：发现违规 → exit 1，输出违规文件和行号。

#### 脚本 2：`scripts/check-file-lines.sh`

**检查范围**：所有 crate 下的 `.rs` 文件（`aemeath-core/`、`aemeath-cli/`、`aemeath-llm/`、`aemeath-tools/`）。

**检查规则**：

| 规则 | 说明 |
|------|------|
| 单文件 ≤ 400 行 | `wc -l < file` 超过 400 行即报错 |
| 跳过生成文件 | 排除 `target/`、`.worktrees/` |

**退出码**：发现超限 → exit 1，输出超限文件和行数。

#### Hook 集成

- **pre-commit**（`.git/hooks/pre-commit` 或 `lefthook`/`husky`）：运行两个脚本
- **CI**（可选）：`.github/workflows/` 中同步调用
- **手动**：`./scripts/check-tea-architecture.sh && ./scripts/check-file-lines.sh`

#### 实施分解

1. 创建 `scripts/check-tea-architecture.sh`
2. 创建 `scripts/check-file-lines.sh`
3. 配置 git hook（推荐 lefthook 或手动 symlink）
4. 运行一次，修复现有违规（如有的话先记录到白名单）
5. 文档更新：CLAUDE.md 验证门禁段加入两个脚本调用

#### 涉及路径

- 新增：`scripts/check-tea-architecture.sh`
- 新增：`scripts/check-file-lines.sh`
- 修改：CLAUDE.md（验证门禁段）
- 修改：git hook 配置

#### 关联

- CLAUDE.md 编码规范（400 行限制、update() 副作用禁止）
- Feature #23（safe_text lint 脚本）—— 已有 `check-unsafe-text-ops.sh` 可参考

#### 开放问题

- TEA 架构白名单粒度：按行还是按函数？按行更精确但维护成本高
- 400 行限制是否包含空行和注释？建议包含（`wc -l` 天然包含）
- 是否需要自动修复能力（如 `--fix` 自动拆分超长文件）？建议不做，手动拆分更可控

---

### #32 TUI 选中和复制逻辑统一

**目标**：将 output area、input area、status line 三处的选中（selection）和复制（copy）逻辑统一为可复用组件或 trait，消除行为差异和重复代码。

**现状问题**：
- **output area**：有完整的 selection state（起止坐标）、screen_line_map 映射、渲染高亮、Ctrl+C 复制
- **input area**：有独立的 selection 逻辑（基于编辑器光标位置），API 和数据结构与 output area 不同
- **status line**：基本无可选中/复制能力（task list 行即 Bug #33）
- 三处对鼠标事件（按下、拖拽、释放）的处理方式不一致
- 复制到 clipboard 的调用路径各不相同

**统一方案**：
1. 抽取 `SelectableRegion` trait 或 struct，封装：
   - selection state（start/end 坐标）
   - screen_line_map（屏幕行 → 文本行映射）
   - 鼠标事件处理（按下→开始选择、拖拽→更新范围、释放→结束选择）
   - 渲染高亮（选中区域反色）
   - 复制功能（提取选中文本、写入 clipboard）
2. output area / input area / status line 分别实现或持有该组件
3. 统一 clipboard 调用（通过 `Cmd::SetClipboard`，不直接调用 arboard）

**实施分解**：
1. 审计现有三处 selection/copy 代码，提取共性
2. 设计 `SelectableRegion` API
3. 先在 output area 上重构验证
4. 逐步迁移 input area、status line
5. 修复 Bug #33（task list 无法选中）作为验证

**关联**：
- Bug #33（spinner 下方 task list 无法选中和复制）
- CLAUDE.md 编码规范（DRY 原则）

**开放问题**：
- input area 的 selection 与编辑器光标耦合较深，统一后是否需要保留编辑器特有的 selection 行为？
- status line 内容多为动态生成（spinner、task list），selection 的文本提取需要统一的数据源接口

---

### #34 Anthropic Claude 原生 Provider

**目标**：作为独立 LLM provider，支持直接调用 Anthropic Claude Messages API，与现有 OpenAI/OpenRouter/DeepSeek 等并列。

**已完成的改动**：

1. **ApiDriverKind::Anthropic**：`aemeath-core/src/provider.rs` 新增 `Anthropic` 变体，且为默认 provider。
2. **AnthropicProvider**（`aemeath-llm/src/providers/anthropic/`）：
   - 原生 Messages API 调用（`POST /v1/messages`）
   - 流式响应解析 + 非流式 fallback（streaming 失败且无 partial output 时自动降级）
   - Thinking budget（extended thinking）：`thinking_max_tokens > 0` 时自动开启 `thinking.type=enabled`
   - 指数退避重试（最多 10 次），支持 429 / 5xx 自动重试
   - 用户取消（CancellationToken）全路径支持
   - Prompt caching beta header（`anthropic-beta: prompt-caching-2024-07-31`）
3. **消息转换**（`anthropic/message_conversion.rs`）：TrackingHandler 检测 partial output 避免非流式 fallback 重复输出。
4. **请求构造**（`types.rs::CreateMessageRequest`）：thinking budget 序列化、tool schema 嵌入。
5. **池化集成**（`pool.rs`）：Anthropic driver 走独立 provider 分支，不经过 OpenAICompatible wrapper。

**涉及路径**：
- `aemeath-core/src/provider.rs`（ApiDriverKind::Anthropic）
- `aemeath-llm/src/providers/anthropic/mod.rs`（AnthropicProvider 实现）
- `aemeath-llm/src/providers/anthropic/message_conversion.rs`（非流式 fallback + TrackingHandler）
- `aemeath-llm/src/client.rs`（with_provider 匹配 Anthropic 分支）
- `aemeath-llm/src/pool.rs`（Anthropic 跳过 OpenAIProviderConfig 构建）
- `aemeath-llm/src/types.rs`（CreateMessageRequest thinking 序列化）

**测试**：`anthropic_request_serializes_thinking_budget`、`anthropic_request_omits_thinking_when_budget_zero` + `provider.rs` 6 个单元测试。

---

### #35 Diff 渲染中 add 行语法高亮

**目标**：LLM 输出的 unified diff（```diff 代码块）中，`+` 开头的 add 行按目标文件语言做语法高亮，让用户更直观地看到新增代码的结构。

**当前现状**：
- TUI 的 markdown 渲染器已支持 ```diff 代码块的基本着色（`+` 行绿色、`-` 行红色）
- 但 add 行内部没有按语言做语法高亮，所有 add 内容统一绿色文本，难以区分关键字、字符串、注释等
- 项目中目前无语法高亮基础设施（无 tree-sitter / syntect 等依赖）

**预期效果**：
```
-旧的纯绿色 add 行（无结构高亮）
+fn main() {     ← 关键字/函数名/括号各按语言规则着色
+    let x = 1;  ← 类型/变量/数值有区分
+}
```

**关键技术选型**：

| 方案 | 优点 | 缺点 |
|------|------|------|
| **tree-sitter** | 精确解析、增量更新、多语言 | 编译慢、需为每种语言生成 .so |
| **syntect**（TextMate grammar） | 纯 Rust、开箱即用、主题丰富 | 解析精度略低于 tree-sitter |
| **bat / ora** 命令行工具调用 | 零集成成本 | 依赖外部工具、每次高亮 fork 进程 |

推荐 **syntect**，纯 Rust、无外部依赖、与 ratatui 渲染管线契合度高。

**实施分解**：

1. **diff 块检测与语言推断**：markdown 渲染器识别 ```diff 块时，从 diff header（`--- a/foo.rs` / `+++ b/foo.rs`）提取文件扩展名，推断目标语言
2. **语法高亮层**：引入 `syntect` crate，封装 `highlight_line(line, language, theme)` 函数，返回 ratatui 可用的 `Vec<(Color, &str)>` 分段
3. **渲染集成**：diff 渲染器对 `+` 行去掉前导 `+` 后调用高亮层，将结果拼回绿色前缀 + 高亮内容
4. **降级策略**：语言推断失败时回退到当前纯绿色渲染
5. **主题适配**：syntect 主题颜色映射到终端 256 色或真彩色，兼顾暗色/亮色终端

**涉及路径**：
- `aemeath-cli/src/tui/output_area/markdown.rs`（diff 块渲染）
- 新增：`aemeath-cli/src/tui/syntax.rs`（语法高亮封装）
- `aemeath-cli/Cargo.toml`（新增 `syntect` 依赖）

**追加需求 — 显示行号**：
- diff 输出中每行前缀显示行号，让用户直观定位变更位置
- 行号应区分 old/new：`-` 行显示 old 文件行号，`+` 行显示 new 文件行号，上下文行同时显示两个行号
- 行号右对齐，宽度按最大行号动态计算，与 `+`/`-` 标记之间留一个空格分隔
- 预期效果示例：
  ```
  3  3 | fn main() {
  4  4 |     let x = 1;
     5 | +    let y = 2;
  5  6 | }
  ```
- 行号颜色应较淡（灰色），不干扰 `+`/`-` 行的主体着色
- 实施点：`build_diff_lines()` 需跟踪 old/new 行号计数器，`similar` 的 `ChangeTag` 可确定 old/new 行号推进

**关联**：
- TUI markdown 渲染管线
- Feature #23（safe_text）—— 高亮后的分段渲染需走安全索引

**开放问题**：
- `-` 行（删除内容）是否也做语法高亮？建议不做，灰色/红色保留即可，减少视觉噪声
- diff 中混合多文件时语言推断策略：按每个 `---`/`+++` 块切换语言
- 性能：syntect 首次加载 grammar 集合有延迟（~50ms），是否需要 lazy init 或预加载常用语言子集
- 终端颜色支持：是否检测终端真彩色能力降级到 256 色

### #52 Tool 描述英文化：所有 tool 给 LLM 的 description 统一为英文

**状态**：未开始

**背景**：当前所有内置 tool 中，大部分 `description()` 已是英文，仅 `EnterWorktree` 和 `ExitWorktree` 两个 tool 的 description 和 input_schema 参数描述仍为中文。LLM 对英文描述的语义理解更精确，统一为英文有助于减少工具调用错误。

**目标**：将所有内置 tool 给 LLM 的 description 和 input_schema 参数描述统一为英文。

**范围**：
1. 内置 tool：`EnterWorktree`、`ExitWorktree` 的 description 和 input_schema 参数描述改为英文。
2. 审查所有内置 tool 的 `input_schema` 参数 `description` 字段，确认无中文残留。
3. MCP tool 的 description 由 MCP server 返回，不在本 feature 范围内（透传不改动）。

**涉及文件**：
- `agent/tools/src/worktree.rs`：`EnterWorktree`、`ExitWorktree` 的 `fn description()` 和 `fn input_schema()` 实现
- 可能涉及：`agent/tools/src/` 下其他 tool 的 `input_schema` 参数描述审查

**验收标准**：
1. `EnterWorktree` 和 `ExitWorktree` 的 `description()` 返回纯英文描述。
2. `EnterWorktree` 和 `ExitWorktree` 的 `input_schema()` 中所有参数 `description` 字段为英文。
3. 全量审查通过：29 个内置 tool 的 description 和 input_schema 参数描述均为英文。
4. 编译通过（`cargo build -p aemeath-tools`）。

### #54 主动压缩触发：大上下文下防止 LiteLLM 代理拒绝

**状态**：活动中

**背景**：Session 019e4ea6 使用 gpt-5.5 经 LiteLLM 代理（platform.wanaka.fun）执行 TUI DDD 架构迁移时，经过 20+ 轮连续 tool calling 后，累积了 356 个 tool_use + 356 个 tool_result，总共 580+ 条消息，请求体达 427KB+（turn 142）。随后在同 session 的新 turn 21 中（input_tokens=58K，累积上下文），模型突然返回 "I'm sorry, but I cannot assist with that request."（stop_reason=EndTurn），疑似 LiteLLM 代理端将密集的自动化 tool calling 模式判定为滥用行为。

同日早些时候，turn 142-143 也因请求体过大（427KB-459KB）导致全部 10 次重试均以 connection failed 告终，session 上下文膨胀严重。

**目标**：在上下文达到一定阈值时，主动触发 compaction，减小发送给模型的请求体规模，降低触发代理端安全策略的概率。

**核心要求**：
1. 定义 compaction 触发阈值：可选消息数（如 ≥ 300 条）、token 数（如 ≥ 150K input tokens）或请求体大小（如 ≥ 300KB），具体阈值待分析现有 compaction 逻辑后确定。
2. 在每次 LLM 调用前检查上下文规模，达到阈值时自动触发 compaction，将旧的 tool_result 摘要化或移除。
3. compaction 必须是渐进式的：优先移除已完成的 tool 交互结果，保留关键决策点和用户消息。
4. 与现有 compaction 逻辑（`packages/core/src/compact.rs`）集成，不引入独立的压缩路径。

**涉及路径（预计）**：
- `packages/core/src/compact.rs`：现有 compaction 逻辑
- `packages/llm/src/` 或 provider 层：LLM 调用前的阈值检查点
- 配置层：可能新增 compaction 阈值配置项

**验收标准**：
1. 当上下文超过阈值时，自动触发 compaction，tool_result 被摘要化。
2. 压缩后的请求体大小明显降低，不再触发代理端安全拒绝。
3. 关键上下文（用户原始问题、重要决策）在压缩后仍然保留。
4. 单元测试覆盖：阈值触发、边界条件、渐进式压缩保留关键消息。
5. 编译通过（`cargo build`）。

**明确不做**：
1. 不改变现有的 PostCompact 反思或其他 compaction 触发时机。
2. 不对 LiteLLM 代理端配置做调整（不在本 feature 范围）。
3. 不引入用户可见的 compaction 配置 UI（纯内部策略）。
