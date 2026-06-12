#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopiedTextSpan {
    pub placeholder: String,
    pub original: String,
    pub start: usize,
    pub end: usize,
}

impl CopiedTextSpan {
    pub fn new(
        placeholder: impl Into<String>,
        original: impl Into<String>,
        start: usize,
        end: usize,
    ) -> Self {
        Self {
            placeholder: placeholder.into(),
            original: original.into(),
            start,
            end,
        }
    }
}
