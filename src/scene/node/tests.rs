use crate::layout::types::layout_mode::PackedLayoutMeta;
use crate::layout::types::limits::MAX_PACKED_GAP;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::*;
use crate::scene::visibility::Visibility;
use crate::widgets::context_menu::MenuItem;
use crate::widgets::drag_value::DragValue;
use crate::widgets::scroll::Scroll;
use crate::widgets::{button::Button, frame::Frame, grid::Grid, panel::Panel, text::Text};

fn node_of<W: Configure>(widget: &mut W) -> &mut Node {
    widget.node_mut().node
}

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
fn authoring_struct_sizes_stay_packed() {
    // Grew from 1 byte to 2 when `Sense::PINCH` claimed bit 4,
    // pushing `DISABLED`/`CLIP`/`FOCUSABLE` past the u8 ceiling.
    // Still packed — sense (5 bits) + disabled (1) + clip (2) +
    // focusable (1) = 9 bits, fitting in a u16 with 7 spare.
    assert_eq!(std::mem::size_of::<NodeFlags>(), 2);
    assert_eq!(std::mem::size_of::<Node>(), 120);
}

#[test]
fn layout_core_size() {
    assert_eq!(std::mem::size_of::<LayoutCore>(), 28);
}

#[test]
fn layout_mode_size() {
    assert_eq!(std::mem::size_of::<LayoutMode>(), 4);
    assert_eq!(std::mem::size_of::<PackedLayoutMeta>(), 4);
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

    let mut node = Node::hstack()
        .id(id)
        .size(size)
        .min_size(min_size)
        .max_size(max_size)
        .padding(padding)
        .margin(margin)
        .position(position)
        .grid_cell((2, 3))
        .grid_span((4, 5))
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
    node.transform = transform;

    assert!(matches!(node.salt, Salt::Verbatim(value) if value == id));
    assert_eq!(node.size, Some(size));
    assert_eq!(node.min_size, Some(min_size));
    assert_eq!(node.max_size, Some(max_size));
    assert_eq!(node.padding, Some(padding));
    assert_eq!(node.margin, Some(margin));
    assert_eq!(node.position, position);
    assert_eq!(
        node.grid,
        GridCell {
            row: 2,
            col: 3,
            row_span: 4,
            col_span: 5,
        },
    );
    assert_eq!(node.gaps.gap(), 6.0);
    assert_eq!(node.gaps.line_gap(), 7.0);
    assert_eq!(node.justify, Justify::SpaceBetween);
    assert_eq!(node.align, align);
    assert_eq!(node.child_align, child_align);
    assert_eq!(node.flags.sense(), sense);
    assert!(!node.flags.is_disabled());
    assert!(node.flags.is_focusable());
    assert_eq!(node.visibility, Visibility::Hidden);
    assert_eq!(node.clip, Some(ClipMode::None));
    assert_eq!(node.transform, transform);
}

#[test]
fn widget_specific_node_setters_reach_the_inner_node() {
    let transform = TranslateScale::new(Vec2::new(4.0, 5.0), 2.0);
    let mut panel = Panel::hstack().transform(transform);
    let mut grid = Grid::new().transform(transform);
    assert_eq!(node_of(&mut panel).transform, transform);
    assert_eq!(node_of(&mut grid).transform, transform);

    let mut item = MenuItem::new("Open").enabled(true);
    assert!(!node_of(&mut item).flags.is_disabled());

    let mut value = 0.0;
    let mut drag = DragValue::new(&mut value).editable(true);
    assert_eq!(node_of(&mut drag).flags.sense(), Sense::CLICK | Sense::DRAG,);

    let mut scroll = Scroll::both().with_zoom();
    assert_eq!(
        node_of(&mut scroll).flags.sense(),
        Sense::SCROLL | Sense::PINCH,
    );
}

#[test]
fn unconfigured_and_explicit_default_values_remain_distinct() {
    let inherited = Node::leaf();
    assert_eq!(inherited.size, None);
    assert_eq!(inherited.min_size, None);
    assert_eq!(inherited.max_size, None);
    assert_eq!(inherited.padding, None);
    assert_eq!(inherited.margin, None);
    assert_eq!(inherited.clip, None);

    let explicit = Node::leaf()
        .size(Sizes::default())
        .min_size(Size::ZERO)
        .max_size(Size::INF)
        .padding(Spacing::ZERO)
        .margin(Spacing::ZERO)
        .disabled(false)
        .focusable(false)
        .visibility(Visibility::Visible)
        .clip(ClipMode::None);
    assert_eq!(explicit.size, Some(Sizes::default()));
    assert_eq!(explicit.min_size, Some(Size::ZERO));
    assert_eq!(explicit.max_size, Some(Size::INF));
    assert_eq!(explicit.padding, Some(Spacing::ZERO));
    assert_eq!(explicit.margin, Some(Spacing::ZERO));
    assert_eq!(explicit.clip, Some(ClipMode::None));

    // Explicitly-set defaults record identically to unset fields.
    let columns = explicit.into_columns(WidgetId::from_hash("explicit-defaults"));
    assert_eq!(columns.attrs, NodeFlags::default());
    assert_eq!(columns.bounds, BoundsExtras::DEFAULT);
}

#[test]
fn constructors_install_layout_modes() {
    let cases = [
        (Node::leaf(), LayoutMode::Leaf),
        (Node::hstack(), LayoutMode::HStack),
        (Node::vstack(), LayoutMode::VStack),
        (Node::wrap_hstack(), LayoutMode::WrapHStack),
        (Node::wrap_vstack(), LayoutMode::WrapVStack),
        (Node::zstack(), LayoutMode::ZStack),
        (Node::canvas(), LayoutMode::Canvas),
    ];

    for (node, expected) in cases {
        assert_eq!(node.mode, NodeMode::Resolved(expected));
    }

    let mut grid = Node::grid();
    assert_eq!(grid.mode, NodeMode::PendingGrid);
    assert!(std::panic::catch_unwind(|| LayoutCore::from_node(&grid)).is_err());
    let grid_id = GridDefId::from_index(42);
    grid.set_grid_def(grid_id);
    assert_eq!(grid.mode, NodeMode::Resolved(LayoutMode::Grid(grid_id)));

    let last_grid = GridDefId::from_index(65_534);
    assert_eq!(usize::from(last_grid), 65_534);
    assert!(std::panic::catch_unwind(|| GridDefId::from_index(65_535)).is_err());

    let scroll = Node::scroll(ScrollSpec::VERTICAL);
    assert_eq!(
        scroll.mode,
        NodeMode::Resolved(LayoutMode::Scroll(ScrollSpec::VERTICAL)),
    );
    assert_eq!(scroll.scroll_spec(), ScrollSpec::VERTICAL);
}

#[test]
fn layout_core_round_trips_mode_align_visibility() {
    use crate::layout::types::align::{Align, HAlign, VAlign};
    use crate::scene::visibility::Visibility;
    let cases: &[(LayoutMode, Align, Visibility)] = &[
        (
            LayoutMode::Leaf,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::HStack,
            Align::new(HAlign::Left, VAlign::Center),
            Visibility::Hidden,
        ),
        (
            LayoutMode::Grid(GridDefId::from_index(42)),
            Align::new(HAlign::Right, VAlign::Bottom),
            Visibility::Collapsed,
        ),
        (
            LayoutMode::Scroll(ScrollSpec::VERTICAL),
            Align::new(HAlign::Center, VAlign::Top),
            Visibility::Visible,
        ),
        (
            LayoutMode::Scroll(ScrollSpec::HORIZONTAL),
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::Scroll(ScrollSpec::BOTH),
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Hidden,
        ),
        (
            LayoutMode::WrapHStack,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::WrapVStack,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::ZStack,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::Canvas,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::VStack,
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
    ];
    for &(mode, align, vis) in cases {
        let mut node = Node::new(NodeMode::Resolved(mode));
        node.align = align;
        node.visibility = vis;
        let core = LayoutCore::from_node(&node);
        assert_eq!(
            LayoutMode::from(core.meta),
            mode,
            "mode for {mode:?}/{align:?}/{vis:?}",
        );
        assert_eq!(
            core.meta.align(),
            align,
            "align for {mode:?}/{align:?}/{vis:?}",
        );
        assert_eq!(
            core.meta.visibility(),
            vis,
            "visibility for {mode:?}/{align:?}/{vis:?}"
        );
    }
}

#[test]
fn node_bounds_accept_ordered_ranges_and_equal_axis_boundaries() {
    let min_then_max = Node::leaf().min_size((10.0, 20.0)).max_size((10.0, 30.0));
    assert_eq!(min_then_max.min_size, Some(Size::new(10.0, 20.0)));
    assert_eq!(min_then_max.max_size, Some(Size::new(10.0, 30.0)));

    let max_then_min = Node::leaf().max_size((30.0, 20.0)).min_size((10.0, 20.0));
    assert_eq!(max_then_min.min_size, Some(Size::new(10.0, 20.0)));
    assert_eq!(max_then_min.max_size, Some(Size::new(30.0, 20.0)));

    let unbounded = Node::leaf().max_size(Size::INF);
    assert_eq!(unbounded.max_size, Some(Size::INF));
}

#[test]
#[cfg(debug_assertions)]
fn node_bounds_reject_inversions_on_each_axis_and_setter_order() {
    type Case = (&'static str, fn() -> Node);

    let cases: &[Case] = &[
        ("minimum exceeds existing x maximum", || {
            Node::leaf()
                .max_size((10.0, f32::INFINITY))
                .min_size((11.0, 0.0))
        }),
        ("minimum exceeds existing y maximum", || {
            Node::leaf()
                .max_size((f32::INFINITY, 10.0))
                .min_size((0.0, 11.0))
        }),
        ("maximum is below existing x minimum", || {
            Node::leaf()
                .min_size((11.0, 0.0))
                .max_size((10.0, f32::INFINITY))
        }),
        ("maximum is below existing y minimum", || {
            Node::leaf()
                .min_size((0.0, 11.0))
                .max_size((f32::INFINITY, 10.0))
        }),
        ("infinite x minimum", || {
            Node::leaf().min_size((f32::INFINITY, 0.0))
        }),
        ("infinite y minimum", || {
            Node::leaf().min_size((0.0, f32::INFINITY))
        }),
        ("NaN minimum", || Node::leaf().min_size((f32::NAN, 0.0))),
        ("negative infinite maximum", || {
            Node::leaf().max_size((f32::NEG_INFINITY, f32::INFINITY))
        }),
        ("NaN maximum", || {
            Node::leaf().max_size((f32::INFINITY, f32::NAN))
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
    let valid = Node::hstack().gap(MAX_PACKED_GAP).line_gap(MAX_PACKED_GAP);
    assert_eq!(valid.gaps.gap(), MAX_PACKED_GAP);
    assert_eq!(valid.gaps.line_gap(), MAX_PACKED_GAP);

    type Case = (&'static str, fn() -> Node);
    let cases: &[Case] = &[
        ("negative gap", || Node::hstack().gap(-1.0)),
        ("NaN gap", || Node::hstack().gap(f32::NAN)),
        ("positive infinite gap", || {
            Node::hstack().gap(f32::INFINITY)
        }),
        ("negative infinite gap", || {
            Node::hstack().gap(f32::NEG_INFINITY)
        }),
        ("f16-overflow gap", || {
            Node::hstack().gap(MAX_PACKED_GAP + 1.0)
        }),
        ("negative line gap", || Node::wrap_hstack().line_gap(-1.0)),
        ("NaN line gap", || Node::wrap_hstack().line_gap(f32::NAN)),
        ("positive infinite line gap", || {
            Node::wrap_hstack().line_gap(f32::INFINITY)
        }),
        ("negative infinite line gap", || {
            Node::wrap_hstack().line_gap(f32::NEG_INFINITY)
        }),
        ("f16-overflow line gap", || {
            Node::wrap_hstack().line_gap(MAX_PACKED_GAP + 1.0)
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
    node_of(&mut w).salt.resolve(None)
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
