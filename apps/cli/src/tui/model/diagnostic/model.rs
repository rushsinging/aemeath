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
#[path = "model_tests.rs"]
mod tests;
