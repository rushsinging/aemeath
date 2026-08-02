use std::path::Path;

#[test]
fn production_resume_paths_do_not_restore_full_history_materializers() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let forbidden_by_file = [
        (
            "agent/features/context/src/domain/session/restore.rs",
            &["materialize_messages"][..],
        ),
        (
            "apps/cli/src/tui/view_assembler/resumed_history.rs",
            &[
                "materialize_for_conversation",
                "ConversationModel::default()",
                "collect::<Vec<&Message>>()",
                "collect::<Vec<_>>()",
            ][..],
        ),
    ];

    for (relative_path, forbidden_symbols) in forbidden_by_file {
        let path = workspace_root.join(relative_path);
        let source = std::fs::read_to_string(&path).expect("read production resume source");
        for symbol in forbidden_symbols {
            assert!(
                !source.contains(symbol),
                "{} must not contain retired full-history materializer {symbol}",
                path.display()
            );
        }
    }
}
