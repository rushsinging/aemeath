use super::*;

#[test]
fn every_openai_compatible_driver_derives_maximum_from_capability() {
    for (kind, expected_maximum) in [
        (ProviderDriverKind::OpenAI, ReasoningLevel::Max),
        (ProviderDriverKind::Zhipu, ReasoningLevel::Max),
        (ProviderDriverKind::LiteLLM, ReasoningLevel::Max),
        (ProviderDriverKind::Volcengine, ReasoningLevel::Max),
        (ProviderDriverKind::Minimax, ReasoningLevel::Medium),
        (ProviderDriverKind::Mimo, ReasoningLevel::Medium),
        (ProviderDriverKind::DeepSeek, ReasoningLevel::Max),
        (ProviderDriverKind::Kimi, ReasoningLevel::Max),
        (ProviderDriverKind::Agnes, ReasoningLevel::Medium),
    ] {
        let driver = driver_for_provider_driver(kind);
        let capability = driver.reasoning_capability();
        assert_eq!(
            maximum_supported(&capability),
            expected_maximum,
            "driver={kind:?}"
        );
        assert_eq!(driver.max_reasoning_level(), maximum_supported(&capability));
        assert_eq!(
            resolve_supported(&capability, ReasoningLevel::Off),
            ReasoningLevel::Off
        );
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
                ReasoningLevel::High,
                ReasoningLevel::Xhigh,
                ReasoningLevel::Max,
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
            capability.as_slice(),
            expected_supported.as_slice(),
            "driver={kind:?}"
        );
        // Minimal 仅在 OpenAI 的 supported 集合内；其他 driver 必须显式不含。
        assert_eq!(
            capability.contains(&ReasoningLevel::Minimal),
            *kind == ProviderDriverKind::OpenAI,
            "driver={kind:?} must only include Minimal when it is OpenAI"
        );
    }
}

// ── #2032：服务端档位归并（真相源：智谱官方「深度思考」文档 Coding Plan 语义） ──

#[test]
fn zhipu_server_effective_effort_merges_glm_53_coding_plan_vocabulary() {
    let driver = driver_for_provider_driver(ProviderDriverKind::Zhipu);
    // glm-5.3 系列（Coding Plan）：none/minimal/low → low；medium/high → high；
    // xhigh/max → max；强制思考（off 也归并 low）。
    let expectations = [
        (ReasoningLevel::Off, ReasoningLevel::Low),
        (ReasoningLevel::Minimal, ReasoningLevel::Low),
        (ReasoningLevel::Low, ReasoningLevel::Low),
        (ReasoningLevel::Medium, ReasoningLevel::High),
        (ReasoningLevel::High, ReasoningLevel::High),
        (ReasoningLevel::Xhigh, ReasoningLevel::Max),
        (ReasoningLevel::Max, ReasoningLevel::Max),
    ];
    for (requested, expected) in expectations {
        assert_eq!(
            driver.server_effective_effort("glm-5.3", requested),
            expected,
            "glm-5.3 请求 {requested:?}"
        );
    }
    // FLASH/FLASHX 同语义。
    assert_eq!(
        driver.server_effective_effort("glm-5.3-flash", ReasoningLevel::Medium),
        ReasoningLevel::High
    );
}

#[test]
fn zhipu_server_effective_effort_merges_glm_52_vocabulary() {
    let driver = driver_for_provider_driver(ProviderDriverKind::Zhipu);
    // glm-5.2：none/minimal → 放弃思考（off）；low/medium → high；xhigh → max。
    let expectations = [
        (ReasoningLevel::Off, ReasoningLevel::Off),
        (ReasoningLevel::Minimal, ReasoningLevel::Off),
        (ReasoningLevel::Low, ReasoningLevel::High),
        (ReasoningLevel::Medium, ReasoningLevel::High),
        (ReasoningLevel::High, ReasoningLevel::High),
        (ReasoningLevel::Xhigh, ReasoningLevel::Max),
        (ReasoningLevel::Max, ReasoningLevel::Max),
    ];
    for (requested, expected) in expectations {
        assert_eq!(
            driver.server_effective_effort("glm-5.2", requested),
            expected,
            "glm-5.2 请求 {requested:?}"
        );
    }
}

#[test]
fn server_effective_effort_defaults_to_identity_for_other_models_and_drivers() {
    let zhipu = driver_for_provider_driver(ProviderDriverKind::Zhipu);
    // 智谱其他模型（4.x/5/5.1 等不支持 effort 字段）：恒等（动态思考无档位语义）。
    assert_eq!(
        zhipu.server_effective_effort("glm-4.6", ReasoningLevel::Medium),
        ReasoningLevel::Medium
    );
    // 其他 driver 未声明服务端归并知识前一律恒等。
    let openai = driver_for_provider_driver(ProviderDriverKind::OpenAI);
    assert_eq!(
        openai.server_effective_effort("gpt-5.6", ReasoningLevel::Max),
        ReasoningLevel::Max
    );
}

#[test]
fn volcengine_server_effective_effort_follows_glm_official_merge() {
    // #2032 用户拍板：方舟上的 GLM 暂按智谱官方归并同源。
    let driver = driver_for_provider_driver(ProviderDriverKind::Volcengine);
    assert_eq!(
        driver.server_effective_effort("glm-5.3", ReasoningLevel::Medium),
        ReasoningLevel::High
    );
    assert_eq!(
        driver.server_effective_effort("glm-5.3", ReasoningLevel::Off),
        ReasoningLevel::Low
    );
    // 非 GLM 模型（kimi-k3/doubao 等）恒等，待官方文档补充。
    assert_eq!(
        driver.server_effective_effort("kimi-k3", ReasoningLevel::Max),
        ReasoningLevel::Max
    );
}

#[test]
fn deepseek_server_effective_effort_follows_official_mapping() {
    // 官方映射表：minimal→low；medium→high；xhigh→high。
    let driver = driver_for_provider_driver(ProviderDriverKind::DeepSeek);
    assert_eq!(
        driver.server_effective_effort("deepseek-chat", ReasoningLevel::Minimal),
        ReasoningLevel::Low
    );
    assert_eq!(
        driver.server_effective_effort("deepseek-chat", ReasoningLevel::Medium),
        ReasoningLevel::High
    );
    assert_eq!(
        driver.server_effective_effort("deepseek-chat", ReasoningLevel::Xhigh),
        ReasoningLevel::High
    );
    assert_eq!(
        driver.server_effective_effort("deepseek-chat", ReasoningLevel::Max),
        ReasoningLevel::Max
    );
    assert_eq!(
        driver.server_effective_effort("deepseek-chat", ReasoningLevel::Off),
        ReasoningLevel::Off
    );
}

#[test]
fn openai_gpt_61_merges_none_and_minimal_to_medium() {
    // 官方：gpt-6.1 不支持 none/minimal，默认 medium。
    let driver = driver_for_provider_driver(ProviderDriverKind::OpenAI);
    assert_eq!(
        driver.server_effective_effort("gpt-6.1-sol", ReasoningLevel::Off),
        ReasoningLevel::Medium
    );
    assert_eq!(
        driver.server_effective_effort("gpt-6.1-sol", ReasoningLevel::Minimal),
        ReasoningLevel::Medium
    );
    assert_eq!(
        driver.server_effective_effort("gpt-6.1-sol", ReasoningLevel::High),
        ReasoningLevel::High
    );
    // 其他模型恒等。
    assert_eq!(
        driver.server_effective_effort("gpt-5.6-sol", ReasoningLevel::Minimal),
        ReasoningLevel::Minimal
    );
}

#[test]
fn kimi_driver_server_effective_effort_merges_k3_vocabulary() {
    let driver = driver_for_provider_driver(ProviderDriverKind::Kimi);
    // k3 词汇 low/high/max：minimal/low→low、medium/high→high、xhigh/max→max；
    // 始终推理 off→low。
    assert_eq!(
        driver.server_effective_effort("k3", ReasoningLevel::Medium),
        ReasoningLevel::High
    );
    assert_eq!(
        driver.server_effective_effort("k3", ReasoningLevel::Off),
        ReasoningLevel::Low
    );
    assert_eq!(
        driver.server_effective_effort("k3", ReasoningLevel::Minimal),
        ReasoningLevel::Low
    );
    assert_eq!(
        driver.server_effective_effort("k3", ReasoningLevel::Xhigh),
        ReasoningLevel::Max
    );
    // k2.7 始终思考：off→low；其余恒等。
    assert_eq!(
        driver.server_effective_effort("kimi-k2.7-code", ReasoningLevel::Off),
        ReasoningLevel::Low
    );
    assert_eq!(
        driver.server_effective_effort("kimi-k2.7-code", ReasoningLevel::High),
        ReasoningLevel::High
    );
}
