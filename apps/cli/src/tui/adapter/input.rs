use crate::tui::model::input::submission::InputSubmission;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationAvailability {
    Idle,
    Running,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmissionRoute {
    StartChat { submission: InputSubmission },
    QueueSubmission { submission: InputSubmission },
    AnswerPrompt { text: String },
}

pub fn route_submission(
    submission: InputSubmission,
    conversation: ConversationAvailability,
    prompt_active: bool,
) -> SubmissionRoute {
    if prompt_active {
        return SubmissionRoute::AnswerPrompt {
            text: submission.text,
        };
    }
    match conversation {
        ConversationAvailability::Idle => SubmissionRoute::StartChat { submission },
        ConversationAvailability::Running => SubmissionRoute::QueueSubmission { submission },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::input::submission::InputSubmission;

    #[test]
    fn test_route_submission_starts_chat_when_idle() {
        let route = route_submission(
            InputSubmission {
                text: "hello".to_string(),
                display_text: "hello".to_string(),
                images: Vec::new(),
            },
            ConversationAvailability::Idle,
            false,
        );
        assert!(matches!(route, SubmissionRoute::StartChat { .. }));
    }

    #[test]
    fn test_route_submission_queues_when_running() {
        let route = route_submission(
            InputSubmission {
                text: "hello".to_string(),
                display_text: "hello".to_string(),
                images: Vec::new(),
            },
            ConversationAvailability::Running,
            false,
        );
        assert!(matches!(route, SubmissionRoute::QueueSubmission { .. }));
    }

    #[test]
    fn test_route_submission_answers_prompt_first() {
        let route = route_submission(
            InputSubmission {
                text: "yes".to_string(),
                display_text: "yes".to_string(),
                images: Vec::new(),
            },
            ConversationAvailability::Idle,
            true,
        );
        assert!(matches!(route, SubmissionRoute::AnswerPrompt { .. }));
    }
}
