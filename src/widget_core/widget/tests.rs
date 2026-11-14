use crate::internals::panic_probe;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::SizeSpec;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::math::domain;
use crate::primitives::packed::half_simd::F16x4;
use crate::scene::node::id_recipe::{IdRecipe, ScopedRecipe};
use crate::scene::node::node_mode::NodeMode;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureDefaults;
use crate::widget_core::widget::Ident;
use crate::widget_core::widget::Widget;
use glam::Vec2;
use std::num::NonZeroU16;

/// The `default_*` family fills in only where the caller stayed silent (the trait's half of "explicit wins, the theme fills the rest"). A `#[track_caller]` auto id must not count as set, or `default_id` is unreachable.
#[test]
fn default_setters_fill_only_where_the_caller_stayed_silent() {
    let fallback_id = WidgetId::from_hash("fallback");
    let caller_id = WidgetId::from_hash("caller");
    let fallback_padding = Spacing::all(4.0);
    let caller_padding = Spacing::all(9.0);

    // Untouched but for its auto id: every default lands.
    let filled = Widget::leaf()
        .default_id(fallback_id)
        .default_padding(fallback_padding)
        .default_min_size(Size::new(10.0, 0.0))
        .default_max_size(Size::new(200.0, 300.0));
    assert!(matches!(filled.ident, Ident::Recipe(IdRecipe::Verbatim(v)) if v == fallback_id));
    assert_eq!(filled.node.padding, Some(fallback_padding));
    assert_eq!(filled.node.min_size, Some(Size::new(10.0, 0.0)));
    assert_eq!(filled.node.max_size, Some(Size::new(200.0, 300.0)));

    // Caller spoke first: every default is a no-op, including the deliberate zero.
    let kept = Widget::leaf()
        .id(caller_id)
        .padding(caller_padding)
        .min_size(Size::ZERO)
        .max_size(Size::new(50.0, 60.0))
        .default_id(fallback_id)
        .default_padding(fallback_padding)
        .default_min_size(Size::new(10.0, 0.0))
        .default_max_size(Size::new(200.0, 300.0));
    assert!(matches!(kept.ident, Ident::Recipe(IdRecipe::Verbatim(v)) if v == caller_id));
    assert_eq!(kept.node.padding, Some(caller_padding));
    assert_eq!(
        kept.node.min_size,
        Some(Size::ZERO),
        "an explicit zero survives"
    );
    assert_eq!(kept.node.max_size, Some(Size::new(50.0, 60.0)));

    // `id_salt` is explicit too, so it blocks the fallback the same way.
    let salted = Widget::leaf().id_salt("row").default_id(fallback_id);
    assert!(matches!(
        salted.ident,
        Ident::Recipe(IdRecipe::Scoped(ScopedRecipe::Hash(_)))
    ));
}

#[test]
fn builder_setters_cover_the_complete_external_node_surface() {
    use crate::primitives::layout::sizing::Sizing;

    let id = WidgetId::from_hash("complete-configuration-surface");
    let size: SizeSpec = (Sizing::fixed(40.0), Sizing::fixed(30.0)).into();
    let min_size = Size::new(10.0, 12.0);
    let max_size = Size::new(100.0, 120.0);
    let padding = Spacing::new(1.0, 2.0, 3.0, 4.0);
    let margin = Spacing::new(5.0, 6.0, 7.0, 8.0);
    let pos = Vec2::new(9.0, 10.0);
    let align = Align::new(HAlign::Right, VAlign::Bottom);
    let child_align = Align::new(HAlign::Center, VAlign::Top);
    let sense = Sense::CLICK | Sense::DRAG;
    let transform = TranslateScale::new(Vec2::new(11.0, 12.0), 1.5);

    let mut widget = Widget::hstack()
        .id(id)
        .size(size)
        .min_size(min_size)
        .max_size(max_size)
        .padding(padding)
        .margin(margin)
        .at(pos)
        .grid_cell(
            GridCell::at(2, 3).with_span(NonZeroU16::new(4).unwrap(), NonZeroU16::new(5).unwrap()),
        )
        .gap(6.0)
        .line_gap(7.0)
        .justify(Justify::SpaceBetween)
        .align(align)
        .child_align(child_align)
        .sense(sense)
        .disabled(false)
        .focusable(true)
        .visibility(Visibility::Hidden)
        .clip(ClipMode::None);
    widget.node.transform = transform;

    // Read back through the `authored_*` surface, the only one an outside widget has.
    assert!(matches!(widget.ident, Ident::Recipe(IdRecipe::Verbatim(value)) if value == id));
    assert_eq!(widget.authored().size(), Some(size));
    assert_eq!(widget.authored().min_size(), Some(min_size));
    assert_eq!(widget.authored().max_size(), Some(max_size));
    assert_eq!(widget.authored().padding(), Some(padding));
    assert_eq!(widget.authored().margin(), Some(margin));
    assert_eq!(widget.authored().pos(), pos);
    assert_eq!(
        widget.authored().grid_cell(),
        GridCell::at(2, 3).with_span(NonZeroU16::new(4).unwrap(), NonZeroU16::new(5).unwrap())
    );
    assert_eq!(widget.authored().gap(), Some(6.0));
    assert_eq!(widget.authored().line_gap(), Some(7.0));
    assert_eq!(widget.authored().justify(), Some(Justify::SpaceBetween));
    assert_eq!(widget.authored().align(), align);
    assert_eq!(widget.authored().child_align(), child_align);
    assert_eq!(widget.authored().sense(), sense);
    assert!(!widget.authored().disabled());
    assert!(widget.authored().focusable());
    assert_eq!(widget.authored().visibility(), Visibility::Hidden);
    assert_eq!(widget.authored().clip(), Some(ClipMode::None));
    assert_eq!(widget.authored().transform(), transform);
}

/// The other half of [`builder_setters_cover_the_complete_external_node_surface`]: every reader answers the untouched default on an unconfigured widget.
#[test]
fn authored_readers_answer_the_defaults_before_anything_is_set() {
    let widget = Widget::hstack();

    assert_eq!(widget.authored().size(), None);
    assert_eq!(widget.authored().min_size(), None);
    assert_eq!(widget.authored().max_size(), None);
    assert_eq!(widget.authored().padding(), None);
    assert_eq!(widget.authored().margin(), None);
    assert_eq!(widget.authored().clip(), None);
    assert_eq!(widget.authored().gap(), None);
    assert_eq!(widget.authored().line_gap(), None);
    assert_eq!(widget.authored().pos(), Vec2::ZERO);
    assert_eq!(widget.authored().grid_cell(), GridCell::default());
    assert_eq!(widget.authored().justify(), None);
    assert_eq!(widget.authored().align(), Align::default());
    assert_eq!(widget.authored().child_align(), Align::default());
    assert_eq!(widget.authored().sense(), Sense::NONE);
    assert!(!widget.authored().disabled());
    assert!(!widget.authored().focusable());
    assert_eq!(widget.authored().visibility(), Visibility::Visible);
    assert_eq!(widget.authored().transform(), TranslateScale::IDENTITY);
}

#[test]
fn constructors_install_layout_modes() {
    let cases = [
        (Widget::leaf(), LayoutMode::Leaf),
        (Widget::hstack(), LayoutMode::Stack(Axis::X)),
        (Widget::vstack(), LayoutMode::Stack(Axis::Y)),
        (Widget::wrap_hstack(), LayoutMode::WrapStack(Axis::X)),
        (Widget::wrap_vstack(), LayoutMode::WrapStack(Axis::Y)),
        (Widget::zstack(), LayoutMode::ZStack),
        (Widget::canvas(), LayoutMode::Canvas),
    ];
    for (widget, expected) in cases {
        assert_eq!(widget.node.mode, NodeMode::Resolved(expected));
    }
    assert_eq!(Widget::grid().node.mode, NodeMode::PendingGrid);
    assert_eq!(Widget::scrollbars().node.mode, NodeMode::PendingScrollbars);
    assert_eq!(
        Widget::scroll(ScrollAxes::VERTICAL).node.mode,
        NodeMode::Resolved(LayoutMode::Scroll(ScrollAxes::VERTICAL)),
    );
}

#[test]
fn node_bounds_accept_ordered_ranges_and_equal_axis_boundaries() {
    let min_then_max = Widget::leaf().min_size((10.0, 20.0)).max_size((10.0, 30.0));
    assert_eq!(min_then_max.node.min_size, Some(Size::new(10.0, 20.0)));
    assert_eq!(min_then_max.node.max_size, Some(Size::new(10.0, 30.0)));

    let max_then_min = Widget::leaf().max_size((30.0, 20.0)).min_size((10.0, 20.0));
    assert_eq!(max_then_min.node.min_size, Some(Size::new(10.0, 20.0)));
    assert_eq!(max_then_min.node.max_size, Some(Size::new(30.0, 20.0)));

    let unbounded = Widget::leaf().max_size(Size::INF);
    assert_eq!(unbounded.node.max_size, Some(Size::INF));
}

/// A minimum above the maximum resolves as in CSS (minimum wins, per axis, either setter order); each bound panics only outside its own kind (minimum a length, maximum an extent).
#[test]
fn node_bounds_coerce_their_order_and_validate_their_kinds() {
    type Case = (&'static str, fn() -> Widget);
    let raised = Widget::leaf()
        .max_size((10.0, f32::INFINITY))
        .min_size((11.0, 0.0));
    assert_eq!(raised.node.max_size, Some(Size::new(11.0, f32::INFINITY)));
    let raised = Widget::leaf()
        .min_size((0.0, 11.0))
        .max_size((f32::INFINITY, 10.0));
    assert_eq!(raised.node.max_size, Some(Size::new(f32::INFINITY, 11.0)));

    let cases: &[Case] = &[
        (domain::LENGTH_RULE, || {
            Widget::leaf().min_size((f32::INFINITY, 0.0))
        }),
        (domain::LENGTH_RULE, || {
            Widget::leaf().min_size((0.0, f32::INFINITY))
        }),
        (domain::LENGTH_RULE, || {
            Widget::leaf().min_size((f32::NAN, 0.0))
        }),
        (domain::LENGTH_RULE, || Widget::leaf().min_size((-1.0, 0.0))),
        (domain::EXTENT_RULE, || {
            Widget::leaf().max_size((f32::NEG_INFINITY, f32::INFINITY))
        }),
        (domain::EXTENT_RULE, || {
            Widget::leaf().max_size((f32::INFINITY, f32::NAN))
        }),
    ];
    for &(rule, build) in cases {
        panic_probe::assert_panics_with(rule, build);
    }
}

#[test]
fn packed_gaps_accept_f16_boundaries_and_reject_invalid_values() {
    type Case = (&'static str, fn() -> Widget);

    let valid = Widget::hstack()
        .gap(F16x4::MAX_LANE)
        .line_gap(F16x4::MAX_LANE);
    assert_eq!(valid.authored().gap(), Some(F16x4::MAX_LANE));
    assert_eq!(valid.authored().line_gap(), Some(F16x4::MAX_LANE));

    let cases: &[Case] = &[
        ("negative gap", || Widget::hstack().gap(-1.0)),
        ("NaN gap", || Widget::hstack().gap(f32::NAN)),
        ("positive infinite gap", || {
            Widget::hstack().gap(f32::INFINITY)
        }),
        ("negative infinite gap", || {
            Widget::hstack().gap(f32::NEG_INFINITY)
        }),
        ("f16-overflow gap", || Widget::hstack().gap(65520.0)),
        ("negative line gap", || Widget::wrap_hstack().line_gap(-1.0)),
        ("NaN line gap", || Widget::wrap_hstack().line_gap(f32::NAN)),
        ("positive infinite line gap", || {
            Widget::wrap_hstack().line_gap(f32::INFINITY)
        }),
        ("negative infinite line gap", || {
            Widget::wrap_hstack().line_gap(f32::NEG_INFINITY)
        }),
        ("f16-overflow line gap", || {
            Widget::wrap_hstack().line_gap(65520.0)
        }),
    ];

    for &(_label, build) in cases {
        panic_probe::assert_panics_with(domain::GAP_RULE, build);
    }
}

/// A canvas position is an offset per axis and a grid span a count: zero names no cell and panics.
#[test]
fn position_validates_its_kind_and_a_grid_span_keeps_its_cell() {
    panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
        Widget::leaf().at(Vec2::new(f32::NAN, 0.0))
    });
    panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
        Widget::leaf().at(Vec2::new(0.0, f32::INFINITY))
    });
    let at = Widget::leaf().at(Vec2::new(-5.0, 7.0));
    assert_eq!(at.node.pos, Vec2::new(-5.0, 7.0));
    let cell =
        GridCell::at(1, 2).with_span(NonZeroU16::new(3).unwrap(), NonZeroU16::new(4).unwrap());
    assert_eq!(
        (cell.row(), cell.col(), cell.row_span(), cell.col_span()),
        (
            1,
            2,
            NonZeroU16::new(3).unwrap(),
            NonZeroU16::new(4).unwrap()
        ),
    );
}
