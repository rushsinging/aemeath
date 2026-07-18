use super::*;

#[test]
fn scanner_rejects_bare_literal_unregistered_and_wrong_owner_targets() {
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    let violations = inspect_source(
        r#"log::info!("bare");
           log::warn!(target: "aemeath:agent:runtime", "literal");
           const LOG_TARGET: &str = "aemeath:not-registered";"#,
        &owner,
        "src/lib.rs",
    );
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::BareLogMacro));
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::LiteralMacroTarget));
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::UnregisteredConstant));
}

#[test]
fn scanner_rejects_registered_constant_owned_by_another_crate() {
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    let violations = inspect_source(
        r#"const LOG_TARGET: &str = "aemeath:agent:storage";"#,
        &owner,
        "src/lib.rs",
    );
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::WrongOwnerConstant));
}

#[test]
fn scanner_handles_multiline_macros_and_ignores_comments_and_strings() {
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    let source = r#"
        // log::warn!("comment only");
        const EXAMPLE: &str = "log::error!(target: bad, text only)";
        log::info!(
            target:
                crate::LOG_TARGET,
            "real"
        );
    "#;
    assert!(inspect_source(source, &owner, "src/file.rs").is_empty());
}

#[test]
fn scanner_rejects_log_macro_aliases_in_production() {
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    let source = "use log::info as event;\nevent!(target: crate::LOG_TARGET, \"x\");";
    assert!(inspect_source(source, &owner, "src/file.rs")
        .iter()
        .any(|v| v.kind == ViolationKind::LogMacroAlias));
}

#[test]
fn scanner_rejects_every_log_macro_import_form_in_production() {
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    for source in [
        "use log::info;\ninfo!(target: crate::LOG_TARGET, \"x\");",
        "use log::warn as warning;\nwarning!(target: crate::LOG_TARGET, \"x\");",
        "use log::{debug, error as failure, Level};",
        "use log::{self, trace};",
    ] {
        assert!(
            inspect_source(source, &owner, "src/file.rs")
                .iter()
                .any(|v| v.kind == ViolationKind::LogMacroAlias),
            "macro import remained invisible: {source}"
        );
    }
}

#[test]
fn scanner_allows_non_macro_log_imports() {
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    assert!(inspect_source("use log::{Level, Metadata};", &owner, "src/file.rs").is_empty());
}

#[test]
fn external_cfg_test_module_does_not_hide_following_production_item() {
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    let source = r#"
        #[cfg(test)]
        mod tests;

        fn production() {
            log::info!("must remain visible");
        }
    "#;
    let violations = inspect_source(source, &owner, "src/file.rs");
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::BareLogMacro));
}

#[test]
fn inline_cfg_test_module_is_still_ignored_without_hiding_following_item() {
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    let source = r#"
        #[cfg(test)]
        mod tests { fn test_only() { log::info!("ignored"); } }
        fn production() { log::warn!(target: crate::LOG_TARGET, "visible and valid"); }
    "#;
    assert!(inspect_source(source, &owner, "src/file.rs").is_empty());
}

#[test]
fn provider_special_target_is_only_allowed_at_registered_owner_path() {
    let owner = OwnerRule::new("provider", "aemeath:agent:provider", "crate::LOG_TARGET");
    let source = r#"log::error!(target: LLM_API_ERROR_TARGET, "api");"#;
    assert!(inspect_source(
        source,
        &owner,
        "agent/features/provider/src/adapters/error_log.rs"
    )
    .is_empty());
    assert!(!inspect_source(
        source,
        &owner,
        "agent/features/provider/src/adapters/client.rs"
    )
    .is_empty());
}

#[test]
fn scanner_rejects_wrong_owner_target_expression() {
    // A production macro that references a target constant belonging to another
    // owner (or a bogus name) must be flagged as an owner mismatch even though
    // it is not a string literal.
    let owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    let source = r#"log::info!(target: crate::WRONG_TARGET, "x");"#;
    assert!(
        inspect_source(source, &owner, "agent/features/runtime/src/lib.rs")
            .iter()
            .any(|v| v.kind == ViolationKind::WrongOwnerTarget)
    );
}

#[test]
fn scanner_accepts_cli_dollar_crate_target_inside_cli_scope() {
    // The TUI macros emit `target: $crate::LOG_TARGET`; this is only legal
    // inside apps/cli/src and must be rejected elsewhere.
    let cli_owner = OwnerRule::new("tui", "aemeath:tui", "crate::LOG_TARGET");
    assert!(inspect_source(
        r#"log::info!(target: $crate::LOG_TARGET, "x");"#,
        &cli_owner,
        "apps/cli/src/tui.rs"
    )
    .is_empty());
    let runtime_owner = OwnerRule::new("runtime", "aemeath:agent:runtime", "crate::LOG_TARGET");
    assert!(inspect_source(
        r#"log::info!(target: $crate::LOG_TARGET, "x");"#,
        &runtime_owner,
        "agent/features/runtime/src/lib.rs"
    )
    .iter()
    .any(|v| v.kind == ViolationKind::WrongOwnerTarget));
}

/// Build a throwaway workspace root containing only the supplied owner scope.
fn scratch_workspace(owner_scope: &str, files: &[(&str, &str)]) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("aemeath-guard-{stamp}"));
    for (name, body) in files {
        let path = root.join(owner_scope).join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    root
}

#[test]
fn workspace_scan_flags_missing_owner_constant() {
    // An owner with production log calls but no LOG_TARGET constant violates
    // the single-owner-constant invariant.
    let root = scratch_workspace(
        "agent/features/runtime",
        &[(
            "src/lib.rs",
            "log::info!(target: crate::LOG_TARGET, \"x\");\n",
        )],
    );
    let violations = inspect_workspace(&root).expect("scan scratch");
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::MissingOwnerConstant));
}

#[test]
fn workspace_scan_flags_duplicate_owner_constant() {
    // Two LOG_TARGET definitions inside one owner scope are forbidden.
    let root = scratch_workspace(
        "agent/features/runtime",
        &[
            (
                "src/lib.rs",
                "pub(crate) const LOG_TARGET: &str = \"aemeath:agent:runtime\";\n",
            ),
            (
                "src/extra.rs",
                "pub(crate) const LOG_TARGET: &str = \"aemeath:agent:runtime\";\n",
            ),
        ],
    );
    let violations = inspect_workspace(&root).expect("scan scratch");
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::DuplicateOwnerConstant));
}

#[test]
fn workspace_scan_accepts_anonymous_reference_to_owner_constant() {
    let root = scratch_workspace(
        "agent/features/config",
        &[(
            "src/lib.rs",
            concat!(
                "pub(crate) const LOG_TARGET: &str = \"aemeath:agent:config\";\n",
                "const _: &str = LOG_TARGET;\n",
            ),
        )],
    );
    let violations = inspect_workspace(&root).expect("scan scratch");
    assert!(!violations.iter().any(|violation| {
        violation.path.starts_with("agent/features/config")
            && matches!(
                violation.kind,
                ViolationKind::MissingOwnerConstant | ViolationKind::DuplicateOwnerConstant
            )
    }));
}

#[test]
fn workspace_scan_requires_a_constant_even_without_log_calls() {
    let root = scratch_workspace(
        "agent/features/config",
        &[
            ("Cargo.toml", "[package]\nname = \"config\"\n"),
            ("src/lib.rs", "pub fn load() {}\n"),
        ],
    );
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"agent/features/config\"]\n",
    )
    .unwrap();
    let violations = inspect_workspace(&root).expect("scan scratch");
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::MissingOwnerConstant));
}

#[test]
fn workspace_manifest_and_guard_cover_the_same_members() {
    let root = workspace_root();
    let manifest_members = workspace_members(&root).expect("parse workspace members");
    let guarded_members = OWNERS
        .iter()
        .map(|(member, _)| (*member).to_owned())
        .collect::<Vec<_>>();
    assert_eq!(manifest_members, guarded_members);
    assert_eq!(manifest_members.len(), 21);
}

#[test]
fn every_workspace_member_root_has_exactly_one_crate_private_target() {
    let root = workspace_root();
    for (member, owner) in OWNERS {
        let crate_root = crate_root(&root.join(member)).expect("workspace member crate root");
        let source = std::fs::read_to_string(&crate_root).expect("read crate root");
        let declarations = owner_constant_declarations(&source);
        assert_eq!(
            declarations,
            vec![owner.target.to_owned()],
            "{} must contain exactly one crate-private LOG_TARGET in {}",
            owner.name,
            crate_root.display()
        );
    }
}

#[test]
fn every_workspace_member_has_an_independent_catalog_route() {
    use super::super::routing::{DiagnosticSinkId as Sink, ModuleOwner as Owner};

    let expected = [
        ("apps/cli", "aemeath:tui", Owner::Tui, Sink::Tui, "tui.log"),
        (
            "agent/composition",
            "aemeath:composition",
            Owner::Composition,
            Sink::Composition,
            "composition.log",
        ),
        (
            "agent/features/audit",
            "aemeath:agent:audit",
            Owner::Audit,
            Sink::Audit,
            "agent-audit.log",
        ),
        (
            "agent/features/config",
            "aemeath:agent:config",
            Owner::Config,
            Sink::Config,
            "agent-config.log",
        ),
        (
            "agent/features/hook",
            "aemeath:agent:hook",
            Owner::Hook,
            Sink::Hook,
            "agent-hook.log",
        ),
        (
            "agent/features/memory",
            "aemeath:agent:memory",
            Owner::Memory,
            Sink::Memory,
            "agent-memory.log",
        ),
        (
            "agent/features/policy",
            "aemeath:agent:policy",
            Owner::Policy,
            Sink::Policy,
            "agent-policy.log",
        ),
        (
            "agent/features/context",
            "aemeath:context",
            Owner::Context,
            Sink::Context,
            "context.log",
        ),
        (
            "agent/features/project",
            "aemeath:agent:project",
            Owner::Project,
            Sink::Project,
            "agent-project.log",
        ),
        (
            "agent/features/provider",
            "aemeath:agent:provider",
            Owner::Provider,
            Sink::Provider,
            "agent-provider.log",
        ),
        (
            "agent/features/runtime",
            "aemeath:agent:runtime",
            Owner::Runtime,
            Sink::Runtime,
            "agent-runtime.log",
        ),
        (
            "agent/features/storage",
            "aemeath:agent:storage",
            Owner::Storage,
            Sink::Storage,
            "agent-storage.log",
        ),
        (
            "agent/features/task",
            "aemeath:agent:task",
            Owner::Task,
            Sink::Task,
            "agent-task.log",
        ),
        (
            "agent/features/tools",
            "aemeath:agent:tools",
            Owner::Tools,
            Sink::Tools,
            "agent-tools.log",
        ),
        (
            "agent/features/update",
            "aemeath:agent:update",
            Owner::Update,
            Sink::Update,
            "agent-update.log",
        ),
        (
            "agent/features/workflow",
            "aemeath:agent:workflow",
            Owner::Workflow,
            Sink::Workflow,
            "agent-workflow.log",
        ),
        (
            "agent/shared",
            "aemeath:shared",
            Owner::Shared,
            Sink::Shared,
            "shared.log",
        ),
        (
            "packages/sdk",
            "aemeath:sdk",
            Owner::Sdk,
            Sink::Sdk,
            "sdk.log",
        ),
        (
            "packages/global/logging",
            "aemeath:logging",
            Owner::Logging,
            Sink::Logging,
            "logging.log",
        ),
        (
            "packages/global/utils",
            "aemeath:utils",
            Owner::Utils,
            Sink::Utils,
            "utils.log",
        ),
        (
            "tools/xtask",
            "aemeath:xtask",
            Owner::Xtask,
            Sink::Xtask,
            "xtask.log",
        ),
    ];
    assert_eq!(expected.len(), OWNERS.len());
    for (member, target, owner, sink, file) in expected {
        let rule = OWNERS.iter().find(|(path, _)| *path == member).unwrap().1;
        assert_eq!(rule.target, target);
        let spec = TargetCatalog::exact(target).expect("member target registered");
        assert_eq!((spec.owner, spec.sink, spec.file_name), (owner, sink, file));
    }
}

#[test]
fn current_workspace_obeys_owner_aware_log_target_policy() {
    let violations = inspect_workspace(&workspace_root()).expect("scan workspace");
    assert!(
        violations.is_empty(),
        "owner-aware log target violations:\n{}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn catalog_targets_are_valid_and_unique() {
    let mut seen = std::collections::HashSet::new();
    for TargetSpec { target, .. } in TargetCatalog::specs() {
        let target = target.as_str();
        assert!(target.starts_with("aemeath:"));
        assert!(target.split(':').count() <= 3);
        assert!(seen.insert(target));
    }
}
