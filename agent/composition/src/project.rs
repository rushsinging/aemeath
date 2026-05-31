use std::sync::Arc;

use ::project::api::ProjectGateway;

pub fn wire_project() -> Arc<dyn ProjectGateway> {
    ::project::api::wire_project()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn wire_project_returns_callable_gateway() {
        let gateway = wire_project();
        let cwd = PathBuf::from("/tmp/aemeath-composition");
        let (returned_cwd, working_root, _) = gateway.new_working_paths(cwd.clone());

        assert_eq!(returned_cwd, cwd);
        assert_eq!(gateway.current_path(&working_root), cwd);
    }
}
