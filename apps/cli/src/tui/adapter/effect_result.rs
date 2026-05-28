use crate::tui::effect::effect::EffectResult;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::session::intent::SessionIntent;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectResultMapping {
    pub diagnostic: Vec<DiagnosticIntent>,
    pub session: Vec<SessionIntent>,
}

pub fn map_effect_result(result: EffectResult) -> EffectResultMapping {
    match result {
        EffectResult::SessionSaved => EffectResultMapping {
            session: vec![SessionIntent::SaveFinished],
            ..EffectResultMapping::default()
        },
        EffectResult::Failed { message } => EffectResultMapping {
            diagnostic: vec![DiagnosticIntent::RecordNotice {
                severity: DiagnosticSeverity::Error,
                message,
            }],
            ..EffectResultMapping::default()
        },
        _ => EffectResultMapping::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_effect_result_session_saved() {
        let mapping = map_effect_result(EffectResult::SessionSaved);
        assert!(matches!(
            mapping.session.first(),
            Some(SessionIntent::SaveFinished)
        ));
    }

    #[test]
    fn test_map_effect_result_failed_records_notice() {
        let mapping = map_effect_result(EffectResult::Failed {
            message: "失败".to_string(),
        });
        assert_eq!(mapping.diagnostic.len(), 1);
    }

    #[test]
    fn test_map_effect_result_noop_is_empty() {
        let mapping = map_effect_result(EffectResult::Noop);
        assert!(mapping.diagnostic.is_empty() && mapping.session.is_empty());
    }
}
