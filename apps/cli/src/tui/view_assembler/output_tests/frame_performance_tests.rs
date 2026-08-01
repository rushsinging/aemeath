use super::super::assemble_output_view;
use crate::tui::model::conversation::intent::AppendUserMessage;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::render::performance::capture;

#[test]
fn assemble_records_source_timeline_items_and_output_roots() {
    let mut conversation = ConversationModel::default();
    for index in 0..7 {
        conversation.apply(AppendUserMessage {
            text: format!("message-{index}"),
        });
    }

    let (view_model, metrics) = capture(|| assemble_output_view(&conversation, None));

    assert_eq!(view_model.roots.len(), 7);
    assert_eq!(metrics.retained_view_sync_calls, 0);
    assert_eq!(metrics.assemble_calls, 0);
}
