use super::*;

#[test]
fn discipline_en_fallback_for_unknown_lang() {
    assert_eq!(
        universal_execution_discipline("fr"),
        UNIVERSAL_EXECUTION_DISCIPLINE_EN
    );
    assert_eq!(
        universal_execution_discipline(""),
        UNIVERSAL_EXECUTION_DISCIPLINE_EN
    );
}

#[test]
fn discipline_zh_selected_for_zh() {
    assert_eq!(
        universal_execution_discipline("zh"),
        UNIVERSAL_EXECUTION_DISCIPLINE_ZH
    );
    assert_ne!(
        UNIVERSAL_EXECUTION_DISCIPLINE_EN,
        UNIVERSAL_EXECUTION_DISCIPLINE_ZH
    );
}
