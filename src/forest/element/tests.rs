use super::*;

#[test]
fn flag_setters_round_trip_each_field_independently() {
    let cases: &[(&str, Sense, bool, ClipMode, bool)] = &[
        ("inert_default", Sense::NONE, false, ClipMode::None, false),
        (
            "sense_click_and_drag",
            Sense::CLICK | Sense::DRAG,
            false,
            ClipMode::None,
            false,
        ),
        (
            "disabled_clip_rounded_focusable",
            Sense::NONE,
            true,
            ClipMode::Rounded,
            true,
        ),
        (
            "all_set_no_alias",
            Sense::CLICK | Sense::DRAG,
            true,
            ClipMode::Rounded,
            true,
        ),
    ];
    for (label, sense, disabled, clip, focusable) in cases {
        let mut f = NodeFlags::default();
        f.set_sense(*sense);
        f.set_disabled(*disabled);
        f.set_clip(*clip);
        f.set_focusable(*focusable);
        assert_eq!(f.sense(), *sense, "case: {label} sense");
        assert_eq!(f.is_disabled(), *disabled, "case: {label} disabled");
        assert_eq!(f.clip_mode(), *clip, "case: {label} clip");
        assert_eq!(f.is_focusable(), *focusable, "case: {label} focusable");
    }
}

#[test]
fn fits_in_one_byte() {
    assert_eq!(std::mem::size_of::<NodeFlags>(), 1);
}

#[test]
fn layout_core_size() {
    assert_eq!(std::mem::size_of::<LayoutCore>(), 28);
}

#[test]
fn layout_mode_size() {
    assert_eq!(std::mem::size_of::<LayoutMode>(), 1);
}

#[test]
fn layout_core_round_trips_mode_align_visibility() {
    use crate::forest::visibility::Visibility;
    use crate::layout::types::align::{Align, HAlign, VAlign};
    let cases: &[(LayoutMode, u16, Align, Visibility)] = &[
        (
            LayoutMode::Leaf,
            0,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::HStack,
            0,
            Align::new(HAlign::Left, VAlign::Center),
            Visibility::Hidden,
        ),
        (
            LayoutMode::Grid,
            42,
            Align::new(HAlign::Right, VAlign::Bottom),
            Visibility::Collapsed,
        ),
        (
            LayoutMode::ScrollVertical,
            0,
            Align::new(HAlign::Center, VAlign::Top),
            Visibility::Visible,
        ),
        (
            LayoutMode::ScrollHorizontal,
            0,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::ScrollBoth,
            0,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Hidden,
        ),
        (
            LayoutMode::WrapHStack,
            0,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::WrapVStack,
            0,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::ZStack,
            0,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::Canvas,
            0,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::VStack,
            0,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::Grid,
            65535,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
    ];
    for &(mode, payload, align, vis) in cases {
        let core = LayoutCore {
            size: Sizes::default(),
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            mode_payload: payload,
            bits: LayoutCore::pack_bits(align, vis),
            mode,
        };
        assert_eq!(core.mode, mode, "mode for {mode:?}/{align:?}/{vis:?}");
        assert_eq!(core.mode_payload, payload, "payload for {mode:?}");
        assert_eq!(core.align(), align, "align for {mode:?}/{align:?}/{vis:?}");
        assert_eq!(
            core.visibility(),
            vis,
            "visibility for {mode:?}/{align:?}/{vis:?}"
        );
    }
}
