use super::super::OutputViewAssembler;
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

    let (view_model, metrics) = capture(|| {
        OutputViewAssembler::assemble_from_conversation(
            &conversation,
            conversation.revision(),
            None,
        )
    });

    assert_eq!(view_model.roots.len(), 7);
    assert_eq!(metrics.assemble_calls, 1);
    assert_eq!(metrics.assemble_source_items, 7);
    assert_eq!(metrics.assemble_output_roots, 7);
}
