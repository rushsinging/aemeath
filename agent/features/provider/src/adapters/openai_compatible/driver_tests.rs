use super::*;

#[test]
fn every_openai_compatible_driver_derives_maximum_from_capability() {
    for (kind, expected_maximum) in [
        (ProviderDriverKind::OpenAI, ReasoningLevel::Max),
        (ProviderDriverKind::Zhipu, ReasoningLevel::Max),
        (ProviderDriverKind::LiteLLM, ReasoningLevel::Max),
        (ProviderDriverKind::Volcengine, ReasoningLevel::Medium),
        (ProviderDriverKind::Minimax, ReasoningLevel::Medium),
        (ProviderDriverKind::Mimo, ReasoningLevel::Medium),
        (ProviderDriverKind::DeepSeek, ReasoningLevel::Max),
        (ProviderDriverKind::Agnes, ReasoningLevel::Medium),
    ] {
        let driver = driver_for_provider_driver(kind);
        let capability = driver.reasoning_capability();
        assert_eq!(capability.maximum(), expected_maximum, "driver={kind:?}");
        assert_eq!(driver.max_reasoning_level(), capability.maximum());
        assert_eq!(capability.resolve(ReasoningLevel::Off), ReasoningLevel::Off);
    }
}

#[test]
fn every_openai_compatible_driver_locks_supported_set_only_openai_includes_minimal() {
    // 锁定每个 driver 的 supported 集合：除 OpenAI 显式声明七档（含 Minimal）
    // 外，其他 driver 的集合必须维持原状，禁止共享枚举新增 Minimal 后被 helper
    // 静默加入 supported。expected_supported 已排序且 Off 在首。
    let expectation: &[(ProviderDriverKind, Vec<ReasoningLevel>)] = &[
        (
            ProviderDriverKind::OpenAI,
            vec![
                ReasoningLevel::Off,
                ReasoningLevel::Minimal,
                ReasoningLevel::Low,
                ReasoningLevel::Medium,
                ReasoningLevel::High,
                ReasoningLevel::Xhigh,
                ReasoningLevel::Max,
            ],
        ),
        (
            ProviderDriverKind::Zhipu,
            vec![
                ReasoningLevel::Off,
                ReasoningLevel::Low,
                ReasoningLevel::Medium,
                ReasoningLevel::High,
                ReasoningLevel::Xhigh,
                ReasoningLevel::Max,
            ],
        ),
        (
            ProviderDriverKind::LiteLLM,
            vec![
                ReasoningLevel::Off,
                ReasoningLevel::Low,
                ReasoningLevel::Medium,
                ReasoningLevel::High,
                ReasoningLevel::Xhigh,
                ReasoningLevel::Max,
            ],
        ),
        (
            ProviderDriverKind::Volcengine,
            vec![
                ReasoningLevel::Off,
                ReasoningLevel::Low,
                ReasoningLevel::Medium,
            ],
        ),
        (
            ProviderDriverKind::Minimax,
            vec![ReasoningLevel::Off, ReasoningLevel::Medium],
        ),
        (
            ProviderDriverKind::Mimo,
            vec![ReasoningLevel::Off, ReasoningLevel::Medium],
        ),
        (
            ProviderDriverKind::DeepSeek,
            vec![
                ReasoningLevel::Off,
                ReasoningLevel::Low,
                ReasoningLevel::Medium,
                ReasoningLevel::High,
                ReasoningLevel::Xhigh,
                ReasoningLevel::Max,
            ],
        ),
        (
            ProviderDriverKind::Agnes,
            vec![ReasoningLevel::Off, ReasoningLevel::Medium],
        ),
    ];

    for (kind, expected_supported) in expectation {
        let driver = driver_for_provider_driver(*kind);
        let capability = driver.reasoning_capability();
        assert_eq!(
            capability.supported(),
            expected_supported.as_slice(),
            "driver={kind:?}"
        );
        // Minimal 仅在 OpenAI 的 supported 集合内；其他 driver 必须显式不含。
        assert_eq!(
            capability.supported().contains(&ReasoningLevel::Minimal),
            *kind == ProviderDriverKind::OpenAI,
            "driver={kind:?} must only include Minimal when it is OpenAI"
        );
    }
}
