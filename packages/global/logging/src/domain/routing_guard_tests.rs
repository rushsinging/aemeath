use super::*;

fn check_layer(dir_name: &str, target_prefix: &str) {
    let root = workspace_root();
    let dir = root.join(dir_name);
    if !dir.exists() {
        return;
    }
    for file in rust_files_under(&dir) {
        if file
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains("test"))
        {
            continue;
        }
        let raw_source = fs::read_to_string(&file).expect("read rust source");
        let source = production_source(&raw_source);

        // 1. 检查裸 log::xxx! 调用（无 target）
        let bare_violations = has_bare_log_calls(&source);
        assert!(
            bare_violations.is_empty(),
            "{} production code must not use bare log::xxx! — use target: \"{}*\" instead.\nViolations:\n{}",
            file.display(),
            target_prefix,
            bare_violations.join("\n")
        );

        // 2. 检查 target 字符串字面量是否合规
        let target_violations = validate_target_values(&source);
        assert!(
            target_violations.is_empty(),
            "{} production code uses invalid log target string literal.\nAllowed: {:#?}\nViolations:\n{}",
            file.display(),
            TargetCatalog::specs(),
            target_violations.join("\n")
        );
    }
}

#[test]
fn tui_layer_must_not_use_bare_log_macros() {
    check_layer("apps/cli/src/tui", "cli::");
}

#[test]
fn chat_layer_must_not_use_bare_log_macros() {
    check_layer("apps/cli/src/chat", "cli::");
}

#[test]
fn hook_layer_must_not_use_bare_log_macros() {
    check_layer("agent/features/hook/src", "hook::");
}

#[test]
fn runtime_layer_must_not_use_bare_log_macros() {
    check_layer("agent/features/runtime/src", "runtime::");
}

#[test]
fn provider_layer_must_not_use_bare_log_macros() {
    check_layer("agent/features/provider/src", "provider::");
}

#[test]
fn tools_layer_must_not_use_bare_log_macros() {
    check_layer("agent/features/tools/src", "tools::");
}

#[test]
fn context_layer_must_not_use_bare_log_macros() {
    check_layer("agent/features/context/src", "context::");
}

#[test]
fn storage_layer_must_not_use_bare_log_macros() {
    check_layer("agent/features/storage/src", "storage::");
}

#[test]
fn update_layer_must_use_catalog_targets() {
    check_layer("agent/features/update/src", "aemeath:agent:update");
}

#[test]
fn workflow_layer_must_use_catalog_targets() {
    check_layer("agent/features/workflow/src", "aemeath:agent:workflow");
}

#[test]
fn catalog_targets_are_valid() {
    for TargetSpec { target, .. } in TargetCatalog::specs() {
        let target = target.as_str();
        assert!(target.starts_with("aemeath:"));
        assert!(target.split(':').count() <= 3);
    }
}
