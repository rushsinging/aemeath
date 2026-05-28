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
                self.notices.push(DiagnosticNotice {
                    id: id.clone(),
                    severity,
                    message,
                });
                vec![DiagnosticChange::NoticeRecorded { id, severity }]
            }
            DiagnosticIntent::OpenPrompt { id, question } => {
                self.active_prompt = Some(ActivePrompt {
                    id: id.clone(),
                    question,
                });
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
        if self
            .notices
            .iter()
            .any(|notice| notice.severity == DiagnosticSeverity::Error)
        {
            return Some(DiagnosticSeverity::Error);
        }
        if self
            .notices
            .iter()
            .any(|notice| notice.severity == DiagnosticSeverity::Warning)
        {
            return Some(DiagnosticSeverity::Warning);
        }
        if self.notices.is_empty() {
            None
        } else {
            Some(DiagnosticSeverity::Info)
        }
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
        let changes = model.apply(DiagnosticIntent::RecordNotice {
            severity: DiagnosticSeverity::Warning,
            message: "late event".to_string(),
        });
        assert_eq!(model.notices.len(), 1);
        assert!(changes
            .iter()
            .any(|change| matches!(change, DiagnosticChange::NoticeRecorded { .. })));
    }

    #[test]
    fn test_opens_and_answers_prompt() {
        let mut model = DiagnosticModel::default();
        model.apply(DiagnosticIntent::OpenPrompt {
            id: "p1".to_string(),
            question: "继续?".to_string(),
        });
        assert!(model.active_prompt.is_some());
        model.apply(DiagnosticIntent::AnswerPrompt {
            answer: "是".to_string(),
        });
        assert!(model.active_prompt.is_none());
    }

    #[test]
    fn test_highest_severity_prefers_error() {
        let mut model = DiagnosticModel::default();
        model.apply(DiagnosticIntent::RecordNotice {
            severity: DiagnosticSeverity::Info,
            message: "info".to_string(),
        });
        model.apply(DiagnosticIntent::RecordNotice {
            severity: DiagnosticSeverity::Error,
            message: "error".to_string(),
        });
        assert_eq!(model.highest_severity(), Some(DiagnosticSeverity::Error));
    }
}
