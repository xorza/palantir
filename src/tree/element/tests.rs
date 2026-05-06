use super::*;

#[test]
fn paint_attrs_round_trip() {
    let cases: &[(&str, Sense, bool, ClipMode, bool)] = &[
        ("inert_default", Sense::NONE, false, ClipMode::None, false),
        ("sense_hover", Sense::HOVER, false, ClipMode::None, false),
        ("sense_click", Sense::CLICK, false, ClipMode::None, false),
        ("sense_drag", Sense::DRAG, false, ClipMode::None, false),
        (
            "sense_click_and_drag",
            Sense::CLICK_AND_DRAG,
            false,
            ClipMode::None,
            false,
        ),
        ("disabled", Sense::NONE, true, ClipMode::None, false),
        ("clip_rect", Sense::NONE, false, ClipMode::Rect, false),
        ("clip_rounded", Sense::NONE, false, ClipMode::Rounded, false),
        ("focusable", Sense::NONE, false, ClipMode::None, true),
        (
            "all_set_no_alias",
            Sense::CLICK_AND_DRAG,
            true,
            ClipMode::Rounded,
            true,
        ),
    ];
    for (label, sense, disabled, clip, focusable) in cases {
        let f = PaintAttrs::pack(*sense, *disabled, *clip, *focusable);
        assert_eq!(f.sense(), *sense, "case: {label} sense");
        assert_eq!(f.is_disabled(), *disabled, "case: {label} disabled");
        assert_eq!(f.clip_mode(), *clip, "case: {label} clip");
        assert_eq!(f.is_focusable(), *focusable, "case: {label} focusable");
    }
}

#[test]
fn fits_in_one_byte() {
    assert_eq!(std::mem::size_of::<PaintAttrs>(), 1);
}
