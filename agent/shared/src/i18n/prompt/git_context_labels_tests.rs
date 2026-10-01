use super::*;

#[test]
fn git_context_labels_bilingual_and_fallback_en() {
    let zh = git_context_labels("zh");
    let en = git_context_labels("en");
    assert_eq!(zh.branch, "当前分支");
    assert_eq!(en.branch, "Current branch");
    let fr = git_context_labels("fr");
    assert_eq!(fr.branch, en.branch);
    assert_eq!(fr.header, "# Git Context");
}
