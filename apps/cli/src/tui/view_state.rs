#![allow(dead_code)]

pub mod animation;
pub mod input;
pub mod input_selection;
pub mod layout;
pub mod output;
pub mod spinner_anim;
pub mod status;

pub use animation::AnimationViewState;
pub use input::InputViewState;
pub use input_selection::InputSelectionViewState;
pub use layout::LayoutViewState;
pub use output::OutputViewState;
pub use spinner_anim::SpinnerAnim;
pub use status::StatusSelectionViewState;

#[derive(Debug, Default)]
pub struct AppViewState {
    pub output: OutputViewState,
    pub input: InputViewState,
    pub layout: LayoutViewState,
    pub animation: AnimationViewState,
    pub spinner: SpinnerAnim,
    /// Status 选区真相（#59 S4）。T2 接入 mouse_handler + 渲染前管线。
    pub status_sel: StatusSelectionViewState,
    /// Input 选区真相（#59 S4）。T4 接入 mouse_handler + 渲染前管线。
    pub input_sel: InputSelectionViewState,
    /// 待刷新的 ViewModel 区域。状态更新只置 dirty，渲染前管线统一派生 widget，
    /// 避免 streaming chunk 每次同步重渲染输出区导致主循环滞后。
    pub dirty: ViewModelDirty,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewModelDirty {
    pub output: bool,
    pub status: bool,
    pub input: bool,
    pub dialog: bool,
}

impl ViewModelDirty {
    pub fn mark_all(&mut self) {
        self.output = true;
        self.status = true;
        self.input = true;
        self.dialog = true;
    }

    pub fn mark_output(&mut self) {
        self.output = true;
    }

    pub fn mark_status(&mut self) {
        self.status = true;
    }

    pub fn mark_input(&mut self) {
        self.input = true;
    }

    pub fn mark_dialog(&mut self) {
        self.dialog = true;
    }

    pub fn clear_output(&mut self) {
        self.output = false;
    }

    pub fn clear_status(&mut self) {
        self.status = false;
    }

    pub fn clear_input(&mut self) {
        self.input = false;
    }

    pub fn clear_dialog(&mut self) {
        self.dialog = false;
    }
}

#[cfg(test)]
mod tests {
    use super::ViewModelDirty;

    #[test]
    fn test_view_model_dirty_tracks_and_clears_output() {
        let mut dirty = ViewModelDirty::default();
        assert!(!dirty.output);
        dirty.mark_output();
        assert!(dirty.output);
        dirty.clear_output();
        assert!(!dirty.output);
    }
}
