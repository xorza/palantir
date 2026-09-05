use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::justify::Justify;
use crate::layout::types::layout_mode::{LayoutMode, ScrollSpec};
use crate::layout::types::limits::MAX_PACKED_GAP;
use crate::layout::types::sizing::Sizes;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::node::node_mode::NodeMode;
use crate::scene::visibility::Visibility;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ConfigureWidget;
use crate::widgets::configure::ThemeDefaults;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::scroll::Scroll;
use crate::widgets::widget::Widget;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel, text::Text};
use glam::Vec2;

fn node_of<W: Configure>(widget: &mut W) -> &mut Node {
    let ConfigureWidget { widget } = widget.configure();
    &mut widget.node
}

/// The `default_*` family fills in only where the caller stayed silent —
/// the trait's half of "explicit wins, the theme fills in the rest". The
/// plain setters can't express it, which is why a widget wrapping
/// another (`ContextMenu` over `Popup`) had no way to resolve its theme
/// without reaching into the wrapped widget.
///
/// A `#[track_caller]` auto id must **not** count as set: every widget
/// has one, so counting it would make `default_id` unreachable.
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
    assert!(matches!(filled.ident, Ident::Verbatim(v) if v == fallback_id));
    assert_eq!(filled.node.padding, Some(fallback_padding));
    assert_eq!(filled.node.min_size, Some(Size::new(10.0, 0.0)));
    assert_eq!(filled.node.max_size, Some(Size::new(200.0, 300.0)));

    // Caller spoke first: every default is a no-op — including the
    // deliberate zero, which is exactly the value a "is it still the
    // default?" check on the value itself would get wrong.
    let kept = Widget::leaf()
        .id(caller_id)
        .padding(caller_padding)
        .min_size(Size::ZERO)
        .max_size(Size::new(50.0, 60.0))
        .default_id(fallback_id)
        .default_padding(fallback_padding)
        .default_min_size(Size::new(10.0, 0.0))
        .default_max_size(Size::new(200.0, 300.0));
    assert!(matches!(kept.ident, Ident::Verbatim(v) if v == caller_id));
    assert_eq!(kept.node.padding, Some(caller_padding));
    assert_eq!(
        kept.node.min_size,
        Some(Size::ZERO),
        "an explicit zero survives"
    );
    assert_eq!(kept.node.max_size, Some(Size::new(50.0, 60.0)));

    // `id_salt` is explicit too, so it blocks the fallback the same way.
    let salted = Widget::leaf().id_salt("row").default_id(fallback_id);
    assert!(matches!(salted.ident, Ident::Hash(_)));
}

#[test]
fn builder_setters_cover_the_complete_external_node_surface() {
    use crate::layout::types::sizing::Sizing;

    let id = WidgetId::from_hash("complete-configuration-surface");
    let size: Sizes = (Sizing::fixed(40.0), Sizing::fixed(30.0)).into();
    let min_size = Size::new(10.0, 12.0);
    let max_size = Size::new(100.0, 120.0);
    let padding = Spacing::new(1.0, 2.0, 3.0, 4.0);
    let margin = Spacing::new(5.0, 6.0, 7.0, 8.0);
    let position = Vec2::new(9.0, 10.0);
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
        .position(position)
        .grid_cell(GridCell::at(2, 3).span(4, 5))
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

    assert!(matches!(widget.ident, Ident::Verbatim(value) if value == id));
    assert_eq!(widget.node.size, Some(size));
    assert_eq!(widget.node.min_size, Some(min_size));
    assert_eq!(widget.node.max_size, Some(max_size));
    assert_eq!(widget.node.padding, Some(padding));
    assert_eq!(widget.node.margin, Some(margin));
    assert_eq!(widget.node.position, position);
    assert_eq!(
        widget.node.grid,
        GridCell {
            row: 2,
            col: 3,
            row_span: 4,
            col_span: 5,
        },
    );
    assert_eq!(widget.authored_gap(), Some(6.0));
    assert_eq!(widget.authored_line_gap(), Some(7.0));
    assert_eq!(widget.authored_justify(), Justify::SpaceBetween);
    assert_eq!(widget.node.align, align);
    assert_eq!(widget.authored_child_align(), child_align);
    assert_eq!(widget.authored_sense(), sense);
    assert!(!widget.authored_disabled());
    assert!(widget.authored_focusable());
    assert_eq!(widget.node.visibility, Visibility::Hidden);
    assert_eq!(widget.node.clip, Some(ClipMode::None));
    assert_eq!(widget.node.transform, transform);
}

#[test]
fn widget_specific_node_setters_reach_the_inner_node() {
    let transform = TranslateScale::new(Vec2::new(4.0, 5.0), 2.0);
    let mut panel = Panel::hstack().transform(transform);
    let mut grid = Grid::new().transform(transform);
    assert_eq!(node_of(&mut panel).transform, transform);
    assert_eq!(node_of(&mut grid).transform, transform);

    let mut item = MenuItem::new("Open").disabled(false);
    assert!(!node_of(&mut item).flags.is_disabled());

    let mut scroll = Scroll::both().with_zoom();
    assert_eq!(
        node_of(&mut scroll).flags.sense(),
        Sense::SCROLL | Sense::PINCH,
    );
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

    let scroll = Widget::scroll(ScrollSpec::VERTICAL);
    assert_eq!(
        scroll.node.mode,
        NodeMode::Resolved(LayoutMode::Scroll(ScrollSpec::VERTICAL)),
    );
    assert_eq!(scroll.node.scroll_spec(), ScrollSpec::VERTICAL);
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

#[test]
fn node_bounds_reject_inversions_on_each_axis_and_setter_order() {
    type Case = (&'static str, fn() -> Widget);

    let cases: &[Case] = &[
        ("minimum exceeds existing x maximum", || {
            Widget::leaf()
                .max_size((10.0, f32::INFINITY))
                .min_size((11.0, 0.0))
        }),
        ("minimum exceeds existing y maximum", || {
            Widget::leaf()
                .max_size((f32::INFINITY, 10.0))
                .min_size((0.0, 11.0))
        }),
        ("maximum is below existing x minimum", || {
            Widget::leaf()
                .min_size((11.0, 0.0))
                .max_size((10.0, f32::INFINITY))
        }),
        ("maximum is below existing y minimum", || {
            Widget::leaf()
                .min_size((0.0, 11.0))
                .max_size((f32::INFINITY, 10.0))
        }),
        ("infinite x minimum", || {
            Widget::leaf().min_size((f32::INFINITY, 0.0))
        }),
        ("infinite y minimum", || {
            Widget::leaf().min_size((0.0, f32::INFINITY))
        }),
        ("NaN minimum", || Widget::leaf().min_size((f32::NAN, 0.0))),
        ("negative infinite maximum", || {
            Widget::leaf().max_size((f32::NEG_INFINITY, f32::INFINITY))
        }),
        ("NaN maximum", || {
            Widget::leaf().max_size((f32::INFINITY, f32::NAN))
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
fn packed_gaps_accept_f16_boundaries_and_reject_invalid_values() {
    let valid = Widget::hstack()
        .gap(MAX_PACKED_GAP)
        .line_gap(MAX_PACKED_GAP);
    assert_eq!(valid.authored_gap(), Some(MAX_PACKED_GAP));
    assert_eq!(valid.authored_line_gap(), Some(MAX_PACKED_GAP));

    type Case = (&'static str, fn() -> Widget);
    let cases: &[Case] = &[
        ("negative gap", || Widget::hstack().gap(-1.0)),
        ("NaN gap", || Widget::hstack().gap(f32::NAN)),
        ("positive infinite gap", || {
            Widget::hstack().gap(f32::INFINITY)
        }),
        ("negative infinite gap", || {
            Widget::hstack().gap(f32::NEG_INFINITY)
        }),
        ("f16-overflow gap", || {
            Widget::hstack().gap(MAX_PACKED_GAP + 1.0)
        }),
        ("negative line gap", || Widget::wrap_hstack().line_gap(-1.0)),
        ("NaN line gap", || Widget::wrap_hstack().line_gap(f32::NAN)),
        ("positive infinite line gap", || {
            Widget::wrap_hstack().line_gap(f32::INFINITY)
        }),
        ("negative infinite line gap", || {
            Widget::wrap_hstack().line_gap(f32::NEG_INFINITY)
        }),
        ("f16-overflow line gap", || {
            Widget::wrap_hstack().line_gap(MAX_PACKED_GAP + 1.0)
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
    // No parent context in this micro-test — `Ident::raw_id(None)`
    // yields the bare auto/explicit id without any parent-scoping
    // mix.
    w.configure().widget.ident.raw_id(None)
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

/// A themed fallback faces the same NaN screen an authored value does.
/// A NaN edge that slips through reaches layout and surfaces frames
/// later as a widget that measured to nothing, with nothing pointing
/// back at the theme that set it.
#[test]
#[should_panic(expected = "NaN in padding")]
fn a_themed_padding_is_nan_screened_like_an_authored_one() {
    let _ = Panel::vstack().default_padding(Spacing::all(f32::NAN));
}
