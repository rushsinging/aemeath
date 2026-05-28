#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InputMode {
    #[default]
    Normal,
    PromptAnswer,
    Completion,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_mode_default_is_normal() {
        assert_eq!(InputMode::default(), InputMode::Normal);
    }

    #[test]
    fn test_input_mode_prompt_answer_is_distinct() {
        assert_ne!(InputMode::PromptAnswer, InputMode::Normal);
    }

    #[test]
    fn test_input_mode_completion_is_distinct() {
        assert_ne!(InputMode::Completion, InputMode::Normal);
    }
}
