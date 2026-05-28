#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayoutViewState {
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub version: u64,
}
