use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::view_model::{DialogActionViewModel, DialogKind, DialogViewModel, StatusSeverity};

pub struct DialogViewAssembler;

impl DialogViewAssembler {
    pub fn none() -> Option<DialogViewModel> {
        None
    }

    pub fn assemble_from_diagnostic(diagnostic: &DiagnosticModel) -> Option<DialogViewModel> {
        let prompt = diagnostic.active_prompt.as_ref()?;
        Some(DialogViewModel {
            kind: DialogKind::Confirmation,
            title: "确认".to_string(),
            body: prompt.question.clone(),
            actions: vec![DialogActionViewModel {
                id: "submit".to_string(),
                label: "提交".to_string(),
            }],
            default_action: Some("submit".to_string()),
            severity: StatusSeverity::Info,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::model::diagnostic::intent::DiagnosticIntent;
    use crate::tui::model::diagnostic::model::DiagnosticModel;

    use super::DialogViewAssembler;

    #[test]
    fn test_dialog_assembler_maps_active_prompt() {
        let mut diagnostic = DiagnosticModel::default();
        diagnostic.apply(DiagnosticIntent::OpenPrompt {
            id: "prompt-1".to_string(),
            question: "继续?".to_string(),
        });

        let vm = DialogViewAssembler::assemble_from_diagnostic(&diagnostic).expect("dialog");
        assert_eq!(vm.body, "继续?");
        assert_eq!(vm.default_action.as_deref(), Some("submit"));
    }
}
