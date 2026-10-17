//! What `ScrollWrappers::split` routes onto which wrapper.

use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::visibility::Visibility;
use crate::scene::node::Node;
use crate::scene::node::node_mode::NodeMode;
use crate::widget_core::configure::{Configure, ThemeDefaults};
use crate::widget_core::widget::Widget;
use crate::widgets::scroll::{Scroll, ScrollWrappers};
use glam::Vec2;

#[derive(Clone, Copy, Debug)]
enum Side {
    Outer,
    Inner,
}

/// Every `Configure` setter that writes the node lands on exactly one wrapper, with its exact value: sizing, placement and interaction on the outer box the caller sees; padding, clip and panel knobs on the inner viewport, where the children are. Each row's expectation is the bare split with only that field changed, so a field routed to both sides, the wrong side, or neither fails.
#[test]
fn split_routes_every_setter_to_one_wrapper() {
    type Row = (
        &'static str,
        fn(Scroll<'static>) -> Scroll<'static>,
        Side,
        fn(&mut Node),
    );
    let rows: [Row; 27] = [
        (
            "size",
            |s| s.size((Sizing::fixed(120.0), Sizing::HUG)),
            Side::Outer,
            |n| n.size = Some((Sizing::fixed(120.0), Sizing::HUG).into()),
        ),
        (
            "min_size",
            |s| s.min_size(Size::new(10.0, 20.0)),
            Side::Outer,
            |n| n.min_size = Some(Size::new(10.0, 20.0)),
        ),
        (
            "max_size",
            |s| s.max_size(Size::new(300.0, 400.0)),
            Side::Outer,
            |n| n.max_size = Some(Size::new(300.0, 400.0)),
        ),
        (
            "padding",
            |s| s.padding(Spacing::all(4.0)),
            Side::Inner,
            |n| n.padding = Some(Spacing::all(4.0)),
        ),
        (
            "margin",
            |s| s.margin(Spacing::all(-2.0)),
            Side::Outer,
            |n| n.margin = Some(Spacing::all(-2.0)),
        ),
        (
            "position",
            |s| s.position(Vec2::new(7.0, 9.0)),
            Side::Outer,
            |n| n.position = Vec2::new(7.0, 9.0),
        ),
        (
            "grid_cell",
            |s| s.grid_cell(GridCell::at(1, 2).with_span(2, 3)),
            Side::Outer,
            |n| n.grid = GridCell::at(1, 2).with_span(2, 3),
        ),
        // `adopt_placement` copies the donor's `Tab` order among its placement fields.
        (
            "adopt_placement",
            |s| s.adopt_placement(&Widget::leaf().tab_index(4)),
            Side::Outer,
            |n| n.tab_index = 4,
        ),
        ("gap", |s| s.gap(3.0), Side::Inner, |n| n.gaps.set_gap(3.0)),
        (
            "line_gap",
            |s| s.line_gap(5.0),
            Side::Inner,
            |n| n.gaps.set_line_gap(5.0),
        ),
        (
            "justify",
            |s| s.justify(Justify::SpaceBetween),
            Side::Inner,
            |n| n.justify = Justify::SpaceBetween,
        ),
        (
            "align",
            |s| s.align(Align::CENTER),
            Side::Outer,
            |n| n.align = Align::CENTER,
        ),
        (
            "child_align",
            |s| s.child_align(Align::CENTER),
            Side::Inner,
            |n| n.child_align = Align::CENTER,
        ),
        // The bare scroll senses `SCROLL`; `sense` replaces it, `add_sense` joins it.
        (
            "sense",
            |s| s.sense(Sense::CLICK),
            Side::Outer,
            |n| n.flags.set_sense(Sense::CLICK),
        ),
        (
            "add_sense",
            |s| s.add_sense(Sense::CLICK),
            Side::Outer,
            |n| n.flags.set_sense(Sense::SCROLL | Sense::CLICK),
        ),
        (
            "disabled",
            |s| s.disabled(true),
            Side::Outer,
            |n| n.flags.set_disabled(true),
        ),
        (
            "focusable",
            |s| s.focusable(true),
            Side::Outer,
            |n| n.flags.set_focusable(true),
        ),
        (
            "tab_stop",
            |s| s.tab_stop(false),
            Side::Outer,
            |n| n.flags.set_tab_stop(false),
        ),
        (
            "arrow_focus",
            |s| s.arrow_focus(Axis::Y),
            Side::Outer,
            |n| n.flags.set_arrow_focus(Some(Axis::Y)),
        ),
        (
            "tab_index",
            |s| s.tab_index(-3),
            Side::Outer,
            |n| n.tab_index = -3,
        ),
        (
            "input_scope",
            |s| s.input_scope(KeyFilter::ALL),
            Side::Outer,
            |n| n.flags.set_key_filter(KeyFilter::ALL),
        ),
        (
            "visibility",
            |s| s.visibility(Visibility::Hidden),
            Side::Outer,
            |n| n.visibility = Visibility::Hidden,
        ),
        (
            "hidden",
            |s| s.hidden(),
            Side::Outer,
            |n| n.visibility = Visibility::Hidden,
        ),
        (
            "collapsed",
            |s| s.collapsed(),
            Side::Outer,
            |n| n.visibility = Visibility::Collapsed,
        ),
        // The bare scroll clips `Rect`, the value `clip_rect` writes, so these rows move it off that.
        (
            "clip",
            |s| s.clip(ClipMode::None),
            Side::Inner,
            |n| n.clip = Some(ClipMode::None),
        ),
        (
            "clip_rounded",
            |s| s.clip_rounded(),
            Side::Inner,
            |n| n.clip = Some(ClipMode::Rounded),
        ),
        // A themed default writes the same field as the setter; one row proves it routes too.
        (
            "default_padding",
            |s| s.default_padding(Spacing::all(6.0)),
            Side::Inner,
            |n| n.padding = Some(Spacing::all(6.0)),
        ),
    ];

    let axes = ScrollAxes::VERTICAL;
    let bare = Scroll::vertical();
    let ScrollWrappers {
        outer: bare_outer,
        inner: bare_inner,
    } = ScrollWrappers::split(&bare.widget, axes);
    assert_eq!(bare_outer.node.mode, NodeMode::Resolved(LayoutMode::ZStack));
    assert_eq!(
        bare_inner.node.mode,
        NodeMode::Resolved(LayoutMode::Scroll(axes))
    );
    assert_eq!(bare_outer.node.size, None);
    assert_eq!(
        bare_inner.node.size,
        Some((Sizing::FILL, Sizing::FILL).into())
    );
    assert_eq!(bare_inner.node.clip, Some(ClipMode::Rect));
    assert_eq!(bare_outer.node.flags.sense(), Sense::SCROLL);

    for (setter, apply, side, expect) in rows {
        let scroll = apply(Scroll::vertical());
        let ScrollWrappers { outer, inner } = ScrollWrappers::split(&scroll.widget, axes);
        let mut want_outer = bare_outer.node;
        let mut want_inner = bare_inner.node;
        expect(match side {
            Side::Outer => &mut want_outer,
            Side::Inner => &mut want_inner,
        });
        assert!(
            want_outer != bare_outer.node || want_inner != bare_inner.node,
            "`{setter}` row expects the bare split",
        );
        assert_eq!(outer.node, want_outer, "`{setter}` on the outer wrapper");
        assert_eq!(inner.node, want_inner, "`{setter}` on the inner viewport");
    }
}

/// The inner pan is the transform of a scroll's descendants, so an authored one has no side.
#[test]
#[should_panic(expected = "a `Scroll` owns its descendants' transform")]
fn split_rejects_an_authored_transform() {
    let scroll = Scroll::vertical().transform(TranslateScale::from_scale(2.0));
    let _ = ScrollWrappers::split(&scroll.widget, scroll.axes);
}
