use super::*;

#[test]
fn commit_guidance_bilingual_and_fallback_en() {
    let zh = commit_guidance_template("zh");
    let en = commit_guidance_template("en");
    assert!(zh.contains("创建"));
    assert!(en.contains("creating"));
    assert_eq!(commit_guidance_template("fr"), en);
}

#[test]
fn commit_guidance_contains_trailer_placeholder() {
    for s in [
        commit_guidance_template("zh"),
        commit_guidance_template("en"),
    ] {
        assert!(s.contains("{trailer}"));
        assert!(s.contains("Co-Authored-By"));
    }
}
