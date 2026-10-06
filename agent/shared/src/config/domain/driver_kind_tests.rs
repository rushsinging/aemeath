use super::DriverKind;

#[test]
fn driver_kind_parse_as_str_round_trips_for_all_variants() {
    for kind in DriverKind::ALL {
        assert_eq!(DriverKind::parse(kind.as_str()), Some(kind));
    }
}

#[test]
fn driver_kind_parse_rejects_unknown_and_case_sensitive() {
    assert_eq!(DriverKind::parse("not-a-driver"), None);
    // 身份词表精确匹配：大小写敏感（调用方 lowercase 属其自身语义）。
    assert_eq!(DriverKind::parse("Anthropic"), None);
}

/// env 映射与词表一致：每个 driver 的 env 归属逐项锁定——词表新增
/// 成员或 driver_env 分支漂移时在此失败（#1850 身份敏感区）。
#[test]
fn driver_env_mapping_is_pinned_per_driver_kind() {
    use super::super::driver_env::driver_api_key_env_name;
    let pinned = [
        (DriverKind::Anthropic, "ANTHROPIC_API_KEY"),
        (DriverKind::OpenAI, "OPENAI_API_KEY"),
        (DriverKind::Volcengine, "VOLCENGINE_CODING_PLAN_API_KEY"),
        (DriverKind::Minimax, "MINIMAX_API_KEY"),
        (DriverKind::Mimo, "MIMO_API_KEY"),
        (DriverKind::DeepSeek, "DEEPSEEK_API_KEY"),
        (DriverKind::Agnes, "AGNES_API_KEY"),
        (DriverKind::Ollama, "OLLAMA_API_KEY"),
    ];
    for (kind, env) in pinned {
        assert_eq!(driver_api_key_env_name(kind.as_str()), Some(env));
    }
    // Zhipu / LiteLLM 无 driver 专属 env（显式 None）。
    assert_eq!(driver_api_key_env_name(DriverKind::Zhipu.as_str()), None);
    assert_eq!(driver_api_key_env_name(DriverKind::LiteLLM.as_str()), None);
}
