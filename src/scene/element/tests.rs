use crate::layout::types::limits::MAX_PACKED_GAP;
use crate::primitives::widget_id::WidgetId;
use crate::scene::element::*;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel, text::Text};

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
fn fits_in_two_bytes() {
    // Grew from 1 byte to 2 when `Sense::PINCH` claimed bit 4,
    // pushing `DISABLED`/`CLIP`/`FOCUSABLE` past the u8 ceiling.
    // Still packed — sense (5 bits) + disabled (1) + clip (2) +
    // focusable (1) = 9 bits, fitting in a u16 with 7 spare.
    assert_eq!(std::mem::size_of::<NodeFlags>(), 2);
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
fn constructors_install_layout_modes_and_payloads() {
    let cases = [
        (Element::leaf(), LayoutMode::Leaf),
        (Element::hstack(), LayoutMode::HStack),
        (Element::vstack(), LayoutMode::VStack),
        (Element::wrap_hstack(), LayoutMode::WrapHStack),
        (Element::wrap_vstack(), LayoutMode::WrapVStack),
        (Element::zstack(), LayoutMode::ZStack),
        (Element::canvas(), LayoutMode::Canvas),
    ];

    for (element, expected) in cases {
        assert_eq!(element.mode, expected);
        assert_eq!(element.mode_payload, ModePayload::NONE);
    }

    let grid = Element::grid();
    assert_eq!(grid.mode, LayoutMode::Grid);
    assert!(std::panic::catch_unwind(|| grid.grid_def_id()).is_err());
    let last_grid = GridDefId::from_index(65_534);
    assert_eq!(usize::from(last_grid), 65_534);
    assert!(std::panic::catch_unwind(|| GridDefId::from_index(65_535)).is_err());

    let scroll = Element::scroll(ScrollSpec::VERTICAL);
    assert_eq!(scroll.mode, LayoutMode::Scroll);
    assert_eq!(scroll.scroll_spec(), ScrollSpec::VERTICAL);
}

#[test]
fn layout_core_round_trips_mode_align_visibility() {
    use crate::layout::types::align::{Align, HAlign, VAlign};
    use crate::scene::visibility::Visibility;
    let cases: &[(LayoutMode, ModePayload, Align, Visibility)] = &[
        (
            LayoutMode::Leaf,
            ModePayload::NONE,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::HStack,
            ModePayload::NONE,
            Align::new(HAlign::Left, VAlign::Center),
            Visibility::Hidden,
        ),
        (
            LayoutMode::Grid,
            ModePayload::grid(GridDefId::from_index(42)),
            Align::new(HAlign::Right, VAlign::Bottom),
            Visibility::Collapsed,
        ),
        (
            LayoutMode::Scroll,
            ModePayload::scroll(ScrollSpec::VERTICAL),
            Align::new(HAlign::Center, VAlign::Top),
            Visibility::Visible,
        ),
        (
            LayoutMode::Scroll,
            ModePayload::scroll(ScrollSpec::HORIZONTAL),
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::Scroll,
            ModePayload::scroll(ScrollSpec::BOTH),
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Hidden,
        ),
        (
            LayoutMode::WrapHStack,
            ModePayload::NONE,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::WrapVStack,
            ModePayload::NONE,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::ZStack,
            ModePayload::NONE,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::Canvas,
            ModePayload::NONE,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::VStack,
            ModePayload::NONE,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::Grid,
            ModePayload::NONE,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
    ];
    for &(mode, payload, align, vis) in cases {
        let mut element = Element::new(mode, payload);
        element.align = align;
        element.visibility = vis;
        let core = LayoutCore::from_element(&element);
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

#[test]
fn element_bounds_accept_ordered_ranges_and_equal_axis_boundaries() {
    let min_then_max = Element::leaf()
        .min_size((10.0, 20.0))
        .max_size((10.0, 30.0));
    assert_eq!(min_then_max.min_size, Size::new(10.0, 20.0));
    assert_eq!(min_then_max.max_size, Size::new(10.0, 30.0));

    let max_then_min = Element::leaf()
        .max_size((30.0, 20.0))
        .min_size((10.0, 20.0));
    assert_eq!(max_then_min.min_size, Size::new(10.0, 20.0));
    assert_eq!(max_then_min.max_size, Size::new(30.0, 20.0));

    let unbounded = Element::leaf().max_size(Size::INF);
    assert_eq!(unbounded.max_size, Size::INF);
}

#[test]
#[cfg(debug_assertions)]
fn element_bounds_reject_inversions_on_each_axis_and_setter_order() {
    type Case = (&'static str, fn() -> Element);

    let cases: &[Case] = &[
        ("minimum exceeds existing x maximum", || {
            Element::leaf()
                .max_size((10.0, f32::INFINITY))
                .min_size((11.0, 0.0))
        }),
        ("minimum exceeds existing y maximum", || {
            Element::leaf()
                .max_size((f32::INFINITY, 10.0))
                .min_size((0.0, 11.0))
        }),
        ("maximum is below existing x minimum", || {
            Element::leaf()
                .min_size((11.0, 0.0))
                .max_size((10.0, f32::INFINITY))
        }),
        ("maximum is below existing y minimum", || {
            Element::leaf()
                .min_size((0.0, 11.0))
                .max_size((f32::INFINITY, 10.0))
        }),
        ("infinite x minimum", || {
            Element::leaf().min_size((f32::INFINITY, 0.0))
        }),
        ("infinite y minimum", || {
            Element::leaf().min_size((0.0, f32::INFINITY))
        }),
        ("NaN minimum", || Element::leaf().min_size((f32::NAN, 0.0))),
        ("negative infinite maximum", || {
            Element::leaf().max_size((f32::NEG_INFINITY, f32::INFINITY))
        }),
        ("NaN maximum", || {
            Element::leaf().max_size((f32::INFINITY, f32::NAN))
        }),
    ];

    for &(label, build) in cases {
        assert!(
            std::panic::catch_unwind(build).is_err(),
            "case `{label}` must panic",
        );
    }
}

#[test]
#[cfg(debug_assertions)]
fn packed_gaps_accept_f16_boundaries_and_reject_invalid_values() {
    let valid = Element::hstack()
        .gap(MAX_PACKED_GAP)
        .line_gap(MAX_PACKED_GAP);
    assert_eq!(valid.gaps.gap(), MAX_PACKED_GAP);
    assert_eq!(valid.gaps.line_gap(), MAX_PACKED_GAP);

    type Case = (&'static str, fn() -> Element);
    let cases: &[Case] = &[
        ("negative gap", || Element::hstack().gap(-1.0)),
        ("NaN gap", || Element::hstack().gap(f32::NAN)),
        ("positive infinite gap", || {
            Element::hstack().gap(f32::INFINITY)
        }),
        ("negative infinite gap", || {
            Element::hstack().gap(f32::NEG_INFINITY)
        }),
        ("f16-overflow gap", || {
            Element::hstack().gap(MAX_PACKED_GAP + 1.0)
        }),
        ("negative line gap", || {
            Element::wrap_hstack().line_gap(-1.0)
        }),
        ("NaN line gap", || Element::wrap_hstack().line_gap(f32::NAN)),
        ("positive infinite line gap", || {
            Element::wrap_hstack().line_gap(f32::INFINITY)
        }),
        ("negative infinite line gap", || {
            Element::wrap_hstack().line_gap(f32::NEG_INFINITY)
        }),
        ("f16-overflow line gap", || {
            Element::wrap_hstack().line_gap(MAX_PACKED_GAP + 1.0)
        }),
    ];

    for &(label, build) in cases {
        assert!(
            std::panic::catch_unwind(build).is_err(),
            "case `{label}` must panic",
        );
    }
}

fn assert_distinct(label: &str, a: WidgetId, b: WidgetId) {
    assert_ne!(
        a, b,
        "{label}: two `.auto_id()` calls on different lines produced the same id — \
         `Configure::auto_id` is missing `#[track_caller]`."
    );
}

fn id_of<W: Configure>(mut w: W) -> WidgetId {
    // No parent context in this micro-test — `Salt::resolve(None)`
    // yields the bare auto/explicit id without any parent-scoping
    // mix.
    w.element_mut().salt.resolve(None)
}

/// Pin: [`Configure::auto_id`] is `#[track_caller]` and resolves a stable
/// id at the *call site*. Two `.auto_id()` calls on different source lines
/// must produce distinct `WidgetId`s — that's the cross-frame-stability
/// contract for builders that opt into auto ids. Dropping the attribute
/// collapses all calls onto one id (occurrence-counter disambiguation
/// still works within a frame, but state stability degrades). The case
/// list covers every public widget constructor so a regression in any
/// one is caught.
#[test]
fn auto_id_propagates_track_caller_through_every_widget() {
    type Case = (&'static str, fn() -> (WidgetId, WidgetId));
    let cases: &[Case] = &[
        ("Button", || {
            (
                id_of(Button::new().auto_id()),
                id_of(Button::new().auto_id()),
            )
        }),
        ("Frame", || {
            (id_of(Frame::new().auto_id()), id_of(Frame::new().auto_id()))
        }),
        ("Grid", || {
            (id_of(Grid::new().auto_id()), id_of(Grid::new().auto_id()))
        }),
        ("Text", || {
            (
                id_of(Text::new("x").auto_id()),
                id_of(Text::new("x").auto_id()),
            )
        }),
        ("Panel::hstack", || {
            (
                id_of(Panel::hstack().auto_id()),
                id_of(Panel::hstack().auto_id()),
            )
        }),
        ("Panel::vstack", || {
            (
                id_of(Panel::vstack().auto_id()),
                id_of(Panel::vstack().auto_id()),
            )
        }),
        ("Panel::zstack", || {
            (
                id_of(Panel::zstack().auto_id()),
                id_of(Panel::zstack().auto_id()),
            )
        }),
        ("Panel::canvas", || {
            (
                id_of(Panel::canvas().auto_id()),
                id_of(Panel::canvas().auto_id()),
            )
        }),
        ("Panel::wrap_hstack", || {
            (
                id_of(Panel::wrap_hstack().auto_id()),
                id_of(Panel::wrap_hstack().auto_id()),
            )
        }),
        ("Panel::wrap_vstack", || {
            (
                id_of(Panel::wrap_vstack().auto_id()),
                id_of(Panel::wrap_vstack().auto_id()),
            )
        }),
    ];
    for (label, mk) in cases {
        let (a, b) = mk();
        assert_distinct(label, a, b);
    }
}

/// Sanity: `id_salt(...)` overrides `auto_id`, so two calls with the
/// same explicit key on different lines produce the *same* id.
#[test]
fn id_salt_overrides_auto_id() {
    assert_eq!(
        id_of(Button::new().id(WidgetId::from_hash("k"))),
        id_of(Button::new().id(WidgetId::from_hash("k"))),
    );
}

/// `Configure::auto_id()` re-derives the id at *its* call site. A helper
/// that builds widgets internally collapses every helper-internal
/// `.auto_id()` to one source location; appending `.auto_id()` at the
/// caller recovers per-line distinctness.
#[test]
fn auto_id_redirects_to_call_site() {
    fn helper() -> Button<'static> {
        Button::new().auto_id()
    }
    // Both `helper()` invocations resolve `.auto_id()` inside the helper
    // body — same source line, same id.
    assert_eq!(id_of(helper()), id_of(helper()));
    // With `.auto_id()` on different source lines, the ids diverge.
    let a = id_of(helper().auto_id());
    let b = id_of(helper().auto_id());
    assert_distinct("auto_id() at call site", a, b);
}
