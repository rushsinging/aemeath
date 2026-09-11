//! `agent/features/config/src/connect/command.rs` 的契约测试。

use crate::connect::command::expected_stages;
use crate::connect::states::ConnectStage;
use crate::connect::ConnectCommand;

#[test]
fn expected_stages_distinguish_probe_commands() {
    assert_eq!(
        expected_stages(&ConnectCommand::SkipProbe),
        &[ConnectStage::ChooseProbe],
    );
    assert_eq!(
        expected_stages(&ConnectCommand::ContinueAfterProbe),
        &[ConnectStage::Probing],
    );
}

#[test]
fn expected_stages_include_saving_for_retry() {
    assert!(expected_stages(&ConnectCommand::ConfirmSave).contains(&ConnectStage::Saving));
}

#[test]
fn expected_stages_reject_terminal_pair_for_non_save_commands() {
    for variant in [
        ConnectCommand::SelectProvider {
            source: crate::catalog::ProviderSource::new("Anthropic"),
        },
        ConnectCommand::RejectOverwrite,
        ConnectCommand::SetEndpoint {
            base_url: "https://x".into(),
        },
    ] {
        let stages = expected_stages(&variant);
        assert!(!stages.contains(&ConnectStage::Completed));
        assert!(!stages.contains(&ConnectStage::Cancelled));
    }
}
