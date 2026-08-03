use super::{OutputWindowIndex, OutputWindowIndexChange};
use crate::tui::view_model::OutputRenderWindow;

fn ids(index: &OutputWindowIndex) -> Vec<&str> {
    index
        .entries()
        .iter()
        .map(|entry| entry.item_id.as_str())
        .collect()
}

#[test]
fn tail_selection_does_not_scan_every_historical_entry() {
    let mut index = OutputWindowIndex::default();
    index.apply_change(OutputWindowIndexChange::Reset {
        entries: (0..100_000)
            .map(|position| (format!("item-{position}"), 3))
            .collect(),
    });
    index.reset_selection_entry_reads();

    let selection = index.select_window(OutputRenderWindow {
        line_limit: 30,
        tail_offset: 0,
    });

    assert_eq!(selection.item_range, 99_990..100_000);
    assert!(index.selection_entry_reads() <= 40);
}

#[test]
fn changes_update_only_lightweight_window_entries() {
    let mut index = OutputWindowIndex::default();

    index.apply_change(OutputWindowIndexChange::Append {
        item_id: "user-1".to_string(),
        estimated_lines: 3,
    });
    index.apply_change(OutputWindowIndexChange::Append {
        item_id: "assistant-1".to_string(),
        estimated_lines: 5,
    });
    index.record_exact_lines("user-1", 80, 4);
    index.record_exact_lines("assistant-1", 80, 7);

    index.apply_change(OutputWindowIndexChange::Update {
        item_id: "assistant-1".to_string(),
        estimated_lines: 6,
    });

    assert_eq!(ids(&index), vec!["user-1", "assistant-1"]);
    assert_eq!(index.entries()[0].exact_lines_for_width(80), Some(4));
    assert_eq!(index.entries()[1].exact_lines_for_width(80), None);
    assert_eq!(index.entries()[1].estimated_lines, 6);

    index.apply_change(OutputWindowIndexChange::Remove {
        item_id: "user-1".to_string(),
    });

    assert_eq!(ids(&index), vec!["assistant-1"]);
    assert_eq!(index.retained_block_nodes(), 0);
}

#[test]
fn timeline_estimates_include_root_separator_lines() {
    use crate::tui::model::output_timeline::OutputTimelineItem;

    let item = OutputTimelineItem::AssistantText {
        id: "assistant-1".to_string(),
        context: None,
        text: "one line".to_string(),
    };

    assert_eq!(OutputWindowIndex::estimated_lines_for_item(&item), 2);
}

#[test]
fn tail_window_uses_complete_entries_and_honors_tail_offset() {
    let mut index = OutputWindowIndex::default();
    for (item_id, lines) in [("one", 3), ("two", 5), ("three", 7), ("four", 11)] {
        index.apply_change(OutputWindowIndexChange::Append {
            item_id: item_id.to_string(),
            estimated_lines: lines,
        });
    }

    let latest = index.select_window(OutputRenderWindow {
        line_limit: 18,
        tail_offset: 0,
    });
    let older = index.select_window(OutputRenderWindow {
        line_limit: 8,
        tail_offset: 18,
    });

    assert_eq!(latest.item_range, 2..4);
    assert_eq!(latest.source_total_lines, 26);
    assert_eq!(latest.folded_earlier_lines, 8);
    assert_eq!(older.item_range, 0..2);
    assert_eq!(older.source_total_lines, 26);
    assert_eq!(older.folded_earlier_lines, 0);
}

#[test]
fn exact_lines_change_the_next_window_selection() {
    let mut index = OutputWindowIndex::default();
    for item_id in ["one", "two", "three"] {
        index.apply_change(OutputWindowIndexChange::Append {
            item_id: item_id.to_string(),
            estimated_lines: 5,
        });
    }
    let request = OutputRenderWindow {
        line_limit: 10,
        tail_offset: 0,
    };
    assert_eq!(index.select_window(request).item_range, 1..3);

    index.record_exact_lines("three", 80, 9);
    index.record_exact_lines("two", 80, 5);

    let exact = index.select_window_for_width(request, 80);
    assert_eq!(exact.item_range, 2..3);
    assert_eq!(exact.source_total_lines, 19);
    assert_eq!(exact.folded_earlier_lines, 10);
}

#[test]
fn zero_limit_returns_an_empty_tail_window() {
    let mut index = OutputWindowIndex::default();
    index.apply_change(OutputWindowIndexChange::Append {
        item_id: "one".to_string(),
        estimated_lines: 4,
    });

    let selection = index.select_window(OutputRenderWindow {
        line_limit: 0,
        tail_offset: 0,
    });

    assert_eq!(selection.item_range, 1..1);
    assert_eq!(selection.source_total_lines, 4);
    assert_eq!(selection.folded_earlier_lines, 4);
}
#[test]
fn reset_replaces_order_without_retaining_output_bodies() {
    let mut index = OutputWindowIndex::default();
    index.apply_change(OutputWindowIndexChange::Append {
        item_id: "old".to_string(),
        estimated_lines: 100,
    });

    index.apply_change(OutputWindowIndexChange::Reset {
        entries: vec![
            ("system-1".to_string(), 2),
            ("user-1".to_string(), 4),
            ("model-stream-placeholder".to_string(), 1),
        ],
    });

    assert_eq!(
        ids(&index),
        vec!["system-1", "user-1", "model-stream-placeholder"]
    );
    assert!(index
        .entries()
        .iter()
        .all(|entry| entry.exact_lines_for_width(80).is_none()));
    assert_eq!(index.retained_block_nodes(), 0);
}
