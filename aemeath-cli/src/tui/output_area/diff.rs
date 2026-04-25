use super::types::{LineStyle, OutputLine, INDENT};

/// 对比 old_content 与 new_content，生成 diff 输出行。
/// 所有行都标记 id_tag 以关联到原始工具块。
pub fn build_diff_lines(
    old_content: &str,
    new_content: &str,
    id_tag: &Option<String>,
    out: &mut Vec<OutputLine>,
) {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(old_content, new_content);
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => {
                out.push(OutputLine {
                    content: format!("  - {}", change),
                    style: LineStyle::DiffRemove,
                    tool_id: id_tag.clone(),
                });
            }
            ChangeTag::Insert => {
                out.push(OutputLine {
                    content: format!("  + {}", change),
                    style: LineStyle::DiffAdd,
                    tool_id: id_tag.clone(),
                });
            }
            ChangeTag::Equal => {
                out.push(OutputLine {
                    content: format!("{INDENT}{change}"),
                    style: LineStyle::System,
                    tool_id: id_tag.clone(),
                });
            }
        }
    }
}
