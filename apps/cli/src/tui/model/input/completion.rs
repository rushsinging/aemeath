#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputCompletion {
    pub visible: bool,
    pub selected_index: Option<usize>,
    pub query: String,
}
