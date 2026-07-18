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
        "agent/features/runtime/src",
        &[("lib.rs", "log::info!(target: crate::LOG_TARGET, \"x\");\n")],
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
        "agent/features/runtime/src",
        &[
            (
                "lib.rs",
                "pub(crate) const LOG_TARGET: &str = \"aemeath:agent:runtime\";\n",
            ),
            (
                "extra.rs",
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
fn workspace_scan_flags_empty_owner_that_defines_a_constant() {
    // Owners with no registered target (config/memory/task) must not define a
    // LOG_TARGET constant at all.
    let root = scratch_workspace(
        "agent/features/config/src",
        &[(
            "lib.rs",
            "pub(crate) const LOG_TARGET: &str = \"aemeath:agent:config\";\n",
        )],
    );
    let violations = inspect_workspace(&root).expect("scan scratch");
    assert!(violations
        .iter()
        .any(|v| v.kind == ViolationKind::EmptyOwnerConstant));
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
