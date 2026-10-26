//! `Language` 枚举化后的取值域与归一化契约（#1863 W4）。

use super::Language;

/// 已知语言原样映射。
#[test]
fn known_languages_map_to_variants() {
    assert_eq!(Language::from_config("en"), Language::En);
    assert_eq!(Language::from_config("zh"), Language::Zh);
}

/// 大小写与区域变体归一到同一语言。
#[test]
fn regional_variants_normalize() {
    assert_eq!(Language::from_config("ZH"), Language::Zh);
    assert_eq!(Language::from_config("zh-CN"), Language::Zh);
    assert_eq!(Language::from_config("zh-Hans"), Language::Zh);
    assert_eq!(Language::from_config("EN-US"), Language::En);
}

/// 空串与首尾空白按默认语言处理，不产生 warn 分支。
#[test]
fn blank_input_falls_back_to_default() {
    assert_eq!(Language::from_config(""), Language::En);
    assert_eq!(Language::from_config("   "), Language::En);
    assert_eq!(Language::from_config(" en "), Language::En);
}

/// 未知语言降级 `En`，保证新增/拼错的配置不会中断会话启动。
#[test]
fn unknown_language_falls_back_to_en() {
    assert_eq!(Language::from_config("fr"), Language::En);
    assert_eq!(Language::from_config("klingon"), Language::En);
}

/// 线上表示与 guidance 子目录名一致，且往返稳定。
#[test]
fn as_str_round_trips() {
    for language in [Language::En, Language::Zh] {
        assert_eq!(Language::from_config(language.as_str()), language);
    }
    assert_eq!(Language::En.as_str(), "en");
    assert_eq!(Language::Zh.as_str(), "zh");
}

/// 默认实现即英文，与配置默认值 `en` 对齐。
#[test]
fn default_is_en() {
    assert_eq!(Language::default(), Language::En);
}
