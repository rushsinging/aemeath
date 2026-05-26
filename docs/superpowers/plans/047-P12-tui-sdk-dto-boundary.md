# Feature 47 P12 TUI SDK DTO Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `apps/cli/src/tui/**` 不再承接 runtime 类型，并把 `sdk::ChatEvent` 中图片、子代理进度、workspace 等过渡 JSON 改为 SDK 强类型 DTO。

**Architecture:** `packages/sdk` 定义 TUI 可消费的稳定 DTO；`agent/runtime::AgentClientImpl` 是 runtime domain 类型到 SDK DTO 的转换边界；TUI 只使用 SDK DTO 或 TUI 私有 view model。为了控制风险，消息内容本轮仍保留 `ChatMessage.content: serde_json::Value`，但 TUI 不再导入 runtime `Message`。

**Tech Stack:** Rust workspace、Tokio、serde_json、`packages/sdk`、`agent/runtime`、`apps/cli` TUI、现有 architecture guard hooks。

---

## File Structure

- Modify: `packages/sdk/src/chat.rs`
  - 新增 `ToolResultImage`、`AgentProgressEventView`、`AgentProgressKindView`、`AgentToolCallProgressView`、`WorkspaceContextView`、`WorkspaceStackEntryView`，并把 `ChatEvent` 三个 JSON 字段改为强类型。
- Modify: `packages/sdk/src/tui.rs`
  - 新增 `ClipboardImageView`、`ReflectionOutputView`、`ReflectionMemorySuggestionView`、`MemoryConfigView`、`ReflectionConfigView`、`SkillView`，并把 `TuiLaunchContext` 改为非泛型 SDK DTO。
- Modify: `packages/sdk/src/lib.rs`
  - re-export 新增 DTO。
- Modify: `agent/runtime/src/client.rs`
  - 将 runtime stream event 映射为 SDK 强类型 DTO；将 `tui_launch_context()` 返回 SDK DTO；补充转换函数测试。
- Modify: `agent/runtime/src/tui_launch.rs`
  - 删除或收窄 runtime 内部过渡结构；若仍被 CLI 使用，仅作为 runtime 私有结构，不暴露给 TUI。
- Modify: `apps/cli/src/runtime_adapter.rs`
  - 继续作为 CLI composition root；新增仅在非 TUI 层需要的 runtime adapter 函数。
- Modify: `apps/cli/src/tui/core/event.rs`
  - `UiEvent` 和 `StatusContextUpdate` 改用 SDK DTO。
- Modify: `apps/cli/src/tui/session/processing.rs`
  - 删除 `images_from_sdk()`、`agent_progress_from_sdk()` 和 workspace JSON 反序列化。
- Modify: `apps/cli/src/tui/core/state/chat.rs`
  - `messages`、`pending_images`、`pending_reflection` 改为 SDK DTO。
- Modify: `apps/cli/src/tui/core/state/session.rs`
  - `memory_config` 改为 `sdk::MemoryConfigView`。
- Modify: `apps/cli/src/tui/mod.rs`
  - 删除 runtime message 转换 helper，新增 SDK message helper 或直接使用 SDK 类型。
- Modify: `apps/cli/src/tui/core/update/enter.rs`
  - 使用 `sdk::ChatMessage::user_text` / `user_with_images` 风格 helper 组装用户消息。
- Modify: `apps/cli/src/tui/core/update/ui_event.rs`
  - 消费 SDK DTO，reflection 格式化改为 SDK view helper 或 TUI 私有函数。
- Modify: `apps/cli/src/tui/core/run_loop.rs`
  - 移除 runtime 参数；pending slash prompt 写入 SDK message；保存时直接 sync SDK messages。
- Modify: `apps/cli/src/tui/session/session_lifecycle.rs`
  - `App::run` 接收 SDK launch context 或更薄参数，不再接收 LlmClient/ToolRegistry/SystemBlock/TaskStore 等 runtime 类型。
- Modify: `apps/cli/src/tui/session/resume.rs`
  - 改为接收 `Vec<sdk::ChatMessage>`，完整性清理移到 runtime/adapter。
- Modify: `apps/cli/src/tui/input/paste_handler.rs`
  - 图片读取改走 SDK/adapter 或 Cmd；TUI 只接收 `sdk::ClipboardImageView`。
- Modify: `apps/cli/src/tui/core/cmd_exec.rs`
  - 移除 runtime 基础设施字段，clipboard/image 侧效应改走 SDK DTO。
- Modify: `apps/cli/src/tui/core/runtime.rs`
  - `Skill` 改为 `sdk::SkillView`；task status fallback 删除或改 SDK。
- Modify: `apps/cli/src/tui/core/slash.rs`
  - `/compact`、`/context`、`/paste`、CommandRegistry action 等 runtime 依赖改成 SDK helper/AgentClient 能力或保守下沉到 non-TUI adapter。
- Modify: `apps/cli/src/tui/core/slash/reflection.rs`
  - Reflection LLM 调用与 memory apply 下沉到 SDK/AgentClient；TUI 只处理 `ReflectionOutputView`。
- Modify: `apps/cli/src/tui/output_area/tool_display/agent.rs`
  - 使用 `sdk::AgentProgressEventView`。
- Modify: `apps/cli/src/tui/output_area/tool_display/common.rs`
  - `format_agent_tool_calls()` 使用 `sdk::AgentToolCallProgressView`。
- Modify tests under `apps/cli/src/tui/**`
  - runtime constructors 改成 SDK DTO constructors。
- Modify: `.agents/hooks/check-forbidden-imports.sh`
  - `apps/cli/src/tui/**` 禁止 `::runtime`、`runtime::api`、`runtime::`。
- Modify: `docs/feature/active.md`
  - 更新 #47 P12 实施状态和剩余债务。

---

### Task 1: SDK ChatEvent DTO 强类型化

**Files:**
- Modify: `packages/sdk/src/chat.rs`
- Modify: `packages/sdk/src/lib.rs`
- Modify: `agent/runtime/src/client.rs`
- Modify: `apps/cli/src/tui/session/processing.rs`

- [ ] **Step 1: 在 SDK 写 DTO 与 ChatEvent 字段变更测试**

Edit `packages/sdk/src/chat.rs` tests，加入：

```rust
    #[test]
    fn test_tool_result_image_keeps_base64_and_media_type() {
        let image = ToolResultImage {
            base64: "abc".to_string(),
            media_type: "image/png".to_string(),
        };

        assert_eq!(image.base64, "abc");
        assert_eq!(image.media_type, "image/png");
    }

    #[test]
    fn test_agent_progress_view_supports_message_and_tool_calls() {
        let message = AgentProgressEventView {
            sequence: 1,
            kind: AgentProgressKindView::Message {
                text: "working".to_string(),
            },
        };
        let tools = AgentProgressEventView {
            sequence: 2,
            kind: AgentProgressKindView::ToolCalls {
                calls: vec![AgentToolCallProgressView {
                    id: "tool-1".to_string(),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path":"a.rs"}),
                    summary: "a.rs".to_string(),
                }],
            },
        };

        assert_eq!(message.sequence, 1);
        match message.kind {
            AgentProgressKindView::Message { text } => assert_eq!(text, "working"),
            other => panic!("unexpected kind: {other:?}"),
        }
        match tools.kind {
            AgentProgressKindView::ToolCalls { calls } => {
                assert_eq!(calls[0].name, "Read");
                assert_eq!(calls[0].summary, "a.rs");
            }
            other => panic!("unexpected kind: {other:?}"),
        }
    }

    #[test]
    fn test_workspace_context_view_keeps_paths() {
        let view = WorkspaceContextView {
            path_base: "/repo/sub".into(),
            working_root: "/repo".into(),
            context_stack: vec![WorkspaceStackEntryView {
                path_base: "/repo".into(),
                working_root: "/repo".into(),
            }],
        };

        assert_eq!(view.path_base.to_string_lossy(), "/repo/sub");
        assert_eq!(view.working_root.to_string_lossy(), "/repo");
        assert_eq!(view.context_stack.len(), 1);
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```bash
cargo test -p sdk chat -- --nocapture
```

Expected: FAIL，错误包含 `cannot find struct, variant or union type 'ToolResultImage'` 或对应 DTO 未定义。

- [ ] **Step 3: 新增 SDK DTO 并替换 ChatEvent 字段**

Edit `packages/sdk/src/chat.rs` near imports:

```rust
use crate::ChatMessage;
use std::path::PathBuf;
```

Insert before `ChatEvent`:

```rust
/// 工具结果中的图片载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultImage {
    pub base64: String,
    pub media_type: String,
}

/// Sub-agent 工具调用进度。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolCallProgressView {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub summary: String,
}

/// Sub-agent 进度类型。
#[derive(Debug, Clone, PartialEq)]
pub enum AgentProgressKindView {
    Message { text: String },
    ToolCalls { calls: Vec<AgentToolCallProgressView> },
}

/// Sub-agent 进度事件。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProgressEventView {
    pub sequence: usize,
    pub kind: AgentProgressKindView,
}

/// workspace 栈条目视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceStackEntryView {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}

/// TUI 可展示的 workspace 上下文视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceContextView {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
    pub context_stack: Vec<WorkspaceStackEntryView>,
}
```

Change `ChatEvent` variants:

```rust
      ToolResult {
          id: String,
          tool_name: String,
          output: String,
          is_error: bool,
          images: Vec<ToolResultImage>,
      },
```

```rust
      AgentProgress {
          tool_id: String,
          event: AgentProgressEventView,
      },
```

```rust
      WorkingDirectoryChanged {
          path_base: String,
          working_root: String,
          workspace: WorkspaceContextView,
      },
```

- [ ] **Step 4: re-export DTO**

Edit `packages/sdk/src/lib.rs` export line:

```rust
pub use chat::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChatEvent,
    ChatInput, ChatRequest, ChatResult, ChatStream, ToolResultImage, WorkspaceContextView,
    WorkspaceStackEntryView,
};
```

- [ ] **Step 5: runtime 转换函数改为强类型**

Edit `agent/runtime/src/client.rs` imports:

```rust
use sdk::{
    AgentClient, AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView,
    ChangeSet, ChatEvent, ChatRequest, ChatStream, CostInfo, ModelSummary, ProjectContext,
    SdkError, SessionSnapshot, SessionSummary, TaskStatusView, TaskSummary, ToolResultImage,
    WorkspaceContextView, WorkspaceStackEntryView,
};
```

Replace ToolResult mapping:

```rust
          crate::chat::RuntimeStreamEvent::ToolResult {
              id,
              tool_name,
              output,
              is_error,
              images,
          } => ChatEvent::ToolResult {
              id,
              tool_name,
              output,
              is_error,
              images: images
                  .into_iter()
                  .map(|image| ToolResultImage {
                      base64: image.base64,
                      media_type: image.media_type,
                  })
                  .collect(),
          },
```

Replace AgentProgress mapping:

```rust
          crate::chat::RuntimeStreamEvent::AgentProgress { tool_id, event } => {
              ChatEvent::AgentProgress {
                  tool_id,
                  event: agent_progress_event_to_sdk(event),
              }
          }
```

Replace WorkingDirectoryChanged mapping:

```rust
              ChatEvent::WorkingDirectoryChanged {
                  path_base,
                  working_root,
                  workspace: workspace_context_to_sdk(workspace),
              }
```

Replace `agent_progress_event_to_json` with:

```rust
fn agent_progress_event_to_sdk(
    event: crate::api::core::tool::AgentProgressEvent,
) -> AgentProgressEventView {
    let kind = match event.kind {
        crate::api::core::tool::AgentProgressKind::ToolCalls { calls } => {
            AgentProgressKindView::ToolCalls {
                calls: calls
                    .into_iter()
                    .map(|call| AgentToolCallProgressView {
                        id: call.id,
                        name: call.name,
                        input: call.input,
                        summary: call.summary,
                    })
                    .collect(),
            }
        }
        crate::api::core::tool::AgentProgressKind::Message { text } => {
            AgentProgressKindView::Message { text }
        }
    };
    AgentProgressEventView {
        sequence: event.sequence,
        kind,
    }
}

fn workspace_context_to_sdk(workspace: crate::session::WorkspaceContext) -> WorkspaceContextView {
    WorkspaceContextView {
        path_base: workspace.path_base.into(),
        working_root: workspace.working_root.into(),
        context_stack: workspace
            .context_stack
            .into_iter()
            .map(|entry| WorkspaceStackEntryView {
                path_base: entry.path_base.into(),
                working_root: entry.working_root.into(),
            })
            .collect(),
    }
}
```

- [ ] **Step 6: TUI processing 删除 JSON 反序列化**

Edit `apps/cli/src/tui/session/processing.rs`:

Replace ToolResult branch body field:

```rust
              images,
```

Replace MessagesSync branch with:

```rust
          sdk::ChatEvent::MessagesSync(messages) => UiEvent::MessagesSync(messages),
```

Replace AgentProgress branch with:

```rust
          sdk::ChatEvent::AgentProgress { tool_id, event } => UiEvent::AgentProgress {
              tool_id,
              event,
          },
```

Replace WorkingDirectoryChanged branch with:

```rust
          sdk::ChatEvent::WorkingDirectoryChanged {
              path_base,
              working_root,
              workspace,
          } => UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
              path_base: crate::tui::core::display_status_path(std::path::Path::new(&path_base)),
              working_root: crate::tui::core::display_status_path(std::path::Path::new(
                  &working_root,
              )),
              branch: crate::tui::core::git_branch_for(std::path::Path::new(&working_root)),
              kind: crate::tui::core::worktree_kind_for(std::path::Path::new(&working_root)),
              raw_path_base: std::path::PathBuf::from(path_base),
              raw_working_root: std::path::PathBuf::from(working_root),
              workspace,
          }),
```

Delete functions `images_from_sdk()` and `agent_progress_from_sdk()` entirely.

- [ ] **Step 7: 更新 TUI event 类型以编译到下一批**

Edit `apps/cli/src/tui/core/event.rs` imports:

```rust
use std::path::PathBuf;
```

Change fields:

```rust
    pub workspace: sdk::WorkspaceContextView,
```

```rust
          images: Vec<sdk::ToolResultImage>,
```

```rust
      MessagesSync(Vec<sdk::ChatMessage>),
```

```rust
          event: sdk::AgentProgressEventView,
```

- [ ] **Step 8: 运行阶段验证**

Run:

```bash
cargo fmt --all
cargo check -p sdk
cargo check -p runtime
cargo check -p cli
cargo test -p sdk chat -- --nocapture
cargo test -p cli tui::session::processing -- --nocapture
```

Expected: all PASS.

- [ ] **Step 9: 提交阶段 1**

Run:

```bash
git add packages/sdk/src/chat.rs packages/sdk/src/lib.rs agent/runtime/src/client.rs apps/cli/src/tui/session/processing.rs apps/cli/src/tui/core/event.rs
git commit -m "refactor: 强类型化 SDK chat 事件 DTO (refs #47)" -m "- 为图片、agent progress、workspace 增加 SDK DTO" -m "- 删除 TUI chat event JSON 反序列化过渡"
```

---

### Task 2: TUI chat/message/image 状态改用 SDK DTO

**Files:**
- Modify: `packages/sdk/src/session.rs`
- Modify: `apps/cli/src/tui/core/state/chat.rs`
- Modify: `apps/cli/src/tui/mod.rs`
- Modify: `apps/cli/src/tui/core/update/enter.rs`
- Modify: `apps/cli/src/tui/core/update/spawn_context.rs`
- Modify: `apps/cli/src/tui/core/update/ui_event.rs`
- Modify: `apps/cli/src/tui/core/run_loop.rs`
- Modify: `apps/cli/src/tui/session/session_lifecycle.rs`
- Modify: `apps/cli/src/tui/session/resume.rs`
- Modify: `apps/cli/src/tui/core/slash.rs`

- [ ] **Step 1: 为 ChatMessage 增加 helper 测试**

Edit `packages/sdk/src/session.rs` tests，加入：

```rust
    #[test]
    fn test_chat_message_user_text_builds_text_content() {
        let message = ChatMessage::user_text("hello");

        assert_eq!(message.role, "user");
        assert_eq!(message.text_content(), "hello");
    }

    #[test]
    fn test_chat_message_user_with_images_keeps_images() {
        let message = ChatMessage::user_with_images(
            "look",
            vec![ToolResultImage {
                base64: "abc".to_string(),
                media_type: "image/png".to_string(),
            }],
        );

        assert_eq!(message.text_content(), "look");
        let blocks = message.content.as_array().expect("content should be array");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1]["type"], "image");
    }

    #[test]
    fn test_chat_message_empty_or_invalid_content_returns_empty_text() {
        let message = ChatMessage {
            role: "assistant".to_string(),
            content: serde_json::Value::Null,
        };

        assert_eq!(message.text_content(), "");
    }
```

Ensure tests import image DTO:

```rust
use crate::ToolResultImage;
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```bash
cargo test -p sdk session -- --nocapture
```

Expected: FAIL，错误包含 `no function or associated item named 'user_text'`。

- [ ] **Step 3: 实现 ChatMessage helper**

Edit `packages/sdk/src/session.rs` add impl:

```rust
impl ChatMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: serde_json::json!([{ "type": "text", "text": text.into() }]),
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: serde_json::json!([{ "type": "text", "text": text.into() }]),
        }
    }

    pub fn user_with_images(text: impl Into<String>, images: Vec<crate::ToolResultImage>) -> Self {
        let mut blocks = vec![serde_json::json!({ "type": "text", "text": text.into() })];
        blocks.extend(images.into_iter().map(|image| {
            serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.media_type,
                    "data": image.base64,
                }
            })
        }));
        Self {
            role: "user".to_string(),
            content: serde_json::Value::Array(blocks),
        }
    }

    pub fn text_content(&self) -> String {
        self.content
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    }
}
```

- [ ] **Step 4: ChatState 改用 SDK DTO**

Edit `apps/cli/src/tui/core/state/chat.rs`:

```rust
//! 聊天相关纯数据状态

/// 聊天会话的所有可变数据（不含视图组件 output_area）
#[derive(Debug)]
pub(crate) struct ChatState {
    pub messages: Vec<sdk::ChatMessage>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_api_calls: u64,
    pub last_input_tokens: u64,
    pub pending_images: Vec<sdk::ToolResultImage>,
    pub system_prompt_text: String,
    pub context_size: usize,
    pub tool_call_active: bool,
    pub active_tool_call_ids: std::collections::HashSet<String>,
    pub turn_count: usize,
    pub pending_reflection: Option<sdk::ReflectionOutputView>,
    pub is_processing: bool,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_api_calls: 0,
            last_input_tokens: 0,
            pending_images: Vec::new(),
            system_prompt_text: String::new(),
            context_size: 200_000,
            tool_call_active: false,
            active_tool_call_ids: std::collections::HashSet::new(),
            turn_count: 0,
            pending_reflection: None,
            is_processing: false,
        }
    }
}
```

This references `ReflectionOutputView`; execute Task 4 Step 1-4 first when applying Task 2, then return to this step. Do not introduce a temporary `serde_json::Value` placeholder for `pending_reflection` because the final boundary forbids JSON stand-ins for TUI-owned runtime views.

- [ ] **Step 5: 删除 `tui/mod.rs` runtime conversion helper**

Edit `apps/cli/src/tui/mod.rs` to remove `messages_to_sdk()` and `message_from_sdk()` completely. The file should only export modules and TUI public types:

```rust
pub mod completion;
pub mod core;
pub mod display;
pub mod input;
pub mod output_area;
pub mod session;

pub use self::core::App;
pub use self::display::status_bar::StatusBar;
pub use self::input::input_area::InputArea;
pub use self::output_area::OutputArea;
```

- [ ] **Step 6: update_enter 使用 SDK message helper**

Edit `apps/cli/src/tui/core/update/enter.rs` remove runtime import and replace message push logic:

```rust
          let images: Vec<sdk::ToolResultImage> = self.chat.pending_images.drain(..).collect();
          if images.is_empty() {
              self.chat.messages.push(sdk::ChatMessage::user_text(&input));
          } else {
              self.chat
                  .messages
                  .push(sdk::ChatMessage::user_with_images(&input, images));
          }
```

- [ ] **Step 7: spawn context 不再转换 messages**

Edit `apps/cli/src/tui/core/update/spawn_context.rs` so `messages` uses clone directly:

```rust
            messages: self.chat.messages.clone(),
```

- [ ] **Step 8: MessagesSync 直接写 SDK messages**

Edit `apps/cli/src/tui/core/update/ui_event.rs` `MessagesSync` branch remains:

```rust
              UiEvent::MessagesSync(msgs) => {
                  self.chat.messages = msgs;
                  return UpdateResult {
                      cmd: Cmd::SaveCurrentSession,
                      pending_slash: None,
                  };
              }
```

No runtime conversion should appear.

- [ ] **Step 9: 保存 session 直接 sync SDK messages**

Edit `apps/cli/src/tui/core/run_loop.rs`, `apps/cli/src/tui/session/session_lifecycle.rs`, `apps/cli/src/tui/core/slash.rs` replacing:

```rust
.sync_current_messages(crate::tui::messages_to_sdk(&self.chat.messages))
```

with:

```rust
.sync_current_messages(self.chat.messages.clone())
```

- [ ] **Step 10: pending slash prompt 使用 SDK message**

Edit `apps/cli/src/tui/core/run_loop.rs` replace runtime message push:

```rust
                      self.chat.messages.push(sdk::ChatMessage::user_text(&prompt));
```

- [ ] **Step 11: resume 接收 SDK messages**

Edit `apps/cli/src/tui/session/resume.rs` signature:

```rust
      pub(crate) fn resume_session_messages(
          &mut self,
          session_id: &str,
          messages: Vec<sdk::ChatMessage>,
          created_at: String,
      ) {
```

Remove sanitize/deep_clean logic from TUI; runtime adapter must pass already-clean messages. Render loop becomes:

```rust
          for i in 0..messages.len() {
              let subsequent = if i + 1 < messages.len() {
                  Some(&messages[i + 1])
              } else {
                  None
              };
              self.render_history_message(&messages[i], subsequent);
          }
          self.chat.messages = messages;
```

- [ ] **Step 12: run resume path converts before TUI or uses SDK session API**

Edit `apps/cli/src/tui/session/session_lifecycle.rs` resume block to avoid runtime types in TUI. Implement the new SDK method `load_tui_session()` from Task 5 before wiring this block. TUI calls `agent_client.load_tui_session(id).await` and consumes only SDK DTO:

```rust
              let Some(agent_client) = self.agent_client.clone() else {
                  self.output_area.push_system("[warning: cannot resume without SDK agent client, starting new]");
                  return;
              };
              match agent_client.load_tui_session(id).await {
                  Ok(s) => {
                      let msg_count = s.messages.len();
                      self.session.session_created_at = Some(s.created_at.clone());
                      if let Some(workspace) = &s.workspace {
                          let event = crate::tui::core::status_context_for_workspace(workspace.clone());
                          if let crate::tui::core::event::UiEvent::WorkingDirectoryChanged(ctx) = event {
                              self.status_bar.set_context_paths(ctx.path_base, ctx.working_root);
                              self.status_bar.set_git_context(ctx.kind, ctx.branch.unwrap_or_default());
                          }
                      }
                      for i in 0..s.messages.len() {
                          let subsequent = if i + 1 < s.messages.len() { Some(&s.messages[i + 1]) } else { None };
                          self.render_history_message(&s.messages[i], subsequent);
                      }
                      self.chat.messages = s.messages;
                      self.output_area.push_system(&format!("[resumed session {} ({} messages)]", id, msg_count));
                  }
                  Err(e) => {
                      self.output_area.push_system(&format!(
                          "[warning: failed to resume session {}: {}, starting new]",
                          id, e
                      ));
                  }
              }
```

- [ ] **Step 13: render_history_message 改为 SDK message**

Search for `fn render_history_message` and change parameter types to:

```rust
message: &sdk::ChatMessage,
subsequent: Option<&sdk::ChatMessage>,
```

Use `message.role.as_str()` and `message.text_content()` instead of runtime role/content helpers.

- [ ] **Step 14: 运行阶段验证**

Run:

```bash
cargo fmt --all
cargo check -p sdk
cargo check -p cli
cargo test -p sdk session -- --nocapture
cargo test -p cli tui::session::processing -- --nocapture
```

Expected: all PASS.

- [ ] **Step 15: 提交阶段 2**

Run:

```bash
git add packages/sdk/src/session.rs apps/cli/src/tui
git commit -m "refactor: TUI chat state 改用 SDK message DTO (refs #47)" -m "- ChatState/UiEvent 使用 sdk::ChatMessage 与 ToolResultImage" -m "- 移除 TUI runtime message 转换 helper"
```

---

### Task 3: Agent progress 输出改用 SDK DTO

**Files:**
- Modify: `apps/cli/src/tui/output_area/tool_display/agent.rs`
- Modify: `apps/cli/src/tui/output_area/tool_display/common.rs`
- Modify: `apps/cli/src/tui/output_area/tool_display_agent_tests.rs`

- [ ] **Step 1: 修改测试使用 SDK DTO**

Edit `apps/cli/src/tui/output_area/tool_display_agent_tests.rs` imports:

```rust
use super::super::OutputArea;
use sdk::{AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView};
```

Replace helper return types:

```rust
fn tool_calls_event(sequence: usize, calls: Vec<AgentToolCallProgressView>) -> AgentProgressEventView {
    AgentProgressEventView {
        sequence,
        kind: AgentProgressKindView::ToolCalls { calls },
    }
}

fn message_event(sequence: usize, text: &str) -> AgentProgressEventView {
    AgentProgressEventView {
        sequence,
        kind: AgentProgressKindView::Message {
            text: text.to_string(),
        },
    }
}

fn call(id: &str, name: &str, summary: &str) -> AgentToolCallProgressView {
    AgentToolCallProgressView {
        id: id.to_string(),
        name: name.to_string(),
        input: serde_json::json!({}),
        summary: summary.to_string(),
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```bash
cargo test -p cli tui::output_area::tool_display_agent_tests -- --nocapture
```

Expected: FAIL，函数参数仍期待 runtime `AgentProgressEvent`。

- [ ] **Step 3: 修改 output_area agent 显示类型**

Edit `apps/cli/src/tui/output_area/tool_display/agent.rs`:

```rust
use sdk::{AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView};

use crate::tui::output_area::{LineStyle, OutputLine, INDENT};

use super::common::format_agent_tool_calls;

impl super::super::OutputArea {
    pub fn push_agent_progress(&mut self, tool_id: &str, event: AgentProgressEventView) {
        match event.kind {
            AgentProgressKindView::ToolCalls { calls } => {
                self.push_agent_tool_calls(tool_id, &calls)
            }
            AgentProgressKindView::Message { text } => {
                self.push_tool_progress(tool_id, &text);
            }
        }
    }

    fn push_agent_tool_calls(&mut self, tool_id: &str, calls: &[AgentToolCallProgressView]) {
        self.finish_streaming();
        let summary = format_agent_tool_calls(calls);
        let content = format!("{INDENT}↳ {summary}");
        if let Some(line) = self.lines.iter_mut().rev().find(|line| {
            line.tool_id.as_deref() == Some(tool_id)
                && line.content.starts_with(&format!("{INDENT}↳ "))
        }) {
            line.content = content;
            line.style = LineStyle::System;
            return;
        }

        let progress_line = OutputLine {
            content,
            style: LineStyle::System,
            tool_id: Some(tool_id.to_string()),
            spans: None,
        };
        let insert_at = self.tool_insert_position(tool_id);
        self.insert_lines_at(insert_at, vec![progress_line]);
    }
```

Keep the existing `push_tool_progress()` and `tool_insert_position()` bodies unchanged.

- [ ] **Step 4: 修改 common formatter 类型**

Edit `apps/cli/src/tui/output_area/tool_display/common.rs` import and function signature:

```rust
use sdk::AgentToolCallProgressView;
```

```rust
pub(super) fn format_agent_tool_calls(calls: &[AgentToolCallProgressView]) -> String {
```

- [ ] **Step 5: 运行阶段验证**

Run:

```bash
cargo fmt --all
cargo check -p cli
cargo test -p cli tui::output_area::tool_display_agent_tests -- --nocapture
```

Expected: all PASS.

- [ ] **Step 6: 提交阶段 3**

Run:

```bash
git add apps/cli/src/tui/output_area/tool_display/agent.rs apps/cli/src/tui/output_area/tool_display/common.rs apps/cli/src/tui/output_area/tool_display_agent_tests.rs
git commit -m "refactor: TUI agent progress 使用 SDK DTO (refs #47)"
```

---

### Task 4: Reflection / clipboard / memory / skill SDK view DTO

**Files:**
- Modify: `packages/sdk/src/tui.rs`
- Modify: `packages/sdk/src/lib.rs`
- Modify: `agent/runtime/src/client.rs`
- Modify: `apps/cli/src/tui/core/event.rs`
- Modify: `apps/cli/src/tui/core/state/chat.rs`
- Modify: `apps/cli/src/tui/core/state/session.rs`
- Modify: `apps/cli/src/tui/input/paste_handler.rs`
- Modify: `apps/cli/src/tui/core/cmd_exec.rs`
- Modify: `apps/cli/src/tui/core/slash/reflection.rs`
- Modify: `apps/cli/src/tui/core/update/ui_event.rs`
- Modify: `apps/cli/src/tui/core/runtime.rs`
- Modify: `apps/cli/src/tui/completion/*.rs`

- [ ] **Step 1: SDK TUI DTO 测试**

Edit `packages/sdk/src/tui.rs` add tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_image_view_keeps_render_fields() {
        let image = ClipboardImageView {
            base64: "abc".to_string(),
            media_type: "image/png".to_string(),
            final_size: 3,
            display_path: Some("/tmp/a.png".to_string()),
            width: Some(10),
            height: Some(20),
        };

        assert_eq!(image.base64, "abc");
        assert_eq!(image.media_type, "image/png");
        assert_eq!(image.final_size, 3);
        assert_eq!(image.display_path.as_deref(), Some("/tmp/a.png"));
    }

    #[test]
    fn test_reflection_output_view_counts_suggestions() {
        let output = ReflectionOutputView {
            content: "summary".to_string(),
            input_tokens: 1,
            output_tokens: 2,
            suggested_memories: vec![ReflectionMemorySuggestionView {
                content: "remember".to_string(),
                layer: "project".to_string(),
            }],
            outdated_memories: vec!["old".to_string()],
        };

        assert_eq!(output.suggested_memories.len(), 1);
        assert_eq!(output.outdated_memories.len(), 1);
        assert_eq!(output.content, "summary");
    }

    #[test]
    fn test_memory_config_view_default_is_disabled_safe() {
        let config = MemoryConfigView::default();

        assert!(!config.enabled);
        assert_eq!(config.max_entries, 0);
        assert!(!config.reflection.enabled);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```bash
cargo test -p sdk tui -- --nocapture
```

Expected: FAIL，DTO 未定义。

- [ ] **Step 3: 新增 SDK TUI DTO**

Edit `packages/sdk/src/tui.rs` after `TaskStatusView`:

```rust
/// TUI 可渲染的图片输入视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardImageView {
    pub base64: String,
    pub media_type: String,
    pub final_size: usize,
    pub display_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl From<ClipboardImageView> for crate::ToolResultImage {
    fn from(value: ClipboardImageView) -> Self {
        Self {
            base64: value.base64,
            media_type: value.media_type,
        }
    }
}

/// Reflection 建议记忆视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionMemorySuggestionView {
    pub content: String,
    pub layer: String,
}

/// Reflection 输出视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionOutputView {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub suggested_memories: Vec<ReflectionMemorySuggestionView>,
    pub outdated_memories: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflectionConfigView {
    pub enabled: bool,
    pub interval_turns: usize,
    pub auto_apply_suggestions: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryConfigView {
    pub enabled: bool,
    pub max_entries: usize,
    pub similarity_threshold: f32,
    pub reflection: ReflectionConfigView,
}

impl Default for MemoryConfigView {
    fn default() -> Self {
        Self {
            enabled: false,
            max_entries: 0,
            similarity_threshold: 0.0,
            reflection: ReflectionConfigView::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillView {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub content: String,
    pub source: Option<String>,
}
```

Change `TuiLaunchContext` to non-generic:

```rust
pub struct TuiLaunchContext {
    pub session_id: String,
    pub cwd: PathBuf,
    pub model_display: String,
    pub memory_config: MemoryConfigView,
    pub skills_map: std::collections::HashMap<String, SkillView>,
    pub initial_resume_id: Option<String>,
}
```

- [ ] **Step 4: re-export TUI DTO**

Edit `packages/sdk/src/lib.rs`:

```rust
pub use tui::{
    ChatEventSink, ChatHandle, ClipboardImageView, MemoryConfigView, QueueDrainPort,
    ReflectionConfigView, ReflectionMemorySuggestionView, ReflectionOutputView, SkillView,
    TaskStatusView, TuiLaunchContext,
};
```

- [ ] **Step 5: runtime 增加 DTO 转换函数**

Edit `agent/runtime/src/client.rs` imports to include:

```rust
ClipboardImageView, MemoryConfigView, ReflectionConfigView, ReflectionMemorySuggestionView,
ReflectionOutputView, SkillView,
```

Add functions near helpers:

```rust
fn memory_config_to_sdk(config: crate::api::core::config::MemoryConfig) -> MemoryConfigView {
    MemoryConfigView {
        enabled: config.enabled,
        max_entries: config.max_entries,
        similarity_threshold: config.similarity_threshold,
        reflection: ReflectionConfigView {
            enabled: config.reflection.enabled,
            interval_turns: config.reflection.interval_turns,
            auto_apply_suggestions: config.reflection.auto_apply_suggestions,
        },
    }
}

fn skill_to_sdk(skill: Skill) -> SkillView {
    SkillView {
        name: skill.name,
        aliases: skill.aliases,
        description: skill.description,
        content: skill.content,
        source: skill.source.map(|path| path.display().to_string()),
    }
}

fn processed_image_to_sdk(image: crate::api::image::ProcessedImage) -> ClipboardImageView {
    ClipboardImageView {
        base64: image.base64,
        media_type: image.media_type,
        final_size: image.final_size,
        display_path: image.path.map(|path| path.display().to_string()),
        width: image.width,
        height: image.height,
    }
}

fn reflection_output_to_sdk(
    output: crate::api::reflection::ReflectionOutput,
    input_tokens: u32,
    output_tokens: u32,
) -> ReflectionOutputView {
    ReflectionOutputView {
        content: crate::api::reflection::ReflectionEngine::format_output(&output),
        input_tokens,
        output_tokens,
        suggested_memories: output
            .suggested_memories
            .into_iter()
            .map(|memory| ReflectionMemorySuggestionView {
                content: memory.content,
                layer: format!("{:?}", memory.layer).to_lowercase(),
            })
            .collect(),
        outdated_memories: output.outdated_memories.into_iter().map(|item| item.id).collect(),
    }
}
```

If actual `ProcessedImage` or reflection field names differ, inspect their definitions and adjust exactly, then update this plan section in the same commit.

- [ ] **Step 6: TUI event/state 使用新 DTO**

Edit `apps/cli/src/tui/core/event.rs`:

```rust
      ClipboardImage(sdk::ClipboardImageView),
```

```rust
      ReflectionDone {
          output: sdk::ReflectionOutputView,
      },
```

Edit `apps/cli/src/tui/core/state/session.rs`:

```rust
pub(crate) struct SessionState {
    pub session_id: String,
    pub cwd: PathBuf,
    pub session_created_at: Option<String>,
    pub cached_sessions: Vec<(String, String)>,
    pub current_model_display: String,
    pub memory_config: sdk::MemoryConfigView,
}
```

- [ ] **Step 7: clipboard/image 读取移出 TUI runtime import**

Add these methods to `sdk::AgentClient`:

```rust
async fn read_clipboard_image(&self) -> Result<ClipboardImageView, SdkError>;
async fn process_image_file(&self, path: String) -> Result<ClipboardImageView, SdkError>;
```

Implement them in runtime using `processed_image_to_sdk()`. Update `paste_handler.rs` and `cmd_exec.rs` to call the SDK methods through `self.agent_client`.

Minimal TUI compile target:

- `UiEvent::ClipboardImage` always carries `sdk::ClipboardImageView`.
- `pending_images` stores `Vec<sdk::ToolResultImage>` or convert immediately when receiving clipboard:

```rust
              UiEvent::ClipboardImage(img) => {
                  self.chat.pending_images.push(img.clone().into());
                  self.input_area
                      .set_pending_images(self.chat.pending_images.len());
              }
```

- Store `pending_images: Vec<sdk::ClipboardImageView>` so `/images` can display `final_size`, `media_type`, and `display_path` without re-reading image content. Convert to `ToolResultImage` in `update_enter` with `map(Into::into)`.

- [ ] **Step 8: reflection 下沉到 SDK/AgentClient**

Add trait methods to `sdk::AgentClient`:

```rust
async fn run_reflection(&self, messages: Vec<ChatMessage>) -> Result<ReflectionOutputView, SdkError>;
async fn apply_reflection(&self, output: ReflectionOutputView) -> Result<String, SdkError>;
```

Runtime implementation owns `ReflectionEngine` and memory store. TUI `reflection.rs` then:

- checks `self.session.memory_config.enabled` using `MemoryConfigView`
- calls `agent_client.run_reflection(self.chat.messages.clone()).await`
- sends `UiEvent::ReflectionDone { output }`
- `/reflect apply` calls `agent_client.apply_reflection(output).await` and prints returned summary

- [ ] **Step 9: skill view 替换 runtime Skill**

Edit `apps/cli/src/tui/core/mod.rs`:

```rust
pub skills: std::collections::HashMap<String, sdk::SkillView>,
```

Edit `apps/cli/src/tui/core/runtime.rs`:

```rust
pub fn set_skills(&mut self, skills: std::collections::HashMap<String, sdk::SkillView>) {
    self.skills = skills;
}

pub(crate) fn find_skill_by_alias(&self, alias: &str) -> Option<&sdk::SkillView> {
    self.skills
        .values()
        .find(|s| s.name == alias || s.aliases.iter().any(|a| a == alias))
}
```

Update completion modules to read `sdk::SkillView` fields.

- [ ] **Step 10: 运行阶段验证**

Run:

```bash
cargo fmt --all
cargo check -p sdk
cargo check -p runtime
cargo check -p cli
cargo test -p sdk tui -- --nocapture
cargo test -p cli tui::core::slash_tests -- --nocapture
```

Expected: all PASS.

- [ ] **Step 11: 提交阶段 4**

Run:

```bash
git add packages/sdk/src/tui.rs packages/sdk/src/lib.rs agent/runtime/src/client.rs apps/cli/src/tui
git commit -m "refactor: TUI reflection image skill 改用 SDK view (refs #47)" -m "- 新增 clipboard/reflection/memory/skill SDK DTO" -m "- 移除 TUI 对 runtime reflection/image/skill 类型依赖"
```

---

### Task 5: TUI 启动与 slash/runtime 能力边界收口

**Files:**
- Modify: `packages/sdk/src/client.rs`
- Modify: `packages/sdk/src/session.rs`
- Modify: `agent/runtime/src/client.rs`
- Modify: `apps/cli/src/runtime_adapter.rs`
- Modify: `apps/cli/src/tui/session/session_lifecycle.rs`
- Modify: `apps/cli/src/tui/core/run_loop.rs`
- Modify: `apps/cli/src/tui/core/runtime.rs`
- Modify: `apps/cli/src/tui/core/slash.rs`
- Modify: `apps/cli/src/tui/core/cmd_exec.rs`

- [ ] **Step 1: SDK 增加 TUI session/load/compact/context 能力**

Add to `packages/sdk/src/session.rs`:

```rust
#[derive(Debug, Clone)]
pub struct TuiSessionView {
    pub id: String,
    pub created_at: String,
    pub messages: Vec<ChatMessage>,
    pub workspace: Option<crate::WorkspaceContextView>,
    pub task_snapshot_present: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactResultView {
    pub was_compacted: bool,
    pub old_len: usize,
    pub new_len: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextUsageView {
    pub estimated_tokens: usize,
    pub context_size: usize,
    pub percent: usize,
    pub message_count: usize,
}
```

Re-export them from `packages/sdk/src/lib.rs`.

Add to `packages/sdk/src/client.rs` trait:

```rust
async fn load_tui_session(&self, id: &str) -> Result<super::session::TuiSessionView, super::SdkError>;
async fn compact_current_messages(
    &self,
    messages: Vec<super::ChatMessage>,
    system_prompt_text: String,
    context_size: usize,
) -> Result<(Vec<super::ChatMessage>, super::session::CompactResultView), super::SdkError>;
fn context_usage(
    &self,
    messages: &[super::ChatMessage],
    system_prompt_text: &str,
    context_size: usize,
) -> super::session::ContextUsageView;
```

- [ ] **Step 2: runtime 实现这些 SDK 能力**

In `agent/runtime/src/client.rs` implement:

- `load_tui_session()` calls runtime `load_session`, sanitizes/deep-cleans messages, restores task snapshot into task store, converts messages/workspace to SDK DTO.
- `compact_current_messages()` converts SDK messages to runtime messages, calls `compact::compact_messages`, converts back to SDK.
- `context_usage()` converts SDK messages to runtime messages and uses `compact::estimate_messages_tokens` / `estimate_tokens`.

- [ ] **Step 3: TUI run/run_loop 删除 runtime 参数**

Change `App::run` signature in `apps/cli/src/tui/session/session_lifecycle.rs` to accept only SDK-facing setup values:

```rust
pub async fn run(
    &mut self,
    context_size: usize,
    verbose: bool,
    allow_all: bool,
    resume_id: Option<String>,
) -> io::Result<()>
```

Use `self.agent_client` for task status and session load. Remove parameters:

- `LlmClient`
- `ToolRegistry`
- `SystemBlock`
- `AgentRunner`
- `TaskStore`
- `agent_semaphore`

Change `run_loop()` signature similarly:

```rust
pub(crate) async fn run_loop(
    &mut self,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()>
```

- [ ] **Step 4: CmdExecutor 删除 runtime 基础设施字段**

Edit `apps/cli/src/tui/core/cmd_exec.rs`:

```rust
pub struct CmdExecutor {
    pub hook_notifier: Option<std::sync::Arc<dyn sdk::AgentClient>>,
}
```

If hook notification is not exposed via SDK yet, use `agent_client` directly in `App` for notification or keep hook runner outside `apps/cli/src/tui/**` by moving command execution to `apps/cli/src/runtime_adapter.rs`.

- [ ] **Step 5: slash runtime 依赖替换为 SDK calls**

In `apps/cli/src/tui/core/slash.rs`:

- `/compact` calls `agent_client.compact_current_messages(...)`.
- `/context` calls `agent_client.context_usage(...)`.
- `/save` already calls `save_current_session()`.
- `/paste` calls SDK image method from Task 4.
- Generic `CommandRegistry` path should be moved to `apps/cli/src/runtime_adapter.rs` as `execute_slash_command(...) -> sdk::SlashCommandResultView` or explicitly left out of TUI if not used by core commands.

Do not leave `::runtime` in this file.

- [ ] **Step 6: refresh session cache via SDK**

Edit `apps/cli/src/tui/core/runtime.rs`:

```rust
pub async fn refresh_session_cache(&mut self) {
    let Some(agent_client) = &self.agent_client else {
        self.session.cached_sessions.clear();
        return;
    };
    match agent_client.list_sessions().await {
        Ok(sessions) => {
            self.session.cached_sessions = sessions
                .into_iter()
                .take(20)
                .map(|s| {
                    let summary = s.summary.or(s.preview).unwrap_or_else(|| s.id.clone());
                    (s.id, summary)
                })
                .collect();
        }
        Err(e) => {
            log::warn!("failed to refresh session cache via SDK: {e}");
            self.session.cached_sessions.clear();
        }
    }
}
```

- [ ] **Step 7: 运行阶段验证**

Run:

```bash
python3 - <<'PY'
from pathlib import Path
bad=[]
for p in Path('apps/cli/src/tui').rglob('*.rs'):
    text=p.read_text()
    if '::runtime' in text or 'runtime::api' in text or 'runtime::' in text:
        bad.append(str(p))
if bad:
    print('\n'.join(bad))
    raise SystemExit(1)
PY
cargo fmt --all
cargo check -p sdk
cargo check -p runtime
cargo check -p cli
cargo test -p cli tui::core::slash_tests -- --nocapture
cargo test -p cli tui::session::processing -- --nocapture
```

Expected: Python scanner prints nothing; all Rust commands PASS.

- [ ] **Step 8: 提交阶段 5**

Run:

```bash
git add packages/sdk/src/client.rs packages/sdk/src/session.rs packages/sdk/src/lib.rs agent/runtime/src/client.rs apps/cli/src/runtime_adapter.rs apps/cli/src/tui
git commit -m "refactor: 收口 TUI 启动与 slash runtime 边界 (refs #47)" -m "- TUI run/slash/session resume 改走 SDK 能力" -m "- apps/cli/src/tui 不再直接引用 runtime 类型"
```

---

### Task 6: 架构守卫、文档、最终验证与合并

**Files:**
- Modify: `.agents/hooks/check-forbidden-imports.sh`
- Modify: `docs/feature/active.md`
- Modify: `docs/feature/specs/047-tui-sdk-dto-boundary-design.md`

- [ ] **Step 1: 更新 architecture guard**

Edit `.agents/hooks/check-forbidden-imports.sh` in the Python violation scan, add before existing specific TUI checks:

```python
          if 'apps/cli/src/tui/' in str(rel) and any(fragment in line for fragment in [
              '::runtime',
              'runtime::api',
              'runtime::',
          ]):
              violations.append(f"{rel}:{lineno}: TUI must depend on sdk DTO/AgentClient, not runtime internals: {line.strip()}")
```

- [ ] **Step 2: 运行守卫确认通过**

Run:

```bash
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh
```

Expected: PASS，没有 TUI runtime import violation。

- [ ] **Step 3: 更新 #47 active 文档**

Edit `docs/feature/active.md` #47 detail paragraph to include:

```markdown
P12 TUI SDK DTO 边界迁移已完成：`sdk::ChatEvent` 的 images / agent progress / workspace 已改为强类型 SDK DTO；`apps/cli/src/tui/**` 的 chat/message/image/reflection/skill/session resume/slash 边界不再直接承接 runtime 类型；runtime domain ⇄ SDK DTO 转换集中在 `agent/runtime::AgentClientImpl` 与 CLI composition root。架构守卫已新增 TUI 禁止 `::runtime` / `runtime::api` 规则。
```

- [ ] **Step 4: 完整验证**

Run:

```bash
cargo check -p sdk
cargo check -p runtime
cargo check -p cli
cargo test -p sdk chat -- --nocapture
cargo test -p sdk tui -- --nocapture
cargo test -p sdk session -- --nocapture
cargo test -p cli tui::session::processing -- --nocapture
cargo test -p cli tui::output_area::tool_display_agent_tests -- --nocapture
cargo test -p cli tui::core::slash_tests -- --nocapture
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh
```

Expected: all PASS.

- [ ] **Step 5: 提交阶段 6**

Run:

```bash
git add .agents/hooks/check-forbidden-imports.sh docs/feature/active.md docs/feature/specs/047-tui-sdk-dto-boundary-design.md
git commit -m "chore: 加固 TUI SDK DTO 边界守卫 (refs #47)" -m "- 禁止 apps/cli/src/tui 重新引入 runtime 类型" -m "- 更新 #47 DTO 边界迁移状态"
```

- [ ] **Step 6: 合并回 main 并在 main 验证**

Run:

```bash
git status --short --branch
git checkout main
git merge --no-ff feature/47-tui-dto-boundary -m "Merge branch 'feature/47-tui-dto-boundary'"
cargo check -p sdk
cargo check -p runtime
cargo check -p cli
AEMEATH_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh
```

Expected: merge succeeds; checks PASS.

- [ ] **Step 7: 清理 worktree/branch**

Run from main workspace:

```bash
git worktree remove .worktrees/feature-47-tui-dto-boundary
git branch -d feature/47-tui-dto-boundary
git status --short --branch
```

Expected: worktree removed; branch deleted; main clean except expected ahead count.
