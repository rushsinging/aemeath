use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::model::RuntimeModel;
use crate::tui::model::runtime::processing_job::ProcessingStatus;
use crate::tui::model::session::model::SessionModel;
use crate::tui::view_model::{SemanticStyle, StatusLineViewModel, StatusSegment, StatusSeverity};

pub struct StatusViewAssembler;

impl StatusViewAssembler {
    pub fn assemble_basic(model_id: Option<&str>, cwd: Option<&str>) -> StatusLineViewModel {
        let mut vm = StatusLineViewModel::default();
        if let Some(model_id) = model_id {
            vm.left.push(StatusSegment {
                key: "model".to_string(),
                text: model_id.to_string(),
                style: SemanticStyle::Accent,
                priority: 10,
            });
        }
        if let Some(cwd) = cwd {
            vm.right.push(StatusSegment {
                key: "cwd".to_string(),
                text: cwd.to_string(),
                style: SemanticStyle::Muted,
                priority: 20,
            });
        }
        vm
    }

    pub fn assemble_from_models(
        runtime: &RuntimeModel,
        diagnostic: &DiagnosticModel,
    ) -> StatusLineViewModel {
        Self::assemble_from_runtime_session(runtime, None, diagnostic)
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

        let vm = StatusViewAssembler::assemble_from_models(&runtime, &diagnostic);
        assert!(vm.left.iter().any(|segment| segment.text == "gpt-5.5"));
        assert!(vm.right.iter().any(|segment| segment.text == "/repo"));
        assert!(vm
            .center
            .iter()
            .any(|segment| segment.text.contains("warning")));
    }
}
