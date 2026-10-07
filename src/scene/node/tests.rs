use crate::input::sense::Sense;
use crate::internals::panic_probe;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::{GridDefId, ScrollbarsDefId};
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::scene::node::*;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

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
        assert!(f.is_tab_stop(), "case: {label}: a stop by default");
        f.set_focusable(*focusable);
        // The opposite of `focusable`, so every row tells the two bits apart.
        f.set_tab_stop(!*focusable);
        assert_eq!(f.sense(), *sense, "case: {label} sense");
        assert_eq!(f.is_disabled(), *disabled, "case: {label} disabled");
        assert_eq!(f.clip_mode(), *clip, "case: {label} clip");
        assert_eq!(f.is_focusable(), *focusable, "case: {label} focusable");
        assert_eq!(f.is_tab_stop(), !*focusable, "case: {label} tab stop");
    }
}

/// A node adopting another's placement takes its Tab-order stop bit and index, and none of its other flags.
#[test]
fn adopt_placement_carries_the_tab_order() {
    let from = Widget::leaf()
        .tab_stop(false)
        .tab_index(7)
        .sense(Sense::CLICK)
        .node;
    let mut to = Widget::leaf().focusable(true).node;
    to.adopt_placement(from);
    assert!(!to.flags.is_tab_stop());
    assert_eq!(to.tab_index, 7);
    assert!(to.flags.is_focusable(), "its own flags stay");
    assert_eq!(
        to.flags.sense(),
        Sense::NONE,
        "the source's sense does not travel"
    );
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
        .size(SizeSpec::default())
        .min_size(Size::ZERO)
        .max_size(Size::INF)
        .padding(Spacing::ZERO)
        .margin(Spacing::ZERO)
        .disabled(false)
        .focusable(false)
        .visibility(Visibility::Visible)
        .clip(ClipMode::None);
    assert_eq!(explicit.node.size, Some(SizeSpec::default()));
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

/// `set_mode` refines a node, never re-kinds it: a pending grid or bar overlay takes only its own definition, a resolved mode only a fresh payload of its own kind.
#[test]
fn set_mode_refines_a_node_and_never_rekinds_it() {
    let mut grid = Node::new(NodeMode::PendingGrid);
    panic_probe::assert_panics_with(
        "grid node recorded before its definition was installed",
        || LayoutCore::from_node(&grid),
    );
    let grid_id = GridDefId::from_index(42);
    grid.set_mode(LayoutMode::Grid(grid_id));
    assert_eq!(grid.mode, NodeMode::Resolved(LayoutMode::Grid(grid_id)));

    let mut bars = Node::new(NodeMode::PendingScrollbars);
    panic_probe::assert_panics_with(
        "scrollbar overlay recorded before its definition was installed",
        || LayoutCore::from_node(&bars),
    );
    let bars_id = ScrollbarsDefId::from_index(7);
    bars.set_mode(LayoutMode::Scrollbars(bars_id));
    assert_eq!(
        bars.mode,
        NodeMode::Resolved(LayoutMode::Scrollbars(bars_id))
    );

    let mut refined = Node::new(NodeMode::Resolved(LayoutMode::Scroll(ScrollAxes::VERTICAL)));
    refined.set_mode(LayoutMode::Scroll(ScrollAxes::BOTH));
    assert_eq!(
        refined.mode,
        NodeMode::Resolved(LayoutMode::Scroll(ScrollAxes::BOTH))
    );
    // A pending grid takes only a grid definition.
    panic_probe::assert_panics_with("ZStack installed on a PendingGrid node", || {
        Node::new(NodeMode::PendingGrid).set_mode(LayoutMode::ZStack);
    });
    // A pending bar overlay takes only a bar definition.
    panic_probe::assert_panics_with("installed on a PendingScrollbars node", || {
        Node::new(NodeMode::PendingScrollbars).set_mode(LayoutMode::Grid(grid_id));
    });
    // A resolved mode is not re-kinded.
    panic_probe::assert_panics_with("installed on a Resolved(Stack(Y)) node", || {
        Node::new(NodeMode::Resolved(LayoutMode::Stack(Axis::Y)))
            .set_mode(LayoutMode::Grid(grid_id));
    });

    let last_grid = GridDefId::from_index(65_534);
    assert_eq!(usize::from(last_grid), 65_534);
    panic_probe::assert_panics_with("exceeded its 65535 row ceiling", || {
        GridDefId::from_index(65_535)
    });
}

#[test]
fn layout_core_round_trips_mode_align_visibility() {
    use crate::primitives::layout::align::{Align, HAlign, VAlign};
    use crate::primitives::layout::visibility::Visibility;
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
            LayoutMode::Scroll(ScrollAxes::VERTICAL),
            Align::new(HAlign::Center, VAlign::Top),
            Visibility::Visible,
        ),
        (
            LayoutMode::Scroll(ScrollAxes::HORIZONTAL),
            Align::new(HAlign::Auto, VAlign::Auto),
            Visibility::Visible,
        ),
        (
            LayoutMode::Scroll(ScrollAxes::BOTH),
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

/// The theme fills in only where the caller stayed silent; an authored bound still faces its own check.
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

/// A themed default never contradicts the caller: a default min above an authored max is clamped to it, a default max below an authored min is raised to it, per axis. Conflicting authored bounds resolve as in CSS (min wins), and a NaN default still reaches the check.
#[test]
fn themed_bounds_yield_to_authored_ones() {
    let leaf = || Node::new(NodeMode::Resolved(LayoutMode::Leaf));

    // Modal: stock min width 280 under an authored max of 240.
    let mut node = leaf();
    node.set_max_size(Size::new(240.0, 400.0));
    node.fill_min_size(Size::new(280.0, 0.0));
    assert_eq!(node.min_size, Some(Size::new(240.0, 0.0)));

    // Tooltip: stock max 280×∞ under an authored min width of 300.
    let mut node = leaf();
    node.set_min_size(Size::new(300.0, 0.0));
    node.fill_max_size(Size::new(280.0, f32::INFINITY));
    assert_eq!(node.max_size, Some(Size::new(300.0, f32::INFINITY)));

    // A default inside the authored bound is taken as it is.
    let mut node = leaf();
    node.set_max_size(Size::new(500.0, 500.0));
    node.fill_min_size(Size::new(280.0, 10.0));
    assert_eq!(node.min_size, Some(Size::new(280.0, 10.0)));

    let mut node = leaf();
    node.set_max_size(Size::new(240.0, 400.0));
    node.set_min_size(Size::new(280.0, 0.0));
    assert_eq!(
        node.max_size,
        Some(Size::new(280.0, 400.0)),
        "the minimum wins"
    );

    panic_probe::assert_panics_with(domain::LENGTH_RULE, || {
        let mut node = leaf();
        node.set_max_size(Size::new(240.0, 400.0));
        node.fill_min_size(Size::new(f32::NAN, 0.0));
    });
}
