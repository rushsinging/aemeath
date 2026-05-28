use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::model::RuntimeModel;
use crate::tui::view_model::{
    SemanticStyle, StatusLineViewModel, StatusSegment, StatusSeverity,
};

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
        let mut vm = StatusLineViewModel::default();
        if let Some(model_id) = runtime.model_id.as_deref() {
            vm.left.push(StatusSegment {
                key: "model".to_string(),
                text: model_id.to_string(),
                style: SemanticStyle::Accent,
                priority: 10,
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
        let mut runtime = RuntimeModel::default();
        runtime.model_id = Some("gpt-5.5".to_string());
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
