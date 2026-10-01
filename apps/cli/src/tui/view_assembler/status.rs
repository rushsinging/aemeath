use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::session_model::SessionModel;
use crate::tui::model::runtime::status_notice::{StatusNotice, StatusNoticeKind};
use crate::tui::model::runtime::workspace::WorktreeKind as ModelWorktreeKind;
use crate::tui::model::runtime_presentation::RuntimePresentation;
use crate::tui::model::workspace_provider::WorkspaceProvider;
use crate::tui::view_model::{
    SemanticStyle, StatusContextViewModel, StatusLineViewModel, StatusNoticeViewKind,
    StatusNoticeViewModel, StatusRuntimeViewModel, StatusSegment, StatusSeverity, StatusViewModel,
    StatusWorktreeKind,
};

pub struct StatusViewAssembler;

impl StatusViewAssembler {
    pub fn assemble_status_view(
        conversation: &ConversationModel,
        presentation: &RuntimePresentation,
        workspace: &WorkspaceProvider,
        session: Option<&SessionModel>,
        diagnostics: &DiagnosticModel,
        permission_mode: &str,
    ) -> StatusViewModel {
        StatusViewModel {
            notice: Self::assemble_notice_view(&conversation.runtime.status_notice),
            runtime: Self::assemble_runtime_view(
                conversation,
                presentation,
                workspace,
                session,
                permission_mode,
            ),
            line: Self::assemble_from_runtime_session(
                conversation,
                presentation,
                workspace,
                session,
                diagnostics,
            ),
            thinking: presentation.thinking(),
        }
    }

    pub fn assemble_notice_view(notice: &StatusNotice) -> StatusNoticeViewModel {
        StatusNoticeViewModel {
            text: notice.text.clone(),
            kind: match notice.kind {
                StatusNoticeKind::Normal => StatusNoticeViewKind::Normal,
                StatusNoticeKind::Success => StatusNoticeViewKind::Success,
                StatusNoticeKind::Warning => StatusNoticeViewKind::Warning,
            },
        }
    }

    /// 由 `RuntimeModel`/`SessionModel` 单向派生 StatusBar 运行态视图模型
    /// （model/session/tps/token/api/context_size/工作目录上下文）。
    ///
    /// StatusBar 不保存运行态或配置 widget mirror；渲染时直接消费本派生结果。
    pub fn assemble_runtime_view(
        conversation: &ConversationModel,
        presentation: &RuntimePresentation,
        workspace: &WorkspaceProvider,
        session: Option<&SessionModel>,
        permission_mode: &str,
    ) -> StatusRuntimeViewModel {
        StatusRuntimeViewModel {
            model: presentation.model_id().map(ToOwned::to_owned),
            // #1616：状态栏直接显示 reasoning 深度字符串（off 也如实显示）。
            reasoning_level: Some(presentation.reasoning_level().as_str()),
            session_id: session.and_then(|s| s.current_session_id.clone()),
            input_tokens: conversation.runtime.usage.input_tokens,
            output_tokens: conversation.runtime.usage.output_tokens,
            context_usage_permille: conversation
                .runtime
                .runtime_status
                .as_ref()
                .map(|status| status.context_budget.usage_permille),
            api_calls: conversation.runtime.usage.api_calls,
            context_size: conversation.runtime.runtime_status.as_ref().map_or_else(
                || presentation.context_size(),
                |status| status.context_budget.context_size,
            ),
            tps: conversation.runtime.live_tps.unwrap_or(0.0),
            context: StatusContextViewModel {
                path_base: workspace.path_base().unwrap_or_default().to_string(),
                workspace_root: workspace.workspace_root().unwrap_or_default().to_string(),
                branch: workspace
                    .branch()
                    .filter(|branch| !branch.trim().is_empty())
                    .map(ToOwned::to_owned),
                kind: match workspace.kind() {
                    ModelWorktreeKind::LinkedWorktree => StatusWorktreeKind::Worktree,
                    _ => StatusWorktreeKind::Main,
                },
                permission_mode: Self::permission_mode_label(permission_mode).to_string(),
            },
        }
    }
    fn permission_mode_label(permission_mode: &str) -> &'static str {
        match permission_mode {
            "auto_read" => "AutoRead",
            "allow_all" => "AllowAll",
            "ask" | "" => "Ask",
            _ => "Ask",
        }
    }

    pub fn assemble_from_runtime_session(
        conversation: &ConversationModel,
        presentation: &RuntimePresentation,
        workspace: &WorkspaceProvider,
        session: Option<&SessionModel>,
        diagnostic: &DiagnosticModel,
    ) -> StatusLineViewModel {
        let mut vm = StatusLineViewModel::default();
        if let Some(provider) = presentation.provider() {
            vm.left.push(StatusSegment {
                key: "provider".to_string(),
                text: provider.to_string(),
                style: SemanticStyle::Muted,
                priority: 5,
            });
        }
        if let Some(model_id) = presentation.model_id() {
            vm.left.push(StatusSegment {
                key: "model".to_string(),
                text: model_id.to_string(),
                style: SemanticStyle::Accent,
                priority: 10,
            });
        }
        if let Some(branch) = workspace.branch() {
            vm.left.push(StatusSegment {
                key: "branch".to_string(),
                text: branch.to_string(),
                style: SemanticStyle::Muted,
                priority: 15,
            });
        }
        if let Some(cwd) = workspace.cwd() {
            vm.right.push(StatusSegment {
                key: "cwd".to_string(),
                text: cwd.to_string(),
                style: SemanticStyle::Muted,
                priority: 20,
            });
        }
        if conversation.runtime.usage.input_tokens > 0
            || conversation.runtime.usage.output_tokens > 0
        {
            vm.right.push(StatusSegment {
                key: "tokens".to_string(),
                text: format!(
                    "{}↑ {}↓",
                    conversation.runtime.usage.input_tokens,
                    conversation.runtime.usage.output_tokens
                ),
                style: SemanticStyle::Muted,
                priority: 30,
            });
        }
        if let Some(tps) = conversation.runtime.live_tps {
            vm.right.push(StatusSegment {
                key: "tps".to_string(),
                text: format!("{tps:.1} tps"),
                style: SemanticStyle::Accent,
                priority: 31,
            });
        }
        if conversation.runtime.task_status.total > 0 {
            vm.right.push(StatusSegment {
                key: "tasks".to_string(),
                text: format!(
                    "tasks {}/{} (+{})",
                    conversation.runtime.task_status.completed,
                    conversation.runtime.task_status.total,
                    conversation.runtime.task_status.in_progress
                ),
                style: SemanticStyle::Muted,
                priority: 40,
            });
        }
        if let Some(session) = session {
            if let Some(id) = session.current_session_id.as_deref() {
                vm.right.push(StatusSegment {
                    key: "session".to_string(),
                    text: format!("session {id}"),
                    style: if session.dirty {
                        SemanticStyle::Warning
                    } else {
                        SemanticStyle::Muted
                    },
                    priority: 50,
                });
            }
        }
        match diagnostic.highest_severity() {
            Some(DiagnosticSeverity::Error) => {
                vm.severity = StatusSeverity::Error;
                vm.center.push(StatusSegment {
                    key: "diagnostic".to_string(),
                    text: "error".to_string(),
                    style: SemanticStyle::Error,
                    priority: 1,
                });
            }
            Some(DiagnosticSeverity::Warning) => {
                vm.severity = StatusSeverity::Warning;
                vm.center.push(StatusSegment {
                    key: "diagnostic".to_string(),
                    text: "warning".to_string(),
                    style: SemanticStyle::Warning,
                    priority: 1,
                });
            }
            Some(DiagnosticSeverity::Info) => {
                vm.severity = StatusSeverity::Info;
                vm.center.push(StatusSegment {
                    key: "diagnostic".to_string(),
                    text: "info".to_string(),
                    style: SemanticStyle::Muted,
                    priority: 1,
                });
            }
            None => {}
        }
        vm
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
