use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::model::RuntimeModel;
use crate::tui::model::runtime::processing_job::ProcessingStatus;
use crate::tui::model::runtime::session_model::SessionModel;
use crate::tui::model::runtime::status_notice::{StatusNotice, StatusNoticeKind};
use crate::tui::model::runtime::workspace::WorktreeKind as ModelWorktreeKind;
use crate::tui::view_model::{
    SemanticStyle, StatusContextViewModel, StatusLineViewModel, StatusNoticeViewKind,
    StatusNoticeViewModel, StatusRuntimeViewModel, StatusSegment, StatusSeverity, StatusViewModel,
    StatusWorktreeKind,
};

pub struct StatusViewAssembler;

impl StatusViewAssembler {
    pub fn assemble_status_view(
        runtime: &RuntimeModel,
        session: Option<&SessionModel>,
        diagnostics: &DiagnosticModel,
    ) -> StatusViewModel {
        StatusViewModel {
            notice: Self::assemble_notice_view(&runtime.status_notice),
            runtime: Self::assemble_runtime_view(runtime, session),
            line: Self::assemble_from_runtime_session(runtime, session, diagnostics),
            thinking: runtime.thinking,
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
    /// StatusBar 不再保存运行态 widget mirror；渲染时直接消费本派生结果。
    /// permission_mode 为启动期配置，不在本派生范围内。
    pub fn assemble_runtime_view(
        runtime: &RuntimeModel,
        session: Option<&SessionModel>,
    ) -> StatusRuntimeViewModel {
        StatusRuntimeViewModel {
            model: runtime.model_id.clone(),
            session_id: session.and_then(|s| s.current_session_id.clone()),
            input_tokens: runtime.usage.input_tokens,
            output_tokens: runtime.usage.output_tokens,
            last_input_tokens: runtime.usage.last_input_tokens,
            api_calls: runtime.usage.api_calls,
            context_size: runtime.usage.context_size,
            tps: runtime.live_tps.unwrap_or(0.0),
            context: StatusContextViewModel {
                path_base: runtime.workspace.path_base.clone().unwrap_or_default(),
                workspace_root: runtime.workspace.workspace_root.clone().unwrap_or_default(),
                branch: runtime
                    .workspace
                    .branch
                    .clone()
                    .filter(|branch| !branch.trim().is_empty()),
                kind: match runtime.workspace.kind {
                    ModelWorktreeKind::LinkedWorktree => StatusWorktreeKind::Worktree,
                    _ => StatusWorktreeKind::Main,
                },
            },
        }
    }
    pub fn assemble_from_runtime_session(
        runtime: &RuntimeModel,
        session: Option<&SessionModel>,
        diagnostic: &DiagnosticModel,
    ) -> StatusLineViewModel {
        let mut vm = StatusLineViewModel::default();
        if let Some(provider) = runtime.provider.as_deref() {
            vm.left.push(StatusSegment {
                key: "provider".to_string(),
                text: provider.to_string(),
                style: SemanticStyle::Muted,
                priority: 5,
            });
        }
        if let Some(model_id) = runtime.model_id.as_deref() {
            vm.left.push(StatusSegment {
                key: "model".to_string(),
                text: model_id.to_string(),
                style: SemanticStyle::Accent,
                priority: 10,
            });
        }
        if let Some(branch) = runtime.workspace.branch.as_deref() {
            vm.left.push(StatusSegment {
                key: "branch".to_string(),
                text: branch.to_string(),
                style: SemanticStyle::Muted,
                priority: 15,
            });
        }
        if let Some(cwd) = runtime.workspace.cwd.as_deref() {
            vm.right.push(StatusSegment {
                key: "cwd".to_string(),
                text: cwd.to_string(),
                style: SemanticStyle::Muted,
                priority: 20,
            });
        }
        if runtime.usage.input_tokens > 0 || runtime.usage.output_tokens > 0 {
            vm.right.push(StatusSegment {
                key: "tokens".to_string(),
                text: format!(
                    "{}↑ {}↓",
                    runtime.usage.input_tokens, runtime.usage.output_tokens
                ),
                style: SemanticStyle::Muted,
                priority: 30,
            });
        }
        if let Some(tps) = runtime.live_tps {
            vm.right.push(StatusSegment {
                key: "tps".to_string(),
                text: format!("{tps:.1} tps"),
                style: SemanticStyle::Accent,
                priority: 31,
            });
        }
        if runtime.task_status.total > 0 {
            vm.right.push(StatusSegment {
                key: "tasks".to_string(),
                text: format!(
                    "tasks {}/{} (+{})",
                    runtime.task_status.completed,
                    runtime.task_status.total,
                    runtime.task_status.in_progress
                ),
                style: SemanticStyle::Muted,
                priority: 40,
            });
        }
        if runtime.processing_jobs.iter().any(|job| {
            matches!(
                job.status,
                ProcessingStatus::Running | ProcessingStatus::Starting
            )
        }) {
            vm.center.push(StatusSegment {
                key: "processing".to_string(),
                text: "processing".to_string(),
                style: SemanticStyle::Running,
                priority: 2,
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
mod tests {
    use crate::tui::model::diagnostic::intent::DiagnosticIntent;
    use crate::tui::model::diagnostic::model::DiagnosticModel;
    use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
    use crate::tui::model::runtime::intent::RuntimeIntent;
    use crate::tui::model::runtime::model::RuntimeModel;

    use super::StatusViewAssembler;
    use crate::tui::model::runtime::session_intent::SessionIntent;
    use crate::tui::model::runtime::session_model::SessionModel;
    use crate::tui::model::runtime::workspace::WorktreeKind;

    #[test]
    fn test_assemble_runtime_view_normal_path_derives_all_fields() {
        let mut runtime = RuntimeModel {
            model_id: Some("glm-5.1".to_string()),
            ..Default::default()
        };
        runtime.apply(RuntimeIntent::RecordLiveTps { tps: 42.0 });
        runtime.apply(RuntimeIntent::WorkspaceSnapshotReceived {
            path_base: Some("~/repo/cli".to_string()),
            workspace_root: Some("~/repo".to_string()),
            branch: Some("feature/x".to_string()),
            kind: WorktreeKind::LinkedWorktree,
        });
        let mut session = SessionModel::default();
        session.apply(SessionIntent::SetCurrentSession {
            id: "s-1".to_string(),
        });

        let vm = StatusViewAssembler::assemble_runtime_view(&runtime, Some(&session));

        assert_eq!(vm.model.as_deref(), Some("glm-5.1"));
        assert_eq!(vm.session_id.as_deref(), Some("s-1"));
        assert_eq!(vm.tps, 42.0);
        assert_eq!(vm.context.path_base, "~/repo/cli");
        assert_eq!(vm.context.branch.as_deref(), Some("feature/x"));
        assert_eq!(
            vm.context.kind,
            crate::tui::view_model::StatusWorktreeKind::Worktree
        );
    }

    #[test]
    fn test_assemble_runtime_view_boundary_empty_branch_becomes_none() {
        let mut runtime = RuntimeModel::default();
        runtime.apply(RuntimeIntent::WorkspaceSnapshotReceived {
            path_base: Some("/repo".to_string()),
            workspace_root: Some("/repo".to_string()),
            branch: Some("   ".to_string()),
            kind: WorktreeKind::MainCheckout,
        });

        let vm = StatusViewAssembler::assemble_runtime_view(&runtime, None);

        assert!(vm.context.branch.is_none());
        assert_eq!(
            vm.context.kind,
            crate::tui::view_model::StatusWorktreeKind::Main
        );
    }

    #[test]
    fn test_assemble_runtime_view_error_path_missing_model_and_session() {
        let runtime = RuntimeModel::default();

        let vm = StatusViewAssembler::assemble_runtime_view(&runtime, None);

        assert!(vm.model.is_none());
        assert!(vm.session_id.is_none());
        assert_eq!(vm.tps, 0.0);
        assert!(vm.context.path_base.is_empty());
    }

    #[test]
    fn test_status_assembler_reads_runtime_and_diagnostic() {
        let mut runtime = RuntimeModel {
            model_id: Some("gpt-5.5".to_string()),
            ..Default::default()
        };
        runtime.apply(RuntimeIntent::UpdateWorkspace {
            cwd: "/repo".to_string(),
            worktree: None,
        });

        let mut diagnostic = DiagnosticModel::default();
        diagnostic.apply(DiagnosticIntent::RecordNotice {
            severity: DiagnosticSeverity::Warning,
            message: "orphan event".to_string(),
        });

        let vm = StatusViewAssembler::assemble_from_runtime_session(&runtime, None, &diagnostic);
        assert!(vm.left.iter().any(|segment| segment.text == "gpt-5.5"));
        assert!(vm.right.iter().any(|segment| segment.text == "/repo"));
        assert!(vm
            .center
            .iter()
            .any(|segment| segment.text.contains("warning")));
    }
}
