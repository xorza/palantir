use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::layout_mode::GridDefId;
use crate::layout::types::layout_mode::PackedLayoutMeta;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::*;
use crate::scene::visibility::Visibility;
use crate::widgets::configure::Configure;
use crate::widgets::widget::Widget;

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
    assert_eq!(std::mem::size_of::<Node>(), 100);
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
fn unconfigured_and_explicit_default_values_remain_distinct() {
    let inherited = Widget::leaf();
    assert_eq!(inherited.node.size, None);
    assert_eq!(inherited.node.min_size, None);
    assert_eq!(inherited.node.max_size, None);
    assert_eq!(inherited.node.padding, None);
    assert_eq!(inherited.node.margin, None);
    assert_eq!(inherited.node.clip, None);

    let explicit = Widget::leaf()
        .size(Sizes::default())
        .min_size(Size::ZERO)
        .max_size(Size::INF)
        .padding(Spacing::ZERO)
        .margin(Spacing::ZERO)
        .disabled(false)
        .focusable(false)
        .visibility(Visibility::Visible)
        .clip(ClipMode::None);
    assert_eq!(explicit.node.size, Some(Sizes::default()));
    assert_eq!(explicit.node.min_size, Some(Size::ZERO));
    assert_eq!(explicit.node.max_size, Some(Size::INF));
    assert_eq!(explicit.node.padding, Some(Spacing::ZERO));
    assert_eq!(explicit.node.margin, Some(Spacing::ZERO));
    assert_eq!(explicit.node.clip, Some(ClipMode::None));

    // Explicitly-set defaults record identically to unset fields.
    let columns = explicit
        .node
        .columns(WidgetId::from_hash("explicit-defaults"));
    assert_eq!(columns.attrs, NodeFlags::default());
    assert_eq!(columns.bounds, BoundsExtras::DEFAULT);
}

/// `set_mode` refines a node; it never re-kinds one. A pending grid
/// takes only its own definition, and a resolved mode only a fresh
/// payload of its own kind — which is what lets one method serve both
/// the grid's deferred id and the scroll's deferred fit bits.
#[test]
fn set_mode_refines_a_node_and_never_rekinds_it() {
    let mut grid = Node::new(NodeMode::PendingGrid);
    assert!(std::panic::catch_unwind(|| LayoutCore::from_node(&grid)).is_err());
    let grid_id = GridDefId::from_index(42);
    grid.set_mode(LayoutMode::Grid(grid_id));
    assert_eq!(grid.mode, NodeMode::Resolved(LayoutMode::Grid(grid_id)));

    let mut refined = Node::new(NodeMode::Resolved(LayoutMode::Scroll(ScrollSpec::VERTICAL)));
    refined.set_mode(LayoutMode::Scroll(ScrollSpec::BOTH));
    assert_eq!(refined.scroll_spec(), ScrollSpec::BOTH);
    assert!(
        std::panic::catch_unwind(|| Node::new(NodeMode::PendingGrid).set_mode(LayoutMode::ZStack))
            .is_err(),
        "a pending grid takes only a grid definition",
    );
    assert!(
        std::panic::catch_unwind(|| {
            Node::new(NodeMode::Resolved(LayoutMode::Stack(Axis::Y)))
                .set_mode(LayoutMode::Grid(grid_id))
        })
        .is_err(),
        "a resolved mode is not re-kinded",
    );

    let last_grid = GridDefId::from_index(65_534);
    assert_eq!(usize::from(last_grid), 65_534);
    assert!(std::panic::catch_unwind(|| GridDefId::from_index(65_535)).is_err());
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
            LayoutMode::Stack(Axis::X),
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
            LayoutMode::WrapStack(Axis::X),
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::WrapStack(Axis::Y),
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
            LayoutMode::Stack(Axis::Y),
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

/// The theme fills in only where the caller stayed silent, and an
/// authored bound still faces its own check.
#[test]
fn an_authored_value_wins_over_the_theme_default() {
    let mut node = Node::new(NodeMode::Resolved(LayoutMode::Leaf));
    node.set_padding(Spacing::all(3.0));
    node.fill_padding(Spacing::all(9.0));
    assert_eq!(node.padding, Some(Spacing::all(3.0)), "explicit wins");

    let mut untouched = Node::new(NodeMode::Resolved(LayoutMode::Leaf));
    untouched.fill_padding(Spacing::all(9.0));
    assert_eq!(untouched.padding, Some(Spacing::all(9.0)), "theme fills in");
}

/// A themed lower bound is checked against an authored upper one, the
/// way an authored lower bound is.
#[test]
#[should_panic]
fn a_themed_min_size_is_bound_checked_against_an_authored_max() {
    let mut node = Node::new(NodeMode::Resolved(LayoutMode::Leaf));
    node.set_max_size(Size::new(10.0, 10.0));
    node.fill_min_size(Size::new(40.0, 40.0));
}
