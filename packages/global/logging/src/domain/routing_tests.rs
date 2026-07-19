use super::*;
use std::collections::HashSet;

#[test]
fn catalog_targets_sinks_and_files_are_unique() {
    let mut targets = HashSet::new();
    let mut sinks = HashSet::new();
    let mut files = HashSet::new();
    let fallback = TargetCatalog::fallback();
    assert!(targets.insert(fallback.target.as_str()));
    assert!(sinks.insert(fallback.sink));
    assert!(files.insert(fallback.file_name));
    for spec in TargetCatalog::specs() {
        assert!(targets.insert(spec.target.as_str()));
        assert!(sinks.insert(spec.sink));
        assert!(files.insert(spec.file_name));
        let _ = spec.owner;
    }
}

#[test]
fn routes_exact_and_child_targets_by_longest_legal_prefix() {
    let runtime = TargetCatalog::route("aemeath:agent:runtime").expect("runtime target");
    assert_eq!(runtime.file_name, "agent-runtime.log");
    let child = TargetCatalog::route("aemeath:agent:runtime:loop").expect("runtime child");
    assert_eq!(child.target, runtime.target);
    assert!(TargetCatalog::route("aemeath:agent:runtimex").is_none());
}

#[test]
fn longest_match_is_independent_of_catalog_order() {
    let parent = target!("aemeath:agent", Runtime, Runtime, "parent.log");
    let child = target!("aemeath:agent:runtime", Runtime, Tools, "child.log");
    for specs in [[parent, child], [child, parent]] {
        assert_eq!(
            route_specs(&specs, "aemeath:agent:runtime:loop")
                .expect("child route")
                .file_name,
            "child.log"
        );
    }
}

#[test]
fn route_boundaries_are_fail_closed() {
    assert!(TargetCatalog::route("").is_none());
    assert!(TargetCatalog::route("aemeath").is_none());
    assert!(TargetCatalog::route("aemeath:agent:runtimex").is_none());
}

#[test]
fn registers_all_current_production_targets() {
    for (target, file) in [
        ("aemeath:agent:update", "agent-update.log"),
        ("aemeath:agent:workflow", "agent-workflow.log"),
        ("aemeath:context", "context.log"),
    ] {
        assert_eq!(
            TargetCatalog::route(target).map(|spec| spec.file_name),
            Some(file)
        );
    }
}

#[test]
fn audit_facts_have_no_diagnostic_route() {
    assert!(TargetCatalog::exact("aemeath:agent:audit").is_none());
    assert!(TargetCatalog::specs()
        .iter()
        .all(|spec| spec.file_name != "agent-audit.log"));
}

#[test]
fn unknown_target_uses_fallback_sink() {
    assert!(TargetCatalog::route("unknown::module").is_none());
    assert_eq!(TargetCatalog::fallback().file_name, "aemeath.log");
}
