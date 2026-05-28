use crate::tui::view_model::DialogViewModel;

pub struct DialogViewAssembler;

impl DialogViewAssembler {
    pub fn none() -> Option<DialogViewModel> {
        None
    }
}
