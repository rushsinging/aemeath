use super::*;

#[test]
fn config_update_round_trips_as_typed_command() {
    let update = ConfigUpdateData::SetPermissionMode {
        mode: PermissionModeView::AllowAll,
    };
    let json = serde_json::to_string(&update).unwrap();
    assert_eq!(
        serde_json::from_str::<ConfigUpdateData>(&json).unwrap(),
        update
    );
}
