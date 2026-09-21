use crate::tui::model::diagnostic::model::DiagnosticModel;
use crate::tui::view_model::{DialogActionViewModel, DialogKind, DialogViewModel, StatusSeverity};

pub struct DialogViewAssembler;

impl DialogViewAssembler {
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
mod tests {}
