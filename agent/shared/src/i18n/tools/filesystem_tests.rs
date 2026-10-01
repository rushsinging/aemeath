use super::*;

#[test]
fn filesystem_bilingual_and_fallback() {
    assert!(bash("zh").contains("执行 bash 命令"));
    // 跨调用状态前提（schema 未承载，必须留在 description）。
    assert!(bash("zh").contains("工作目录在多次调用间保持"));
    assert!(bash("zh").contains("shell 状态不保持"));
    assert!(bash("zh").contains("&&"));
    assert!(bash("en").contains("Executes a bash command"));
    assert!(bash("en").contains("working directory persists"));
    assert!(bash("en").contains("shell state does not"));
    // `goal` 必填与 timeout 默认/上限属参数契约，迁至 BashInput 字段 doc
    //（tools crate 由 schema required + 字段 description 锁定）。
    assert_eq!(bash("fr"), bash("en"));
    assert!(grep("zh").contains("搜索文件内容"));
    assert!(file_read("zh").contains("读取文件"));
    assert!(file_edit("zh").contains("精确字符串替换"));
    assert!(file_write("zh").contains("写入文件"));
    assert!(file_write("zh").contains("必须先调用 Read"));
    assert!(file_write("en").contains("Read must be called first"));
    assert!(glob("zh").contains("文件模式匹配"));
}

#[test]
fn trimmed_descriptions_fit_the_200_char_budget() {
    for (zh, en) in [
        (bash("zh"), bash("en")),
        (file_write("zh"), file_write("en")),
    ] {
        assert!(
            zh.chars().count() <= 200,
            "zh too long ({}): {zh}",
            zh.chars().count()
        );
        assert!(
            en.chars().count() <= 200,
            "en too long ({}): {en}",
            en.chars().count()
        );
    }
}
