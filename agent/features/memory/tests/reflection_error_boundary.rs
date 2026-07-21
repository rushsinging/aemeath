use memory::{ReflectionEngine, ReflectionPromptPort};

#[test]
fn parse_errors_never_expose_model_response_content() {
    let secret = "REFLECTION-RAW-SECRET";
    let engine = ReflectionEngine;

    for response in [
        format!("not json {secret}"),
        format!(r#"{{"deviations":["{secret}"]"#),
    ] {
        let error = engine
            .parse_output(&response)
            .expect_err("invalid model output must fail");
        assert!(
            !error.to_string().contains(secret),
            "parse error leaked model response content: {error}"
        );
    }
}
