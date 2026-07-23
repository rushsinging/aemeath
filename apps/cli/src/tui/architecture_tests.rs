use std::fs;
use std::path::Path;

fn rust_files_under(path: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).expect("read test directory") {
            let entry = entry.expect("read directory entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files
}

fn production_source(source: &str) -> String {
    let mut output = String::new();
    let mut skip_test_module = false;
    let mut brace_depth = 0usize;

    for line in source.lines() {
        if line.trim() == "#[cfg(test)]" {
            skip_test_module = true;
            continue;
        }
        if skip_test_module {
            let opens = line.matches('{').count();
            let closes = line.matches('}').count();
            if opens > 0 || brace_depth > 0 {
                brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);
                if brace_depth == 0 {
                    skip_test_module = false;
                }
            }
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }

    output
}

#[test]
fn test_phase_one_root_reducer_intent_entrypoint_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let intent = fs::read_to_string(root.join("update/intent.rs")).expect("read AgentIntent");
    let reducer =
        fs::read_to_string(root.join("update/root_reducer.rs")).expect("read root reducer");

    for context in ["Conversation", "Input", "Diagnostic", "Session"] {
        assert!(
            intent.contains(&format!("{context}(")),
            "AgentIntent must route the {context} context"
        );
    }
    assert!(
        reducer.contains("fn reduce_intent(model: &mut TuiModel, intent: AgentIntent)"),
        "root reducer must expose the phase-one AgentIntent entrypoint"
    );
}

#[test]
fn test_phase_two_bypass_mutation_paths_are_retired() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let app_update = root.join("app/update");

    for file in rust_files_under(&app_update) {
        if file
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("test"))
        {
            continue;
        }
        let source = production_source(&fs::read_to_string(&file).expect("read rust source"));
        for forbidden in [
            "spinner.chat_active =",
            "spinner.phase =",
            "spinner.running_tool_count =",
            "sync_queued_from_runtime(",
            "clear_compact_runtime(",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} must not retain phase-two bypass mutation {forbidden}",
                file.display()
            );
        }
    }

    let runtime_state = fs::read_to_string(root.join("model/conversation/runtime_state.rs"))
        .expect("read runtime state");
    assert!(
        !runtime_state.contains("spinner_mut("),
        "RuntimeState::spinner_mut must be retired"
    );
}

#[test]
fn test_phase_three_batch_a_has_no_direct_context_apply() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/app/update");

    for name in ["key.rs", "notice.rs", "ask_user_key.rs", "ui_event.rs"] {
        let source =
            production_source(&fs::read_to_string(root.join(name)).expect("read update source"));
        let compact: String = source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        for context in ["conversation", "input", "diagnostic", "session"] {
            assert!(
                !compact.contains(&format!("model.{context}.apply(")),
                "{name} must use App::apply_agent_intent instead of direct {context} apply"
            );
        }
    }
}

#[test]
fn test_phase_three_batch_b_has_no_direct_context_apply() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/app");
    let files = [
        root.join("runtime.rs"),
        root.join("util.rs"),
        root.join("slash/suggestions.rs"),
    ];

    for file in files {
        let source = production_source(&fs::read_to_string(&file).expect("read batch B source"));
        let compact: String = source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        for context in ["conversation", "input", "diagnostic", "session"] {
            assert!(
                !compact.contains(&format!("model.{context}.apply(")),
                "{} must use App::apply_agent_intent instead of direct {context} apply",
                file.display()
            );
        }
    }
}

#[test]
fn test_phase_three_batch_c_has_no_direct_context_apply() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/effect/session/resume.rs");
    let source = production_source(&fs::read_to_string(&file).expect("read resume source"));
    let compact: String = source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    for context in ["conversation", "input", "diagnostic", "session"] {
        assert!(
            !compact.contains(&format!("model.{context}.apply(")),
            "resume must use App::apply_agent_intent instead of direct {context} apply"
        );
    }
}

#[test]
fn test_phase_three_production_context_apply_is_exclusive_to_root_reducer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let root_reducer = root.join("update/root_reducer.rs");

    for file in rust_files_under(&root) {
        if file == root_reducer
            || file == root.join("app.rs")
            || file
                .components()
                .any(|component| component.as_os_str() == "scenario_tests")
            || file
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("test"))
        {
            continue;
        }

        let source = fs::read_to_string(&file).expect("read TUI source");
        let source = if file == root.join("app/update.rs") {
            source
                .split("#[cfg(test)]")
                .next()
                .unwrap_or_default()
                .to_string()
        } else {
            production_source(&source)
        };
        let compact: String = source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        for context in ["conversation", "input", "diagnostic", "session"] {
            assert!(
                !compact.contains(&format!("model.{context}.apply(")),
                "{} must not directly apply the {context} context outside root_reducer",
                file.display()
            );
        }
    }
}

#[test]
fn test_phase_four_acl_only_produces_intents() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let mapper = fs::read_to_string(root.join("adapter/agent_event.rs")).expect("read ACL mapper");
    let reducer =
        fs::read_to_string(root.join("update/root_reducer.rs")).expect("read root reducer");

    assert!(
        !mapper.contains("effect::Effect") && !mapper.contains("effects: Vec<Effect>"),
        "AgentEventMapping must not import or carry Effect values"
    );
    assert!(
        !reducer.contains("mapping.effects"),
        "root reducer must not pass through mapper Effects"
    );
}

#[test]
fn runtime_presentation_owns_runtime_display_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let runtime = fs::read_to_string(root.join("model/conversation/runtime_state.rs"))
        .expect("read conversation runtime");
    let intent = fs::read_to_string(root.join("model/conversation/intent.rs"))
        .expect("read conversation intent");
    let usage = fs::read_to_string(root.join("model/conversation/usage.rs"))
        .expect("read conversation usage");
    let status =
        fs::read_to_string(root.join("view_assembler/status.rs")).expect("read status assembler");

    for forbidden in ["provider:", "model_id:", "thinking:"] {
        assert!(
            !runtime.contains(forbidden),
            "Conversation RuntimeState must not own RuntimePresentation field {forbidden}"
        );
    }
    assert!(
        !usage.contains("context_size:"),
        "Conversation usage must not own RuntimePresentation context_size"
    );
    assert!(
        !intent.contains("RuntimePresentationIntent"),
        "ConversationIntent must not retain RuntimePresentation intent"
    );
    assert!(
        status.contains("presentation: &RuntimePresentation"),
        "Status assembler must receive RuntimePresentation explicitly"
    );
}

#[test]
fn test_phase_four_workspace_provider_owns_workspace_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let runtime = fs::read_to_string(root.join("model/conversation/runtime_state.rs"))
        .expect("read conversation runtime");
    let intent = fs::read_to_string(root.join("model/conversation/intent.rs"))
        .expect("read conversation intent");
    let change = fs::read_to_string(root.join("model/conversation/change.rs"))
        .expect("read conversation change");
    let status =
        fs::read_to_string(root.join("view_assembler/status.rs")).expect("read status assembler");
    let update = fs::read_to_string(root.join("app/update.rs")).expect("read app update");

    assert!(
        !runtime.contains("workspace:"),
        "Conversation RuntimeState must not own WorkspaceProvider fields"
    );
    for forbidden in [
        "UpdateWorkspace",
        "WorkspaceSnapshotReceived",
        "WorkspaceChanged",
        "WorkspaceSnapshotChanged",
    ] {
        assert!(
            !intent.contains(forbidden) && !change.contains(forbidden),
            "Conversation intent/change must not retain WorkspaceProvider symbol {forbidden}"
        );
    }
    assert!(
        status.contains("workspace: &WorkspaceProvider"),
        "Status assembler must receive WorkspaceProvider explicitly"
    );
    assert!(
        update.contains("workspace_provider\n            .workspace_root()"),
        "Output cache must derive workspace_root from WorkspaceProvider"
    );
}

#[test]
fn test_phase_four_workspace_metadata_git_is_executor_only() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let app = fs::read_to_string(root.join("app.rs")).expect("read app");
    let mapping = fs::read_to_string(root.join("effect/session/processing/event_mapping.rs"))
        .expect("read event mapping");
    let event = fs::read_to_string(root.join("app/event.rs")).expect("read app event");
    let provider = fs::read_to_string(root.join("model/workspace_provider.rs"))
        .expect("read workspace provider");
    let executor = fs::read_to_string(root.join("effect/executor.rs")).expect("read executor");

    for source in [&app, &mapping] {
        assert!(
            !source.contains("Command::new(\"git\")")
                && !source.contains("git_branch_for")
                && !source.contains("worktree_kind_for"),
            "SDK event and app paths must not synchronously resolve Git metadata"
        );
    }
    let snapshot_event = event
        .split("pub struct StatusContextUpdate")
        .nth(1)
        .and_then(|source| source.split("pub struct WorkspaceMetadataResolved").next())
        .expect("extract workspace snapshot event");
    assert!(
        !snapshot_event.contains("pub branch:") && !snapshot_event.contains("pub kind:"),
        "workspace snapshot event must not carry derived Git metadata"
    );
    assert!(
        !provider.contains("ApplySnapshot {\n        path_base: Option<String>,\n        workspace_root: Option<String>,\n        branch:"),
        "Workspace snapshot intent must not carry branch metadata"
    );
    assert!(
        executor.contains("Command::new(\"git\")") && executor.contains("spawn_blocking"),
        "executor must be the only asynchronous Git metadata resolver"
    );
}

#[test]
fn test_phase_five_agent_run_state_is_tui_owned_and_mapper_is_not_wired() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let interaction = fs::read_to_string(root.join("model/conversation/interaction.rs"))
        .expect("read interaction model");
    let intent = fs::read_to_string(root.join("model/conversation/intent.rs"))
        .expect("read conversation intent");
    let change = fs::read_to_string(root.join("model/conversation/change.rs"))
        .expect("read conversation change");
    let mapper = fs::read_to_string(root.join("effect/session/processing/event_mapping.rs"))
        .expect("read processing mapper");

    let run_intents = intent
        .split("pub struct RunStarted")
        .nth(1)
        .and_then(|source| source.split("// ════════════════════════════════════════════════════════════════════\n//  Runtime intent structs").next())
        .expect("extract agent run intent declarations");
    for (name, source) in [
        ("agent run model", interaction.as_str()),
        ("agent run intent", run_intents),
        ("agent run change", change.as_str()),
    ] {
        for forbidden in [
            "sdk::",
            "oneshot::Sender",
            "tokio::sync",
            "AgentClient",
            ".await",
            "spawn",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} must remain TUI-owned and pure: found {forbidden}"
            );
        }
    }
    assert!(
        mapper.contains("sdk::ChatEvent::RunStarted { .. }")
            && mapper.contains("UiEvent::SystemMessage(String::new())"),
        "#943 must wire Runtime lifecycle DTOs; #944 5A must not modify the SDK mapper"
    );
}

#[test]
fn test_phase_four_interaction_model_is_sender_free() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let interaction = fs::read_to_string(root.join("model/conversation/interaction.rs"))
        .expect("read interaction model");
    let interaction_change = fs::read_to_string(root.join("model/conversation/change.rs"))
        .expect("read interaction change");
    let coordinator = fs::read_to_string(root.join("update/coordinator.rs"))
        .expect("read interaction coordinator");
    let executor =
        fs::read_to_string(root.join("effect/executor.rs")).expect("read interaction executor");

    for (name, source) in [
        ("interaction model", interaction.as_str()),
        ("interaction change", interaction_change.as_str()),
    ] {
        for forbidden in [
            "sdk::",
            "oneshot::Sender",
            "tokio::sync",
            "PendingInteraction",
            "InteractionBridge",
            "Registry",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} must not contain Runtime resource {forbidden}"
            );
        }
    }
    for forbidden in ["AgentClient", ".await", "tokio::spawn"] {
        assert!(
            !coordinator.contains(forbidden),
            "Coordinator must remain pure and must not contain {forbidden}"
        );
    }
    assert!(
        executor.contains("reply_interaction") && executor.contains("cancel_interaction"),
        "only executor must convert interaction Effects into AgentClient commands"
    );
}

#[test]
fn test_tui_facade_reexports_only_app_entrypoint() {
    let tui_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui.rs");
    let source = fs::read_to_string(&tui_root).expect("read tui facade");

    assert!(
        source.contains("pub use self::app::App;"),
        "tui facade must publish the App entrypoint"
    );
    let reexports_render = source.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("pub use ") && trimmed.contains("render")
    });
    assert!(
        !reexports_render,
        "tui facade must not re-export render widgets; they are internal implementation details"
    );
}
#[test]
fn test_adapter_and_view_assembler_production_do_not_depend_on_render_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui");
    let checked_dirs = [root.join("adapter"), root.join("view_assembler")];

    for dir in checked_dirs {
        for file in rust_files_under(&dir) {
            if file
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("test"))
            {
                continue;
            }
            let source = production_source(&fs::read_to_string(&file).expect("read rust source"));
            assert!(
                !source.contains("crate::tui::render::"),
                "{} production code must not depend on render modules",
                file.display()
            );
        }
    }
}
