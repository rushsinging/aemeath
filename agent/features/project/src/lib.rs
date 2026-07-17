/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:project";

mod adapters;
mod domain;

pub use adapters::wiring::{wire_production_workspace, WorkspaceViews, WorkspaceWiring};
pub use domain::types::{
    WorkspaceControl, WorkspaceError, WorkspaceFrame, WorkspacePersist, WorkspaceRead,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_wiring_exposes_three_views_from_one_backing() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let wiring = wire_production_workspace(cwd.clone());

        let read = wiring.read();
        let control = wiring.control();
        let persist = wiring.persist();

        assert_eq!(read.current_path_base(), cwd);
        control.set_path_base(cwd.clone()).unwrap();
        assert_eq!(read.current_path_base(), cwd);
        assert_eq!(persist.snapshot().path_base, cwd.display().to_string());
    }

    #[test]
    fn derived_wiring_has_isolated_state() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let parent = wire_production_workspace(cwd.clone());
        let child = parent.derive_isolated();
        let child_path = cwd.join("child-only");

        child.control().set_path_base(child_path.clone()).unwrap();

        assert_eq!(child.read().current_path_base(), child_path);
        assert_eq!(parent.read().current_path_base(), cwd);
    }
}
