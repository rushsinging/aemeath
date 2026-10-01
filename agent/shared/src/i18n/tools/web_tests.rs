use super::*;

#[test]
fn web_bilingual_and_fallback() {
    assert!(web_search("zh").contains("搜索网络"));
    assert!(web_search("en").contains("Search the web"));
    assert_eq!(web_search("fr"), web_search("en"));
    assert!(web_fetch("zh").contains("获取 URL 内容"));
    assert!(web_fetch("en").contains("Fetches content"));
}

/// 收敛后的 description 长度预算：en/zh 均不得超过 200 字符。
#[test]
fn trimmed_web_descriptions_fit_the_200_char_budget() {
    for (zh, en) in [
        (web_search("zh"), web_search("en")),
        (web_fetch("zh"), web_fetch("en")),
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
