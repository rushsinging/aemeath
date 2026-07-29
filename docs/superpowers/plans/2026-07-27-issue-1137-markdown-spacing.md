# Issue #1137 Markdown Spacing 根因级实施计划

> **对应 Issue：** https://github.com/rushsinging/aemeath/issues/1137
> **执行方式：** 实施时按任务顺序执行 TDD；跨层字段必须逐层保留相邻契约测试，不得只验证配置源头和最终屏幕。

## 目标

为 TUI Markdown 输出建立一个类型安全、可热更新、可缓存失效的块级间距系统：

- `ui.markdown_spacing = "normal"` 默认保持当前“按原文段落空行，连续空行折叠为一行，首尾空行裁掉”的行为；
- `ui.markdown_spacing = "compact"` 删除 fence 外的块间空行；
- `ui.markdown_spacing_overrides` 可按 `paragraph`、`heading`、`list`、`code_block`、`table`、`blockquote` 独立指定 `before` / `after`；
- override 优先于全局模式，配置 reload 后当前 TUI 立即按新策略重渲染，不能命中旧 block cache；
- fenced code block 内部空行始终保持原样。

## 根因

当前问题不是“过滤空行时少一个条件”，而是四个结构缺口叠加：

1. `primitives/markdown.rs` 与 `primitives/fenced.rs` 逐行消费文本，只知道“当前行是否为空”，没有 Markdown 块身份及块边界，无法正确实现 per-element before/after。
2. `UiConfig` 没有类型化 spacing policy；Issue 原提议的 `String + HashMap` 会把未知 mode/key 和拼写错误推迟到运行时。
3. TUI 禁止直接读取 Config；现有 `ConfigSnapshot → Runtime → SDK → TUI` DTO 未携带 spacing，且 file reload 事件只带 changed keys，不带新的只读配置视图。
4. `RenderCtx` 与两级缓存只把宽度、block version、depth/animation 纳入 key。即使 TUI 收到新配置，不把 spacing policy 纳入上下文和缓存 key，旧渲染仍会复用。

## 核心设计决策

### 1. Shared Config 使用显式类型，不使用开放字符串 Map

在 `agent/shared/src/config/domain/ui.rs` 定义：

- `MarkdownSpacingMode::{Normal, Compact}`，serde `snake_case`，默认 `Normal`；
- `SpacingLines(u8)`：自定义 serde，只接受 `0..=8`；
- `ElementSpacingOverride { before: Option<SpacingLines>, after: Option<SpacingLines> }`，必须保留“未指定”和“显式 0”的区别；
- `MarkdownSpacingOverrides` / `MarkdownSpacingOverridesPatch`：六个显式可选元素字段，不使用 `HashMap<String, _>`。

显式字段避免运行时字符串分派；`MarkdownSpacingOverrides`、对应 patch 与元素对象使用 `#[serde(deny_unknown_fields)]`，因此拼错元素名或 `before` / `after` 会在配置解析阶段失败，Config reload 保留上一 committed snapshot。未知 mode 同样作为 enum 解析错误处理。`SpacingLines` 从类型层阻止任意大空白造成渲染内存放大；边未出现时保持 `None`，显式 `0` 保持 `Some(0)`，供边界优先级算法区分。

`UiConfigPatch` 中 mode 为 `Option<MarkdownSpacingMode>`；overrides 为显式 patch。跨配置层按“字段级稀疏合并”处理：高层只写 `heading.after` 时，低层 `heading.before` 与其他元素仍保留。禁止整体替换 overrides。

### 2. normal 保留原文语义，override 只接管相关边界

块解析器记录相邻块之间原文 fence 外空白是否存在，并把连续空白归一为 `source_gap ∈ {0,1}`。

对相邻块 `left → right`：

- 若边界相关的 `left.after` 或 `right.before` 存在 override：间距为 `max(left.after.unwrap_or(0), right.before.unwrap_or(0))`；未指定侧按 0 处理，确保 override 真正高于全局模式；
- 否则 `Normal` 使用 `source_gap`，精确保留当前行为；
- 否则 `Compact` 使用 0。

文档首部只消费首块 `before` override，尾部只消费末块 `after` override；没有 override 时继续裁掉首尾原文空行。这样既保持默认兼容，也允许 `{ before: 0, after: 0 }` 真正压紧指定元素。

### 3. 先分类块，再渲染块

在 `primitives/fenced.rs` 收敛为单一块级 orchestration：先把输入解析为私有 `MarkdownBlock` 序列，再统一应用边界间距，最后分发到现有 inline/table/diff/syntax renderer。

块类型固定为六类：

- `CodeBlock`：从 opening fence 到 closing fence；未闭合 fence 吞到 EOF；内部行（包括空行）原样交给现有 fence renderer；
- `Table`：表头行后紧跟 separator，继续消费全部 table row；
- `Heading`：fence 外 trim-start 后为 ATX `#{1,6}` 且 `#` 后为空格或行尾；
- `List`：连续 list item 与其缩进续行作为一个块；空行终止；
- `Blockquote`：连续 trim-start 后以 `>` 开头的行作为一个块；
- `Paragraph`：其余连续非空普通行作为一个块。

优先级为 fence → table → heading → list → blockquote → paragraph。列表和引用内部不人为插空行；inline 样式、链接偏移、wrap、table、diff 和 syntax highlight 继续复用现有原语。块分类和 spacing policy 分开，避免把配置判断散落到各 renderer。

### 4. spacing 是 TUI-owned 渲染值，DTO 只负责传输

Shared Config 类型不得进入 CLI。SDK 在 `packages/sdk/src/config_view.rs` 发布纯 DTO：

- `MarkdownSpacingModeView`；
- `ElementSpacingView { before: Option<u8>, after: Option<u8> }`，保留未指定与显式 0；
- `MarkdownSpacingOverridesView`；
- `ConfigView.markdown_spacing` 与 `ConfigView.markdown_spacing_overrides`，新增字段加 `#[serde(default)]`，保持旧 SDK JSON 可反序列化。

Runtime `config_snapshot_to_sdk` 完整映射。`ConfigReloadedEvent` 增加 committed `ConfigView`，Runtime 在成功 reload 时使用 `ConfigRefreshOutcome::Reloaded.snapshot` 生成 view；guidance-only 通知使用当前 committed snapshot。启动 `AgentClientBootstrap` 同样携带初始 `ConfigView`，避免 TUI 以 `ConfigView::default()` 启动后直到事件到来才正确显示。

CLI adapter 把 SDK DTO 映射成 TUI-owned `MarkdownSpacingPolicy`。TUI model 新增专责 `UiPreferences`/`UiPreferencesIntent::MarkdownSpacingChanged`，不把该字段塞入只用于 provider/context/thinking 展示的 `RuntimePresentation`。启动和 `ConfigChanged` / `ConfigReloaded` 走同一 intent；变更必须标记 output dirty。

### 5. policy 进入 RenderCtx 和两级缓存 key

`MarkdownSpacingPolicy` 使用固定六元素值结构并派生 `Clone + Copy + Eq + Hash`。它进入：

- `RenderCtx.markdown_spacing`；
- `BlockCache::CacheKey.markdown_spacing`；
- `document_renderer::GuttedKey.markdown_spacing`。

`OutputDocumentRenderer::render_model_document` / `render_tree` 接受 policy。所有 block 共用该 key，配置变化时 block cache 和 gutted cache 都 miss；不依赖手工清缓存。测试辅助提供 `RenderCtx::for_width(width)` 与 `MarkdownSpacingPolicy::normal()`，避免各测试重复手填完整结构。

`document_renderer.rs` 中“每个 root block 前固定插一空行”是对话块间距，不是单个 Assistant Markdown 内部元素间距，#1137 不修改它。

## 验收门禁映射

| Issue 验收项 | 证据 |
|---|---|
| compact 段落紧贴 | spacing policy L1 + fenced block L2 + TUI framebuffer L4 |
| overrides 独立控制 | Shared patch merge L1、block boundary matrix L1/L2、TUI 场景 L4 |
| 默认行为不变 | normal golden/语义断言、现有 fenced/markdown/table 测试全通过 |
| cargo test + clippy | affected crates、workspace、全架构守卫 |
| fence 内空行不变 | code block 专项 L1/L2 与场景断言 |

---

## Task 1：建立类型化 Config domain 与稀疏合并

**文件：**

- 修改 `agent/shared/src/config/domain/ui.rs`
- 新增 `agent/shared/src/config/domain/ui_tests.rs`
- 修改 `agent/shared/src/config/domain/merge.rs`（沿用现有 inline 测试模块补本次用例，不批量迁移历史测试）
- 修改 `agent/shared/src/config/domain/snapshot.rs`
- 修改 `agent/shared/src/config/domain/scope.rs`
- 修改 `agent/shared/src/config/domain/scope_tests.rs`

**TDD：**

1. 先写失败测试：缺省 config 得到 `Normal + empty overrides`；合法 JSON 能解析六种元素；未知 mode、未知元素名和超过 8 的 spacing 都在反序列化阶段失败。
2. 写跨层 merge 测试：Global 设置 `heading.before=1, heading.after=2`，Local 只设 `heading.after=0`，最终保留 before=1 且 after=0；其他元素不丢失。
3. 写 snapshot 测试：只通过窄 accessor 读取 mode/overrides，不公开完整 `UiConfig`。
4. 把 markdown spacing 归类为“当前 Session 可立即应用”的 `ConfigApplicationScope::Immediate`，同步稳定字符串与测试；`classify_application_scopes` 必须只在 mode/overrides 变化时加入 Immediate，不能因其他 `ui` 字段误报；禁止错误归入 Run 导致下一轮才生效。
5. 实现 domain 类型、默认值、patch apply 与范围分类，使 RED 变 GREEN。
6. 本任务只迁移 `ui.rs` 新增测试到 `ui_tests.rs`；`merge.rs` 的历史 inline 测试不在 #1137 中批量搬迁，新增用例先遵循现有模块形状，避免无关重构。

**验证：**

```bash
cargo test -p share config::domain::ui
cargo test -p share config::domain::merge
cargo test -p share config::domain::scope
```

## Task 2：发布 SDK DTO 并锁定向后兼容

**文件：**

- 修改 `packages/sdk/src/config_view.rs`
- 修改 `packages/sdk/src/wire.rs`，注册新增 DTO 的 OpenAPI schema
- 修改 `packages/sdk/tests/openapi_contract.rs`
- 新增 `packages/sdk/tests/config_view_compat.rs`

**TDD：**

1. 旧 JSON（没有 spacing 字段）反序列化后必须是 normal/空 overrides。
2. 新 JSON round-trip 必须保留 mode、六元素 override 与 before/after。
3. OpenAPI definitions 必须包含新增 DTO，`ConfigView` schema 引用正确。
4. `ConfigReloadedEvent` 新增 `view: ConfigView` 且 `#[serde(default)]`；旧 reload event 仍可读。

**验证：**

```bash
cargo test -p sdk config_view
cargo test -p sdk --test openapi_contract
```

## Task 3：贯通 ConfigSnapshot → Runtime → SDK reload/启动投影

**文件：**

- 修改 `agent/features/runtime/src/application/client/mapping.rs`
- 外置本次相关测试到 `agent/features/runtime/src/application/client/mapping_tests.rs`
- 修改 `agent/features/runtime/src/application/main_loop/looping/events.rs`
- 修改 `agent/features/runtime/src/application/main_loop/looping/loop_phases.rs`
- 修改 `agent/features/runtime/src/adapters/event_projection.rs`
- 修改 `agent/features/runtime/src/adapters/event_projection_tests.rs`
- 修改 `agent/features/runtime/src/adapters/tui_launch.rs`
- 修改 `agent/features/runtime/src/application/client/accessors.rs`
- 修改 `agent/composition/src/app.rs`
- 修改 Composition/Runtime 对应测试

**TDD：**

1. mapping 相邻测试先证明 ConfigSnapshot 六元素 policy 完整进入 SDK ConfigView。
2. Runtime event 测试先证明 successful file reload 携带该次 `Reloaded.snapshot` 对应的 committed view，而不是旧 shell/run snapshot。
3. event projection 测试证明 `RuntimeStreamEvent::ConfigReloaded` 到 `sdk::ChatEvent::ConfigReloaded` 不丢 mode/override。
4. bootstrap 测试证明 `AgentClientBootstrap.config_view` 来自同一 committed ConfigReader snapshot。
5. 实现 Immediate scope 的 SDK 映射与提示规则：Immediate 不显示“下一 Run/重启后生效”。

**验证：**

```bash
cargo test -p runtime application::client
cargo test -p runtime event_projection
cargo test -p composition
```

## Task 4：建立 SDK → TUI adapter 与 UiPreferences 单一状态源

**文件：**

- 修改 `apps/cli/src/tui/adapter/tui_runtime_event.rs`
- 修改 `apps/cli/src/tui/adapter/event_mapping.rs`
- 修改 `apps/cli/src/tui/adapter/event_mapping_tests.rs`
- 修改 `apps/cli/src/tui/adapter/agent_event.rs`
- 修改 `apps/cli/src/tui/adapter/agent_event_runtime_tests.rs`
- 新增 `apps/cli/src/tui/model/ui_preferences.rs`
- 新增 `apps/cli/src/tui/model/ui_preferences_tests.rs`
- 新增 `apps/cli/src/tui/render/output/spacing.rs`，定义 TUI-owned `MarkdownSpacingPolicy` 及 SDK DTO 转换
- 新增 `apps/cli/src/tui/render/output/spacing_tests.rs`
- 修改 `apps/cli/src/tui/model.rs`、`apps/cli/src/tui/model/root.rs`
- 修改 `apps/cli/src/tui/update/intent.rs`
- 修改 `apps/cli/src/tui/update/root_reducer.rs`
- 修改 `apps/cli/src/tui/update/root_reducer_intent_tests.rs`
- 修改 `apps/cli/src/tui/app.rs`
- 修改 `apps/cli/src/chat.rs`
- 修改 `apps/cli/src/tui/app/update.rs`
- 修改 `apps/cli/src/tui/app/update/ui_event_tests.rs`

**TDD：**

1. SDK → TUI ACL 测试先证明 `ConfigView` 的 mode 和所有 override 完整映射到 TUI-owned policy，第二层 mapper 继续零 `sdk::` 依赖；SDK DTO 转换只存在于 `render/output/spacing.rs` adapter。
2. `UiPreferences` reducer 测试先证明默认 normal；应用新 policy 产生 output dirty + 单个 render request；重复相同 policy 返回 no-op change，不产生 output dirty。
3. `map_runtime_event(ConfigChanged/ConfigReloaded)` 生成 `UiPreferencesIntent`，不再 Nop；`App::update_runtime_event` 应用配置时更新 `app.config_view`，保持 `/config` 展示同步。
4. 启动路径把 `AgentClientBootstrap.config_view` 写入同一 UiPreferences 状态，不直接修改 renderer。
5. `ConfigReloaded` 仍可附加原有 system notice，但 presentation-only change 不得错误提示“next run”。

**验证：**

```bash
cargo test -p cli tui::adapter::event_mapping
cargo test -p cli tui::model::ui_preferences
cargo test -p cli tui::update::root_reducer
```

## Task 5：建立块级 Markdown parser 与纯 spacing policy

**文件：**

- 新增 `apps/cli/src/tui/render/output/primitives/blocks.rs`
- 新增 `apps/cli/src/tui/render/output/primitives/blocks_tests.rs`
- 修改 `apps/cli/src/tui/render/output/primitives.rs`
- 修改 `apps/cli/src/tui/render/output/primitives/markdown.rs`
- 新增 `apps/cli/src/tui/render/output/primitives/markdown_tests.rs`，迁移本次触及的 markdown inline tests
- 修改 `apps/cli/src/tui/render/output/primitives/fenced.rs`
- 新增 `apps/cli/src/tui/render/output/primitives/fenced_tests.rs`，迁移现有 fenced tests 后扩展块级矩阵

**TDD（先 RED）：**

1. policy 纯函数矩阵：normal 有/无 source gap；compact；单侧 override；双侧 override 取 max；首块 before；末块 after；0 显式覆盖 normal source gap。
2. classifier 表驱动：ATX heading 边界（`# title` 是 heading，`#tag` 是 paragraph）；连续 list/blockquote；table 优先于 paragraph；closed/unclosed fence；普通 paragraph。
3. normal 回归：`p1\n\np2` 仍只有一个空行，`p1\n\n\np2` 也只有一个；首尾原文空行裁掉。
4. compact：paragraph、heading、list、table、blockquote、code block 之间没有 fence 外空行。
5. override：六种元素分别验证 before/after，跨元素边界用 max 合并而不是相加。
6. fence：普通/语言/diff/text/unclosed fence 内空行原样；compact 只删除 opening/closing fence 与相邻外部块之间的空行。
7. 保留现有 bold/link/wrap/table/diff/style 断言，证明 parser 重组没有破坏其他渲染语义。

**实现约束：**

- `markdown()` 继续只负责单块 inline/list/blockquote 行渲染，不读取全局配置；
- `render_fenced_markdown` 接受 `&MarkdownSpacingPolicy`，拥有 block parse + gap apply + renderer dispatch；
- 不引入完整 CommonMark 依赖，不扩展 Issue 未要求的 setext heading、HTML block、thematic break 语义；这些输入按 paragraph 保持现状；
- 生成空白使用 `RenderedLine::default()`，且最大 8 行约束已在 domain/DTO 转换前保证。

**验证：**

```bash
cargo test -p cli tui::render::output::primitives::blocks
cargo test -p cli tui::render::output::primitives::markdown
cargo test -p cli tui::render::output::primitives::fenced
```

## Task 6：把 policy 接入 RenderCtx 并锁定两级缓存失效

**文件：**

- 修改 `apps/cli/src/tui/render/output/rendered.rs`
- 修改 `apps/cli/src/tui/render/output/block_cache.rs`
- 修改 `apps/cli/src/tui/render/output/document_renderer.rs`
- 修改 `apps/cli/src/tui/render/output/document_renderer/tests.rs`
- 修改 `apps/cli/src/tui/render/output/blocks/assistant_message.rs`
- 新增 `apps/cli/src/tui/render/output/blocks/assistant_message_tests.rs`，并在 `assistant_message.rs` 以外置测试模块替换现有 inline tests
- 修改所有 `RenderCtx { text_width }` 和 `render_model_document` 测试调用点，统一改用 helper/默认 policy

**TDD：**

1. `BlockCache` 测试：同 version/width/policy 命中；只改变 policy 必须 miss。
2. `OutputDocumentRenderer` 测试：只改变 policy 时 content cache 和 gutted cache 都重渲染；再次使用相同 policy 命中。
3. assistant block 测试：RenderCtx normal 与 compact 产出不同内部行数；root block 固定前导分隔行保持不变。
4. 实现 `RenderCtx::for_width`、policy accessor，以及两级 key 字段；生产路径从 `TuiModel.ui_preferences` 把 policy传给 renderer。
5. 搜索确认不存在绕过 RenderCtx、直接调用旧三参数 fenced renderer 的生产路径。

**验证：**

```bash
cargo test -p cli tui::render::output::block_cache
cargo test -p cli tui::render::output::document_renderer
cargo test -p cli tui::render::output::blocks::assistant_message
```

## Task 7：跨层场景验收与 framebuffer 证据

**文件：**

- 修改 `apps/cli/src/tui/app/scenario_tests.rs`
- 新增 `apps/cli/src/tui/app/scenario_tests/markdown_spacing.rs`
- 新增对应 `apps/cli/src/tui/app/scenario_tests/snapshots/*.snap`

**TDD：**

1. 在生成 snapshot 前先写语义断言：相同 assistant Markdown 在 normal 下保留一个段落空行，在 compact 下两段紧贴。
2. 场景通过 `sdk::ChatEvent::ConfigReloaded`（包含新 ConfigView）驱动 ACL → TUI model → renderer，禁止直接篡改 renderer policy。
3. 先 normal 渲染，再发送 compact reload，在不改变 block version/终端宽度的情况下重渲染；断言屏幕空行变化，证明缓存正确失效。
4. 增加 override 场景，至少同时包含 heading、paragraph、list、blockquote、table、code block，并断言各边界符合 policy。
5. 增加 code fence 内空行语义断言；最后接受两张稳定 snapshot（normal/compact 或 compact/overrides）。

**验证：**

```bash
cargo test -p cli markdown_spacing -- --nocapture
```

## Task 8：文档、门禁、Issue 与 PR 收尾

**文件：**

- 修改 `specs/3.9-config-compat.md`：字段、优先级、Immediate scope、ConfigSnapshot accessor、SDK/TUI 消费规则
- 修改 `specs/3.3-tui-cli.md`：块类型、边界合并、fence 内不变量、cache key 规则
- 更新 Issue #1137 checklist/status

**检查清单：**

1. 搜索 `markdown_spacing`，确认配置类型只定义一次，CLI 不依赖 Shared Config 类型。
2. 搜索 `render_fenced_markdown(`、`RenderCtx {`、`CacheKey {`，确认所有生产与测试调用已迁移，没有旧旁路。
3. 检查 `markdown.rs` / `fenced.rs` 是否仍有本次触及的 inline test；按 `rust-coding.md` 渐进外置。
4. 检查旧 `should_skip_blank_outside_fence` / `prev_blank_outside` 路径已删除，避免双重 spacing 规则。
5. Issue 四项验收必须全部完成；任何 N/A 在 PR 明确给出证据。

**完整验证：**

```bash
cargo fmt --check
cargo build --workspace
cargo test -p share
cargo test -p sdk
cargo test -p runtime
cargo test -p composition
cargo test -p cli
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
.agents/hooks/check-architecture-guards.sh --full
```

完成后执行：

```bash
git pull origin main
git diff --check
git status --short
git push -u origin feat/1137-markdown-spacing
```

创建指向 `main` 的非 Draft PR，body 使用仓库模板并写 `Closes #1137`。未经用户对具体 PR 的当前 head 明确授权，不合并 PR、不关闭 Issue。

## 风险与回滚边界

- **默认行为回归：** normal 以 source gap 为真相，并保留现有渲染测试；若现有 snapshot 发生大面积空行变化，先视为 parser 兼容缺陷，不直接接受快照。
- **热更新 stale cache：** policy 同时进入两级 key；禁止只调用 `cache.clear()` 作为补丁，因为启动、事件和测试路径容易漏清。
- **配置兼容：**新增 SDK 字段全部 serde default；Config 文件未知 spacing mode/element 或越界值拒绝 candidate，保留 committed snapshot。
- **类型泄漏：** Shared Config、SDK DTO、TUI policy 三层各自拥有类型，通过显式 adapter 映射；禁止 CLI 引用 `share::config::MarkdownSpacing*`。
- **范围膨胀：**不修改对话 root block 分隔、不引入完整 CommonMark parser、不改变工具 Plain result、不新增 slash 命令或环境变量。
- **回滚：**实现按 domain → DTO → adapter/model → parser → cache → scenario 分层提交；任一层可回滚，不改变 session persistence schema。
