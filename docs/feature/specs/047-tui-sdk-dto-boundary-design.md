# #47 TUI SDK DTO 边界彻底迁移设计

## 背景

#47 的目标是让 CLI/TUI 等入口保持薄，业务能力通过 `packages/sdk::AgentClient` 契约访问 runtime。前两轮已经完成：

1. `AgentClient` 负责初始化 runtime，并提供 models/sessions/task/save 等能力。
2. `AgentClient::chat(ChatRequest) -> ChatStream` 已接入真实 `process_chat_loop`。
3. TUI chat turn、Ctrl+C/Esc cancel、`/save`、MessagesSync、退出自动保存、task status 已改为通过 SDK 调用。

当前剩余问题不是调用路径，而是类型边界：TUI 的事件和渲染状态仍直接承接若干 runtime/core 类型，SDK 中也仍用 `serde_json::Value` 作为过渡载体表达部分 runtime 事件。这会让 TUI 虽然不直接调用 runtime，但仍理解 runtime 的内部模型。

## 目标

本轮目标是更彻底迁移 TUI 边界：

1. `apps/cli/src/tui/**` 不再出现 `runtime::api` 或 `::runtime` 类型依赖。
2. `sdk::ChatEvent` 使用强类型 SDK DTO 表达图片、子代理进度、工作区上下文等事件载荷。
3. TUI 内部 `UiEvent` 和渲染状态只使用 SDK DTO 或 TUI 私有 view model。
4. runtime 类型与 SDK DTO 的转换集中在 `agent/runtime` 的 `AgentClientImpl` 及 CLI composition root / runtime adapter。
5. 不改变现有 TUI 行为，只改变边界和类型投影。

## 非目标

1. 不重写 chat loop、tool loop、hook、session save 等 runtime 行为。
2. 不引入 HTTP/server 或多进程 SDK 通信。
3. 不把所有 CLI 模块都一次性去 runtime 化；本轮范围聚焦 `apps/cli/src/tui/**`。
4. 不为 UI 渲染引入新的状态管理框架。
5. 不主动重构与本目标无关的文件。

## 当前耦合点

已识别的主要耦合包括：

1. `apps/cli/src/tui/core/event.rs`
   - `Message`
   - `ImageData`
   - `AgentProgressEvent`
   - `WorkspaceContext`
   - `ProcessedImage`
   - `ReflectionOutput`
2. `apps/cli/src/tui/session/processing.rs`
   - `images_from_sdk()` 从 `serde_json::Value` 还原 runtime `ImageData`
   - `agent_progress_from_sdk()` 从 `serde_json::Value` 还原 runtime `AgentProgressEvent`
   - `WorkingDirectoryChanged` 从 JSON 还原 runtime `WorkspaceContext`
3. `packages/sdk/src/chat.rs`
   - `ChatEvent::ToolResult.images: serde_json::Value`
   - `ChatEvent::AgentProgress.event: serde_json::Value`
   - `ChatEvent::WorkingDirectoryChanged.workspace: serde_json::Value`
4. TUI 启动上下文中的 memory/skill 等泛型类型是本轮必须处理的边界：若 TUI 不读取这些字段则删除；若仍需展示或命令使用，则替换为 SDK view。

## 设计方案

### 1. SDK 新增强类型 DTO

在 `packages/sdk` 中新增或扩展以下 DTO：

#### Chat 与消息 DTO

- `ChatMessage`：保留现有结构，作为 SDK 消息边界。
- `ChatContentBlock`：后续可替代 `ChatMessage.content: serde_json::Value`，但本轮只在必要时引入，避免一次性触碰所有 message 渲染逻辑。

#### 图片 DTO

- `ToolResultImage`
  - `base64: String`
  - `media_type: String`
- `ClipboardImageView`
  - `display_path: Option<String>`
  - `media_type: String`
  - `width: Option<u32>`
  - `height: Option<u32>`
  - `base64: Option<String>`，仅在现有 TUI 确实需要渲染图片内容时保留

#### 子代理进度 DTO

- `AgentProgressEventView`
  - `sequence: usize`
  - `kind: AgentProgressKindView`
- `AgentProgressKindView`
  - `Message { text: String }`
  - `ToolCalls { calls: Vec<AgentToolCallProgressView> }`
- `AgentToolCallProgressView`
  - `id: String`
  - `name: String`
  - `input: serde_json::Value`
  - `summary: String`

`input` 可以保留 `serde_json::Value`，因为它是工具输入的开放 JSON，而不是 runtime 类型泄漏。

#### 工作区 DTO

- `WorkspaceContextView`
  - `path_base: PathBuf`
  - `working_root: PathBuf`
  - `context_stack: Vec<WorkspaceStackEntryView>`
- `WorkspaceStackEntryView`
  - `path_base: PathBuf`
  - `working_root: PathBuf`

如果 TUI 只需要当前路径和保存 session 所需 workspace，则由 runtime 保存完整 workspace，TUI 只保留展示字段；不强迫 TUI 持有完整 domain workspace。

#### Reflection DTO

- `ReflectionOutputView`
  - `content: String`
  - `input_tokens: u32`
  - `output_tokens: u32`
  - 不暴露 runtime reflection 模块类型。

#### Skill / Memory DTO

- `SkillView`
  - `name: String`
  - `description: Option<String>`
  - `source: Option<String>`
- `MemoryConfigView`
  - 只保留 TUI 展示或命令需要的稳定配置字段。

### 2. ChatEvent 强类型化

`packages/sdk/src/chat.rs` 中事件改为：

- `ToolResult { images: Vec<ToolResultImage>, ... }`
- `AgentProgress { event: AgentProgressEventView, ... }`
- `WorkingDirectoryChanged { path_base, working_root, workspace: WorkspaceContextView }`

不再用 `serde_json::Value` 承载上述三类结构。

### 3. Runtime 作为 DTO 转换边界

`agent/runtime/src/client.rs` 中保留 runtime 内部类型，新增纯转换函数：

- runtime image → `ToolResultImage`
- runtime agent progress → `AgentProgressEventView`
- runtime workspace → `WorkspaceContextView`
- runtime message → `ChatMessage`
- SDK message → runtime message
- runtime reflection/image/skill/memory → SDK TUI view

这些函数属于 runtime adapter，不放入 TUI。

### 4. TUI Event 去 runtime 类型

`apps/cli/src/tui/core/event.rs` 改为只引用 SDK DTO：

- `ToolResult.images: Vec<sdk::ToolResultImage>`
- `MessagesSync(Vec<sdk::ChatMessage>)`
- `AgentProgress.event: sdk::AgentProgressEventView`
- `WorkingDirectoryChanged(StatusContextUpdate)` 中 `workspace: sdk::WorkspaceContextView`
- `ClipboardImage(sdk::ClipboardImageView)`
- `ReflectionDone { output: sdk::ReflectionOutputView }`

如果某些渲染组件更适合使用 UI 私有格式，可在 TUI 内部创建 `tui::view_model`，但不得回退到 runtime 类型。

### 5. TUI message 渲染策略

本轮不要求把 `ChatMessage.content` 全面拆成强类型内容块。原因：消息内容块兼容 provider/tool 结果，结构较多，全面迁移容易扩大范围。

过渡策略：

1. `ChatMessage` 继续作为 SDK 边界类型。
2. TUI 的文本提取、消息追加、MessagesSync 使用 SDK helper，如 `ChatMessage::text_content()`。
3. runtime 内部保存 session 时继续在 `AgentClientImpl` 中转换回 runtime message。
4. 后续单独一轮再将 `ChatMessage.content` 从 JSON 迁移为枚举内容块。

### 6. 启动上下文策略

`TuiLaunchContext<MemoryConfig, Skill>` 当前是泛型过渡结构。彻底方案改为：

- `TuiLaunchContext` 不再泛型化。
- `memory_config: MemoryConfigView`
- `skills: HashMap<String, SkillView>`

如果 TUI 只透传 memory/skill 给 runtime，不直接使用，则删除这些字段，由 `AgentClientImpl` 内部持有。

优先选择“删除无用字段”，其次才创建 view DTO。

## 分阶段实施

### 阶段 1：ChatEvent DTO 强类型化

1. 在 SDK 新增 `ToolResultImage`、`AgentProgressEventView`、`WorkspaceContextView`。
2. 修改 `ChatEvent` 对应字段类型。
3. 修改 `AgentClientImpl::runtime_event_to_sdk_event()` 转换逻辑。
4. 修改 TUI `sdk_event_to_ui_event()`，删除 JSON 反序列化辅助函数。
5. 验证 sdk/runtime/cli 编译与 processing 单测。

### 阶段 2：TUI UiEvent 去 runtime 类型

1. 修改 `apps/cli/src/tui/core/event.rs` 的事件载荷。
2. 调整所有消费 `UiEvent` 的渲染/状态更新模块。
3. 把消息相关逻辑改为 SDK helper 或 TUI view model。
4. 把 agent progress 渲染改为读取 SDK DTO。
5. 把 workspace 状态更新改为 SDK DTO。

### 阶段 3：Clipboard / Reflection / LaunchContext 去 runtime 类型

1. 为 clipboard image 和 reflection output 建立 SDK view DTO。
2. runtime adapter 将现有 runtime 类型投影为 SDK view。
3. TUI 更新展示逻辑。
4. 检查 `TuiLaunchContext` 泛型字段，删除无用字段或替换为 SDK view。

### 阶段 4：守卫与文档

1. `.agents/hooks/check-forbidden-imports.sh` 新增 `apps/cli/src/tui/**` 禁止 `::runtime` / `runtime::api`。
2. 更新 `docs/feature/active.md` 中 #47 当前推进状态。
3. 保持 Rust 文件 <= 400 行，必要时拆分 DTO 或转换函数。

## 测试与验证

必须运行：

1. `cargo check -p sdk`
2. `cargo check -p runtime`
3. `cargo check -p cli`
4. `cargo test -p sdk chat -- --nocapture`
5. `cargo test -p cli tui::session::processing -- --nocapture`
6. 与受影响渲染模块相关的现有 TUI 单测
7. `AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh`

新增测试优先覆盖：

1. SDK DTO 转换保持 image 字段不丢失。
2. agent progress message/tool_calls 两种 kind 均可映射。
3. workspace context view 至少保留 path_base 和 working_root。
4. TUI `sdk_event_to_ui_event()` 不再依赖 JSON 反序列化。

## 风险与缓解

### 风险 1：消息内容块迁移范围过大

缓解：本轮保留 `ChatMessage.content: serde_json::Value`，只把明确 runtime 类型泄漏的事件载荷改成强类型。

### 风险 2：TUI 保存 session 需要完整 WorkspaceContext

缓解：优先让 runtime 持有和保存完整 workspace，TUI 只保存展示 view；如必须由 TUI 回传，则 `WorkspaceContextView` 需包含完整可逆字段。

### 风险 3：Reflection / Clipboard 字段不清楚

缓解：先按当前 TUI 实际读取字段建 DTO，不做超前抽象。

### 风险 4：文件超过 400 行

缓解：SDK DTO 分文件放置，如 `chat.rs` 只保留 chat 事件，TUI 视图类型放 `tui.rs` 或新文件；runtime 转换函数必要时拆到 `client/chat_projection.rs`。

## 完成定义

本轮完成后满足：

1. `apps/cli/src/tui/**` 中无 `::runtime`、`runtime::api` import 或类型路径。
2. `sdk::ChatEvent` 中 images、agent progress、workspace 均为 SDK 强类型 DTO。
3. TUI chat/cancel/save/task status 行为保持不变。
4. 架构守卫能阻止 TUI 再次引入 runtime 类型。
5. #47 active 文档记录本轮完成状态与剩余债务。
