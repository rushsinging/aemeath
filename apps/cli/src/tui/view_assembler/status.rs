use crate::tui::view_model::{SemanticStyle, StatusLineViewModel, StatusSegment};

pub struct StatusViewAssembler;

impl StatusViewAssembler {
    pub fn assemble_basic(model_id: Option<&str>, cwd: Option<&str>) -> StatusLineViewModel {
        let mut vm = StatusLineViewModel::default();
        if let Some(model_id) = model_id {
            vm.left.push(StatusSegment {
                key: "model".to_string(),
                text: model_id.to_string(),
                style: SemanticStyle::Accent,
                priority: 10,
            });
        }
        if let Some(cwd) = cwd {
            vm.right.push(StatusSegment {
                key: "cwd".to_string(),
                text: cwd.to_string(),
                style: SemanticStyle::Muted,
                priority: 20,
            });
        }
        vm
    }
}
