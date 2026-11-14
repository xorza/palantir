//! The node setters and ids as each bundled widget hands them to its `Widget`, which `widget_core`'s own tests cannot name.

use crate::host::harness::Harness;
use crate::internals::panic_probe;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::pointer::PointerButton;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::packed::serde::LaneCodec;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::node::Node;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureDefaults;
use crate::widgets::menu::menu_item::MenuItem;
use crate::widgets::scroll::Scroll;
use crate::widgets::{block::Block, button::Button, grid::Grid, panel::Panel, text::Text};
use glam::{UVec2, Vec2};

fn node_of<W: Configure>(widget: &mut W) -> &mut Node {
    let Configurator { widget } = widget.configure();
    &mut widget.node
}

fn assert_distinct(label: &str, a: WidgetId, b: WidgetId) {
    assert_ne!(
        a, b,
        "{label}: two `.auto_id()` calls on different lines produced the same id — \
         `Configure::auto_id` is missing `#[track_caller]`."
    );
}

fn id_of<W: Configure>(mut w: W) -> WidgetId {
    // No parent context: `IdRecipe::raw_id(None)` yields the bare id.
    w.configure().widget.ident.recipe().raw_id(None)
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

    let mut scroll = Scroll::both().zoomable();
    assert_eq!(
        node_of(&mut scroll).flags.sense(),
        Sense::SCROLL | Sense::PINCH,
    );
}

/// Pin: [`Configure::auto_id`] is `#[track_caller]` and resolves a stable id at the call site: two calls on different lines give distinct `WidgetId`s. The cases cover every public widget constructor.
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
        ("Block", || {
            (id_of(Block::new().auto_id()), id_of(Block::new().auto_id()))
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

/// `id_salt(...)` overrides `auto_id`: the same key on different lines gives the same id.
#[test]
fn id_salt_overrides_auto_id() {
    assert_eq!(
        id_of(Button::new().id(WidgetId::from_hash("k"))),
        id_of(Button::new().id(WidgetId::from_hash("k"))),
    );
}

/// `Configure::auto_id()` re-derives the id at its call site: helper-internal calls collapse to one location, `.auto_id()` at the caller restores per-line ids.
#[test]
fn auto_id_redirects_to_call_site() {
    fn helper() -> Button<'static> {
        Button::new().auto_id()
    }
    // Both `helper()` calls resolve inside the helper body: same line, same id.
    assert_eq!(id_of(helper()), id_of(helper()));
    // With `.auto_id()` on different source lines, the ids diverge.
    let a = id_of(helper().auto_id());
    let b = id_of(helper().auto_id());
    assert_distinct("auto_id() at call site", a, b);
}

/// A themed fallback faces the same check as an authored value, in every build: padding edges are lengths, margin edges offsets. A non-finite edge never gets that far: `Spacing` refuses it where it is built.
#[test]
fn spacings_validate_their_kinds_authored_or_themed() {
    type Case = (&'static str, fn() -> Panel);
    let lane = <Spacing as LaneCodec>::LANE_RULE;
    let cases: &[Case] = &[
        (domain::LENGTH_RULE, || {
            Panel::vstack().default_padding(Spacing::all(-1.0))
        }),
        (domain::LENGTH_RULE, || {
            Panel::vstack().padding(Spacing::new(0.0, 0.0, 0.0, -2.0))
        }),
    ];
    for &(rule, build) in cases {
        panic_probe::assert_panics_with(rule, build);
    }
    let unbuilt: &[fn() -> Panel] = &[
        || Panel::vstack().default_padding(Spacing::all(f32::NAN)),
        || Panel::vstack().padding(Spacing::new(0.0, 0.0, 0.0, f32::INFINITY)),
        || Panel::vstack().margin(Spacing::new(f32::NEG_INFINITY, 0.0, 0.0, 0.0)),
    ];
    for &build in unbuilt {
        panic_probe::assert_panics_with(lane, build);
    }
    // A negative margin is an offset, so it is taken.
    let _ = Panel::vstack().margin(Spacing::all(-4.0));
}

/// A border wider than an f16 lane is a valid length, and the frame draws it: the padding it grows
/// saturates, as do the inner corners and the cover the renderer derives from it.
#[test]
fn a_border_past_an_f16_lane_draws() {
    let mut h = Harness::mono(UVec2::new(64, 48));
    h.frame(|ui| {
        Panel::vstack()
            .chrome(
                Chrome::rounded(RgbaF32::WHITE, Corners::all(4.0))
                    .with_border(Border::new(RgbaF32::BLACK, 70000.0)),
            )
            .show(ui, |_| {});
    });
}

/// `show` reads the response a widget that acts on input opens with: a sensing node disabled on
/// this frame, before the cascade knows it, reads disabled and takes no click, as a `Button` does.
#[test]
fn show_folds_a_nodes_own_disabled_into_its_response() {
    let id = WidgetId::from_hash("self-disabled");
    let mut h = Harness::mono(UVec2::new(200, 100));
    let record = |ui: &mut Ui, disabled: bool| {
        Panel::vstack()
            .id(id)
            .sense(Sense::CLICK)
            .disabled(disabled)
            .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
            .show(ui, |ui| {
                Text::new("child").auto_id().show(ui);
            })
            .response
    };
    h.frame(|ui| {
        record(ui, false);
    });
    h.press(Vec2::new(50.0, 25.0), PointerButton::Left);
    h.frame(|ui| {
        record(ui, false);
    });
    h.release(PointerButton::Left);
    let passes = h.frame_passes(|ui| record(ui, true));
    let pass_a = passes.a();
    assert!(
        pass_a.disabled,
        "the node's own flag, on the pass the cascade lags"
    );
    assert!(!pass_a.clicked(), "a disabled node takes no click");
    assert_eq!(passes.count_where(Response::clicked), 0);
}
