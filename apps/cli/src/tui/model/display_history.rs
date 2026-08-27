use super::conversation::resumed_history::{
    ResumedHistoryBacking, ResumedHistoryItem, ResumedHistoryStep,
};
use crate::tui::adapter::runtime_view::TuiDisplayHistoryWindow;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DisplayHistoryModel {
    backing: ResumedHistoryBacking,
}

impl DisplayHistoryModel {
    pub(crate) fn clear(&mut self) {
        self.backing = ResumedHistoryBacking::default();
    }

    pub(crate) fn replace(&mut self, backing: ResumedHistoryBacking) {
        self.backing = backing;
    }

    pub(crate) fn step(&self, index: usize) -> Option<&ResumedHistoryStep> {
        self.backing.step(index)
    }

    #[cfg(test)]
    pub(crate) fn loaded_step_count_for_test(&self) -> usize {
        self.backing.loaded_step_count_for_test()
    }

    pub(crate) fn items(&self) -> &[ResumedHistoryItem] {
        self.backing.items()
    }

    pub(crate) fn item(&self, id: &str) -> Option<&ResumedHistoryItem> {
        self.backing.item(id)
    }

    pub(crate) fn window_request(
        &self,
        item_ids: &[String],
    ) -> Option<sdk::DisplayHistoryWindowRequest> {
        self.backing.history_window_request(item_ids)
    }

    pub(crate) fn apply_window(&mut self, window: TuiDisplayHistoryWindow) -> bool {
        self.backing.apply_window(window)
    }
}
