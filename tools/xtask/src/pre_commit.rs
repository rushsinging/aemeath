use std::path::Path;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub format_rust: bool,
    pub source_guard: bool,
    pub snapshot_drafts: bool,
}

pub fn plan(paths: &[String]) -> Plan {
    Plan {
        format_rust: paths.iter().any(|path| path.ends_with(".rs")),
        source_guard: paths.iter().any(|path| {
            ["agent/", "apps/", "packages/", "tools/xtask/", ".agents/"]
                .iter()
                .any(|prefix| path.starts_with(prefix))
        }),
        snapshot_drafts: paths.iter().any(|path| {
            path.contains("scenario_tests") || path.contains("snapshots") || path.ends_with(".snap")
        }),
    }
}

pub fn snapshot_drafts(paths: &[String]) -> Vec<String> {
    let mut drafts: Vec<_> = paths
        .iter()
        .filter(|path| path.ends_with(".snap.new") || path.ends_with(".pending-snap"))
        .cloned()
        .collect();
    drafts.sort();
    drafts
}

pub fn needs_issue_tree_check(_path: &Path) -> bool {
    false
}
