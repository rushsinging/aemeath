#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AnimationViewState {
    pub spinner_frame: u64,
    pub cursor_blink_frame: u64,
    pub version: u64,
}
