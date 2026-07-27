use super::parse_blocks;
use crate::tui::render::output::spacing::MarkdownElement;

#[test]
fn classifier_recognizes_six_blocks_and_source_gaps() {
    let blocks = parse_blocks(
        "# title\n\nparagraph\n\n- item\n  continuation\n\n> quote\n\n| a |\n|---|\n| b |\n\n```rust\n\nfn main() {}\n```",
    );

    assert_eq!(
        blocks.iter().map(|block| block.kind).collect::<Vec<_>>(),
        vec![
            MarkdownElement::Heading,
            MarkdownElement::Paragraph,
            MarkdownElement::List,
            MarkdownElement::Blockquote,
            MarkdownElement::Table,
            MarkdownElement::CodeBlock,
        ]
    );
    assert!(!blocks[0].source_gap_before);
    assert!(blocks.iter().skip(1).all(|block| block.source_gap_before));
    assert_eq!(blocks[5].fence_language.as_deref(), Some("rust"));
    assert!(blocks[5].lines.iter().any(|line| line.is_empty()));
}

#[test]
fn classifier_treats_hash_without_space_and_unclosed_fence_correctly() {
    let blocks = parse_blocks("#tag\n\n```\ncode\n\nmore");

    assert_eq!(blocks[0].kind, MarkdownElement::Paragraph);
    assert_eq!(blocks[1].kind, MarkdownElement::CodeBlock);
    assert_eq!(blocks[1].lines.last(), Some(&"more"));
}
