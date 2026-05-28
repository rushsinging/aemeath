use super::ids::ChatTurnId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssistantStream {
    pub turn_id: ChatTurnId,
    pub kind: AssistantStreamKind,
    pub buffer: String,
    pub synthetic_think_open: bool,
}

impl AssistantStream {
    pub fn new(turn_id: ChatTurnId, kind: AssistantStreamKind) -> Self {
        Self {
            turn_id,
            kind,
            buffer: String::new(),
            synthetic_think_open: false,
        }
    }

    pub fn append(&mut self, text: &str) {
        self.buffer.push_str(text);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssistantStreamKind {
    Text,
    Thinking,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::ChatTurnId;

    #[test]
    fn test_assistant_stream_starts_empty() {
        let stream = AssistantStream::new(ChatTurnId::new("turn-1"), AssistantStreamKind::Text);
        assert_eq!(stream.buffer, "");
    }

    #[test]
    fn test_assistant_stream_appends_text() {
        let mut stream = AssistantStream::new(ChatTurnId::new("turn-1"), AssistantStreamKind::Text);
        stream.append("hello");
        stream.append(" world");
        assert_eq!(stream.buffer, "hello world");
    }

    #[test]
    fn test_assistant_stream_tracks_kind() {
        let stream = AssistantStream::new(ChatTurnId::new("turn-1"), AssistantStreamKind::Thinking);
        assert_eq!(stream.kind, AssistantStreamKind::Thinking);
    }
}
