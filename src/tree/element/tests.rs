use super::*;

#[test]
fn default_round_trips_to_inert_state() {
    let f = PaintAttrs::pack(Sense::NONE, false, ClipMode::None, false);
    assert_eq!(f.sense(), Sense::NONE);
    assert!(!f.is_disabled());
    assert_eq!(f.clip_mode(), ClipMode::None);
    assert!(!f.is_focusable());
}

#[test]
fn every_sense_variant_round_trips() {
    for sense in [
        Sense::NONE,
        Sense::HOVER,
        Sense::CLICK,
        Sense::DRAG,
        Sense::CLICK_AND_DRAG,
    ] {
        let f = PaintAttrs::pack(sense, false, ClipMode::None, false);
        assert_eq!(f.sense(), sense, "sense {sense:?}");
    }
}

#[test]
fn flag_bits_round_trip() {
    for &(disabled, clip, focusable) in &[
        (false, ClipMode::None, false),
        (true, ClipMode::None, false),
        (false, ClipMode::Rect, false),
        (false, ClipMode::Rounded, false),
        (false, ClipMode::None, true),
        (true, ClipMode::Rounded, true),
    ] {
        let f = PaintAttrs::pack(Sense::NONE, disabled, clip, focusable);
        assert_eq!(f.is_disabled(), disabled);
        assert_eq!(f.clip_mode(), clip);
        assert_eq!(f.is_focusable(), focusable);
    }
}

#[test]
fn fields_do_not_alias_each_other() {
    let f = PaintAttrs::pack(Sense::CLICK_AND_DRAG, true, ClipMode::Rounded, true);
    assert_eq!(f.sense(), Sense::CLICK_AND_DRAG);
    assert!(f.is_disabled());
    assert_eq!(f.clip_mode(), ClipMode::Rounded);
    assert!(f.is_focusable());
}

#[test]
fn fits_in_one_byte() {
    assert_eq!(std::mem::size_of::<PaintAttrs>(), 1);
}
