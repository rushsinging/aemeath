#![allow(dead_code)]

pub mod animation;
pub mod cache;
pub mod input;
pub mod layout;
pub mod output;

pub use animation::AnimationViewState;
pub use cache::ViewRenderCache;
pub use input::InputViewState;
pub use layout::LayoutViewState;
pub use output::OutputViewState;

#[derive(Debug, Default)]
pub struct AppViewState {
    pub output: OutputViewState,
    pub input: InputViewState,
    pub layout: LayoutViewState,
    pub animation: AnimationViewState,
    pub cache: ViewRenderCache,
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
