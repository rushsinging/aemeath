use super::*;

#[test]
fn test_spinner_anim_default_is_empty() {
    let anim = SpinnerAnim::default();
    assert_eq!(anim.frame, 0);
    assert_eq!(anim.phase_frame, 0);
    assert_eq!(anim.verb, "");
}

#[test]
fn test_spinner_anim_advance_increments_frame() {
    let mut anim = SpinnerAnim::default();
    anim.advance();
    anim.advance();
    assert_eq!(anim.frame, 2);
    assert_eq!(anim.phase_frame, 2);
}

#[test]
fn test_spinner_anim_advance_wraps_at_max() {
    let mut anim = SpinnerAnim {
        frame: u64::MAX,
        phase_frame: u64::MAX,
        verb: String::new(),
    };
    anim.advance();
    assert_eq!(anim.frame, 0);
    assert_eq!(anim.phase_frame, 0);
}

#[test]
fn test_spinner_anim_pick_verb_selects_from_pool_and_is_stable() {
    let mut anim = SpinnerAnim {
        frame: 42,
        phase_frame: 7,
        verb: String::new(),
    };
    anim.pick_verb();
    let chosen = anim.verb.clone();
    assert!(SPINNER_VERBS.contains(&chosen.as_str()));
    // pick_verb 复位 frame
    assert_eq!(anim.frame, 0);
    assert_eq!(anim.phase_frame, 0);
    // 不再调用 pick_verb，verb 保持稳定
    anim.advance();
    assert_eq!(anim.verb, chosen);
}
