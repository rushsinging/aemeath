use super::ids::ChatRunId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssistantStream {
    pub run_id: ChatRunId,
    pub kind: AssistantStreamKind,
    pub buffer: String,
    pub synthetic_think_open: bool,
}

impl AssistantStream {
    pub fn new(run_id: ChatRunId, kind: AssistantStreamKind) -> Self {
        Self {
            run_id,
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
#[path = "stream_tests.rs"]
mod tests;

