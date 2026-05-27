# TUI M4 Runtime Diagnostic Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 `RuntimeModel` 与 `DiagnosticModel`，让 status line、error、warning、prompt、hook notice、orphan/late event 有统一来源。

**Architecture:** 新增纯 model context 保存 runtime snapshot 与 diagnostic notices。Status/Dialog/Output ViewAssembler 从 RuntimeModel/DiagnosticModel 读取显示摘要，逐步替换 status line 和 output/dialog 中散落的错误/运行状态字段。

**Tech Stack:** Rust 2021、现有 `apps/cli` crate、ratatui legacy render、`cargo test -p cli`、architecture guard。

---

## File Structure

- Create: `apps/cli/src/tui/model/runtime/mod.rs` — RuntimeModel 出口。
- Create: `apps/cli/src/tui/model/runtime/model.rs` — RuntimeModel root。
- Create: `apps/cli/src/tui/model/runtime/intent.rs` — RuntimeIntent。
- Create: `apps/cli/src/tui/model/runtime/change.rs` — RuntimeChange。
- Create: `apps/cli/src/tui/model/runtime/workspace.rs` — WorkspaceState。
- Create: `apps/cli/src/tui/model/runtime/processing_job.rs` — ProcessingJob。
- Create: `apps/cli/src/tui/model/runtime/usage.rs` — UsageSummary。
- Create: `apps/cli/src/tui/model/runtime/task_status.rs` — TaskStatusSnapshot。
- Create: `apps/cli/src/tui/model/diagnostic/mod.rs` — DiagnosticModel 出口。
- Create: `apps/cli/src/tui/model/diagnostic/model.rs` — DiagnosticModel root。
- Create: `apps/cli/src/tui/model/diagnostic/intent.rs` — DiagnosticIntent。
- Create: `apps/cli/src/tui/model/diagnostic/change.rs` — DiagnosticChange。
- Create: `apps/cli/src/tui/model/diagnostic/notice.rs` — DiagnosticNotice。
- Create: `apps/cli/src/tui/model/diagnostic/prompt.rs` — ActivePrompt。
- Modify: `apps/cli/src/tui/model/mod.rs` — 导出 runtime/diagnostic。
- Modify: `apps/cli/src/tui/view_assembler/status.rs` — 从 Runtime/Diagnostic 组装 StatusLineViewModel。
- Modify: `apps/cli/src/tui/view_assembler/dialog.rs` — 从 DiagnosticModel 组装 DialogViewModel。

## Task 1: Add RuntimeModel

**Files:**
- Create: `apps/cli/src/tui/model/runtime/workspace.rs`
- Create: `apps/cli/src/tui/model/runtime/processing_job.rs`
- Create: `apps/cli/src/tui/model/runtime/usage.rs`
- Create: `apps/cli/src/tui/model/runtime/task_status.rs`
- Create: `apps/cli/src/tui/model/runtime/intent.rs`
- Create: `apps/cli/src/tui/model/runtime/change.rs`
- Create: `apps/cli/src/tui/model/runtime/model.rs`
- Create: `apps/cli/src/tui/model/runtime/mod.rs`
- Modify: `apps/cli/src/tui/model/mod.rs`

- [ ] **Step 1: Write failing RuntimeModel tests**

Create `apps/cli/src/tui/model/runtime/model.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::runtime::intent::RuntimeIntent;

    #[test]
    fn test_runtime_updates_workspace() {
        let mut model = RuntimeModel::default();
        let changes = model.apply(RuntimeIntent::UpdateWorkspace { cwd: "/repo".to_string(), worktree: Some("feature/x".to_string()) });
        assert_eq!(model.workspace.cwd.as_deref(), Some("/repo"));
        assert!(changes.iter().any(|change| matches!(change, RuntimeChange::WorkspaceChanged { .. })));
    }

    #[test]
    fn test_runtime_records_usage() {
        let mut model = RuntimeModel::default();
        model.apply(RuntimeIntent::RecordUsage { input_tokens: 10, output_tokens: 5, cost_usd: 0.01 });
        assert_eq!(model.usage.input_tokens, 10);
        assert_eq!(model.usage.output_tokens, 5);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::model::runtime::model::tests
```

Expected: FAIL because runtime model is missing.

- [ ] **Step 3: Implement RuntimeModel files**

Create `apps/cli/src/tui/model/runtime/workspace.rs`:

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceState {
    pub cwd: Option<String>,
    pub worktree: Option<String>,
}
```

Create `apps/cli/src/tui/model/runtime/processing_job.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessingJob {
    pub id: String,
    pub chat_id: Option<String>,
    pub status: ProcessingStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessingStatus {
    Starting,
    Running,
    Finishing,
    Finished,
    Failed,
}
```

Create `apps/cli/src/tui/model/runtime/usage.rs`:

```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}
```

Create `apps/cli/src/tui/model/runtime/task_status.rs`:

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskStatusSnapshot {
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
}
```

Create `apps/cli/src/tui/model/runtime/intent.rs`:

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeIntent {
    UpdateWorkspace { cwd: String, worktree: Option<String> },
    RecordUsage { input_tokens: u64, output_tokens: u64, cost_usd: f64 },
    UpdateTaskStatus { total: usize, completed: usize, in_progress: usize },
}
```

Create `apps/cli/src/tui/model/runtime/change.rs`:

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeChange {
    WorkspaceChanged { cwd: String, worktree: Option<String> },
    UsageChanged { input_tokens: u64, output_tokens: u64, cost_usd: f64 },
    TaskStatusChanged { total: usize, completed: usize, in_progress: usize },
}
```

Create `apps/cli/src/tui/model/runtime/model.rs`:

```rust
use super::change::RuntimeChange;
use super::intent::RuntimeIntent;
use super::task_status::TaskStatusSnapshot;
use super::usage::UsageSummary;
use super::workspace::WorkspaceState;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuntimeModel {
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub workspace: WorkspaceState,
    pub usage: UsageSummary,
    pub task_status: TaskStatusSnapshot,
}

impl RuntimeModel {
    pub fn apply(&mut self, intent: RuntimeIntent) -> Vec<RuntimeChange> {
        match intent {
            RuntimeIntent::UpdateWorkspace { cwd, worktree } => {
                self.workspace.cwd = Some(cwd.clone());
                self.workspace.worktree = worktree.clone();
                vec![RuntimeChange::WorkspaceChanged { cwd, worktree }]
            }
            RuntimeIntent::RecordUsage { input_tokens, output_tokens, cost_usd } => {
                self.usage.input_tokens += input_tokens;
                self.usage.output_tokens += output_tokens;
                self.usage.cost_usd += cost_usd;
                vec![RuntimeChange::UsageChanged { input_tokens: self.usage.input_tokens, output_tokens: self.usage.output_tokens, cost_usd: self.usage.cost_usd }]
            }
            RuntimeIntent::UpdateTaskStatus { total, completed, in_progress } => {
                self.task_status = TaskStatusSnapshot { total, completed, in_progress };
                vec![RuntimeChange::TaskStatusChanged { total, completed, in_progress }]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::runtime::intent::RuntimeIntent;

    #[test]
    fn test_runtime_updates_workspace() {
        let mut model = RuntimeModel::default();
        let changes = model.apply(RuntimeIntent::UpdateWorkspace { cwd: "/repo".to_string(), worktree: Some("feature/x".to_string()) });
        assert_eq!(model.workspace.cwd.as_deref(), Some("/repo"));
        assert!(changes.iter().any(|change| matches!(change, RuntimeChange::WorkspaceChanged { .. })));
    }

    #[test]
    fn test_runtime_records_usage() {
        let mut model = RuntimeModel::default();
        model.apply(RuntimeIntent::RecordUsage { input_tokens: 10, output_tokens: 5, cost_usd: 0.01 });
        assert_eq!(model.usage.input_tokens, 10);
        assert_eq!(model.usage.output_tokens, 5);
    }
}
```

Create `apps/cli/src/tui/model/runtime/mod.rs`:

```rust
pub mod change;
pub mod intent;
pub mod model;
pub mod processing_job;
pub mod task_status;
pub mod usage;
pub mod workspace;

pub use change::RuntimeChange;
pub use intent::RuntimeIntent;
pub use model::RuntimeModel;
pub use processing_job::{ProcessingJob, ProcessingStatus};
pub use task_status::TaskStatusSnapshot;
pub use usage::UsageSummary;
pub use workspace::WorkspaceState;
```

Modify `apps/cli/src/tui/model/mod.rs`:

```rust
pub mod conversation;
pub mod input;
pub mod runtime;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::model::runtime::model::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/model
git commit -m "feat: add TUI runtime model"
```

## Task 2: Add DiagnosticModel

**Files:**
- Create: `apps/cli/src/tui/model/diagnostic/notice.rs`
- Create: `apps/cli/src/tui/model/diagnostic/prompt.rs`
- Create: `apps/cli/src/tui/model/diagnostic/intent.rs`
- Create: `apps/cli/src/tui/model/diagnostic/change.rs`
- Create: `apps/cli/src/tui/model/diagnostic/model.rs`
- Create: `apps/cli/src/tui/model/diagnostic/mod.rs`
- Modify: `apps/cli/src/tui/model/mod.rs`

- [ ] **Step 1: Write failing DiagnosticModel tests**

Create `apps/cli/src/tui/model/diagnostic/model.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::diagnostic::intent::DiagnosticIntent;
    use crate::tui::model::diagnostic::notice::DiagnosticSeverity;

    #[test]
    fn test_records_notice() {
        let mut model = DiagnosticModel::default();
        let changes = model.apply(DiagnosticIntent::RecordNotice { severity: DiagnosticSeverity::Warning, message: "late event".to_string() });
        assert_eq!(model.notices.len(), 1);
        assert!(changes.iter().any(|change| matches!(change, DiagnosticChange::NoticeRecorded { .. })));
    }

    #[test]
    fn test_opens_and_answers_prompt() {
        let mut model = DiagnosticModel::default();
        model.apply(DiagnosticIntent::OpenPrompt { id: "p1".to_string(), question: "继续?".to_string() });
        assert!(model.active_prompt.is_some());
        model.apply(DiagnosticIntent::AnswerPrompt { answer: "是".to_string() });
        assert!(model.active_prompt.is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::model::diagnostic::model::tests
```

Expected: FAIL because diagnostic model is missing.

- [ ] **Step 3: Implement DiagnosticModel**

Create `apps/cli/src/tui/model/diagnostic/notice.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticNotice {
    pub id: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}
```

Create `apps/cli/src/tui/model/diagnostic/prompt.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivePrompt {
    pub id: String,
    pub question: String,
}
```

Create `apps/cli/src/tui/model/diagnostic/intent.rs`:

```rust
use super::notice::DiagnosticSeverity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticIntent {
    RecordNotice { severity: DiagnosticSeverity, message: String },
    OpenPrompt { id: String, question: String },
    AnswerPrompt { answer: String },
    DismissNotice { id: String },
}
```

Create `apps/cli/src/tui/model/diagnostic/change.rs`:

```rust
use super::notice::DiagnosticSeverity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticChange {
    NoticeRecorded { id: String, severity: DiagnosticSeverity },
    PromptOpened { id: String },
    PromptAnswered { answer: String },
    NoticeDismissed { id: String },
}
```

Create `apps/cli/src/tui/model/diagnostic/model.rs`:

```rust
use super::change::DiagnosticChange;
use super::intent::DiagnosticIntent;
use super::notice::{DiagnosticNotice, DiagnosticSeverity};
use super::prompt::ActivePrompt;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticModel {
    pub notices: Vec<DiagnosticNotice>,
    pub active_prompt: Option<ActivePrompt>,
    next_notice_id: usize,
}

impl DiagnosticModel {
    pub fn apply(&mut self, intent: DiagnosticIntent) -> Vec<DiagnosticChange> {
        match intent {
            DiagnosticIntent::RecordNotice { severity, message } => {
                self.next_notice_id += 1;
                let id = format!("notice-{}", self.next_notice_id);
                self.notices.push(DiagnosticNotice { id: id.clone(), severity, message });
                vec![DiagnosticChange::NoticeRecorded { id, severity }]
            }
            DiagnosticIntent::OpenPrompt { id, question } => {
                self.active_prompt = Some(ActivePrompt { id: id.clone(), question });
                vec![DiagnosticChange::PromptOpened { id }]
            }
            DiagnosticIntent::AnswerPrompt { answer } => {
                self.active_prompt = None;
                vec![DiagnosticChange::PromptAnswered { answer }]
            }
            DiagnosticIntent::DismissNotice { id } => {
                self.notices.retain(|notice| notice.id != id);
                vec![DiagnosticChange::NoticeDismissed { id }]
            }
        }
    }

    pub fn highest_severity(&self) -> Option<DiagnosticSeverity> {
        if self.notices.iter().any(|notice| notice.severity == DiagnosticSeverity::Error) { return Some(DiagnosticSeverity::Error); }
        if self.notices.iter().any(|notice| notice.severity == DiagnosticSeverity::Warning) { return Some(DiagnosticSeverity::Warning); }
        if self.notices.is_empty() { None } else { Some(DiagnosticSeverity::Info) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::diagnostic::intent::DiagnosticIntent;
    use crate::tui::model::diagnostic::notice::DiagnosticSeverity;

    #[test]
    fn test_records_notice() {
        let mut model = DiagnosticModel::default();
        let changes = model.apply(DiagnosticIntent::RecordNotice { severity: DiagnosticSeverity::Warning, message: "late event".to_string() });
        assert_eq!(model.notices.len(), 1);
        assert!(changes.iter().any(|change| matches!(change, DiagnosticChange::NoticeRecorded { .. })));
    }

    #[test]
    fn test_opens_and_answers_prompt() {
        let mut model = DiagnosticModel::default();
        model.apply(DiagnosticIntent::OpenPrompt { id: "p1".to_string(), question: "继续?".to_string() });
        assert!(model.active_prompt.is_some());
        model.apply(DiagnosticIntent::AnswerPrompt { answer: "是".to_string() });
        assert!(model.active_prompt.is_none());
    }
}
```

Create `apps/cli/src/tui/model/diagnostic/mod.rs`:

```rust
pub mod change;
pub mod intent;
pub mod model;
pub mod notice;
pub mod prompt;

pub use change::DiagnosticChange;
pub use intent::DiagnosticIntent;
pub use model::DiagnosticModel;
pub use notice::{DiagnosticNotice, DiagnosticSeverity};
pub use prompt::ActivePrompt;
```

Modify `apps/cli/src/tui/model/mod.rs`:

```rust
pub mod conversation;
pub mod diagnostic;
pub mod input;
pub mod runtime;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::model::diagnostic::model::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/model
git commit -m "feat: add TUI diagnostic model"
```

## Task 3: Assemble StatusLineViewModel from Runtime and Diagnostic

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/status.rs`

- [ ] **Step 1: Add failing status assembler test**

Append tests to `apps/cli/src/tui/view_assembler/status.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::tui::model::diagnostic::{DiagnosticIntent, DiagnosticModel, DiagnosticSeverity};
    use crate::tui::model::runtime::{RuntimeIntent, RuntimeModel};

    use super::StatusViewAssembler;

    #[test]
    fn test_status_assembler_reads_runtime_and_diagnostic() {
        let mut runtime = RuntimeModel::default();
        runtime.model_id = Some("gpt-5.5".to_string());
        runtime.apply(RuntimeIntent::UpdateWorkspace { cwd: "/repo".to_string(), worktree: None });

        let mut diagnostic = DiagnosticModel::default();
        diagnostic.apply(DiagnosticIntent::RecordNotice { severity: DiagnosticSeverity::Warning, message: "orphan event".to_string() });

        let vm = StatusViewAssembler::assemble_from_models(&runtime, &diagnostic);
        assert!(vm.left.iter().any(|segment| segment.text == "gpt-5.5"));
        assert!(vm.right.iter().any(|segment| segment.text == "/repo"));
        assert!(vm.center.iter().any(|segment| segment.text.contains("warning")));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::view_assembler::status::tests::test_status_assembler_reads_runtime_and_diagnostic
```

Expected: FAIL because `assemble_from_models` is missing.

- [ ] **Step 3: Implement status assembler**

Add to `apps/cli/src/tui/view_assembler/status.rs`:

```rust
use crate::tui::model::diagnostic::{DiagnosticModel, DiagnosticSeverity};
use crate::tui::model::runtime::RuntimeModel;
use crate::tui::view_model::StatusSeverity;

impl StatusViewAssembler {
    pub fn assemble_from_models(runtime: &RuntimeModel, diagnostic: &DiagnosticModel) -> StatusLineViewModel {
        let mut vm = StatusLineViewModel::default();
        if let Some(model_id) = runtime.model_id.as_deref() {
            vm.left.push(StatusSegment { key: "model".to_string(), text: model_id.to_string(), style: SemanticStyle::Accent, priority: 10 });
        }
        if let Some(cwd) = runtime.workspace.cwd.as_deref() {
            vm.right.push(StatusSegment { key: "cwd".to_string(), text: cwd.to_string(), style: SemanticStyle::Muted, priority: 20 });
        }
        match diagnostic.highest_severity() {
            Some(DiagnosticSeverity::Error) => {
                vm.severity = StatusSeverity::Error;
                vm.center.push(StatusSegment { key: "diagnostic".to_string(), text: "error".to_string(), style: SemanticStyle::Error, priority: 1 });
            }
            Some(DiagnosticSeverity::Warning) => {
                vm.severity = StatusSeverity::Warning;
                vm.center.push(StatusSegment { key: "diagnostic".to_string(), text: "warning".to_string(), style: SemanticStyle::Warning, priority: 1 });
            }
            Some(DiagnosticSeverity::Info) => {
                vm.severity = StatusSeverity::Info;
                vm.center.push(StatusSegment { key: "diagnostic".to_string(), text: "info".to_string(), style: SemanticStyle::Muted, priority: 1 });
            }
            None => {}
        }
        vm
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::view_assembler::status::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/view_assembler/status.rs
git commit -m "feat: assemble status from runtime and diagnostics"
```

## Task 4: Assemble DialogViewModel from DiagnosticModel

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/dialog.rs`

- [ ] **Step 1: Add failing dialog assembler test**

Append to `apps/cli/src/tui/view_assembler/dialog.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::tui::model::diagnostic::{DiagnosticIntent, DiagnosticModel};

    use super::DialogViewAssembler;

    #[test]
    fn test_dialog_assembler_uses_active_prompt() {
        let mut diagnostic = DiagnosticModel::default();
        diagnostic.apply(DiagnosticIntent::OpenPrompt { id: "p1".to_string(), question: "允许执行?".to_string() });
        let dialog = DialogViewAssembler::assemble_from_diagnostic(&diagnostic).expect("dialog");
        assert_eq!(dialog.title, "需要确认");
        assert!(dialog.body.contains("允许执行"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::view_assembler::dialog::tests::test_dialog_assembler_uses_active_prompt
```

Expected: FAIL because method is missing.

- [ ] **Step 3: Implement dialog assembler**

Modify `apps/cli/src/tui/view_assembler/dialog.rs`:

```rust
use crate::tui::model::diagnostic::DiagnosticModel;
use crate::tui::view_model::{DialogActionViewModel, DialogKind, DialogViewModel, StatusSeverity};

pub struct DialogViewAssembler;

impl DialogViewAssembler {
    pub fn none() -> Option<DialogViewModel> { None }

    pub fn assemble_from_diagnostic(diagnostic: &DiagnosticModel) -> Option<DialogViewModel> {
        let prompt = diagnostic.active_prompt.as_ref()?;
        Some(DialogViewModel {
            kind: DialogKind::Permission,
            title: "需要确认".to_string(),
            body: prompt.question.clone(),
            actions: vec![
                DialogActionViewModel { id: "yes".to_string(), label: "允许".to_string() },
                DialogActionViewModel { id: "no".to_string(), label: "拒绝".to_string() },
            ],
            default_action: Some("yes".to_string()),
            severity: StatusSeverity::Warning,
        })
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::view_assembler::dialog::tests::test_dialog_assembler_uses_active_prompt
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/view_assembler/dialog.rs
git commit -m "feat: assemble dialog from diagnostics"
```

## Final verification

Run:

```bash
cargo test -p cli tui::model::runtime tui::model::diagnostic tui::view_assembler::status tui::view_assembler::dialog
cargo check -p cli
.agents/hooks/check-architecture-guards.sh
```

Expected: all PASS.

M4 is complete when runtime/status facts and diagnostic notices have model-level sources and ViewAssembler can produce status/dialog ViewModels from those sources.
