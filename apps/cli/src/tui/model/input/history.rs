#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputHistory {
    pub entries: Vec<String>,
    pub selected_index: Option<usize>,
    pub saved_input: String,
}
