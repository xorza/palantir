//! Rules every chrome-bearing widget keeps alike, checked across all of
//! them because no trait holds them in step.

use crate::input::interaction::response_state::ResponseState;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::scene::layer::Layer;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::ResponseSnapshot;
use crate::widgets::block::Block;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::grid::Grid;
use crate::widgets::modal::Modal;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::scroll::Scroll;
use crate::widgets::tooltip::Tooltip;
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(400, 300);
const ID: &str = "chrome-default";

/// Which of the two chrome setters a case calls, and in which order.
#[derive(Clone, Copy, Debug)]
enum Chrome {
    DefaultOnly,
    ExplicitOnly,
    ExplicitThenDefault,
    DefaultThenExplicit,
}

const DEFAULT_FILL: RgbaF32 = RgbaF32::new(0.9, 0.1, 0.1, 1.0);
const EXPLICIT_FILL: RgbaF32 = RgbaF32::new(0.1, 0.1, 0.9, 1.0);

impl Chrome {
    /// Apply the case to `w` through the widget's own two setters.
    fn apply<W>(
        self,
        w: W,
        background: fn(W, Background) -> W,
        default: fn(W, Background) -> W,
    ) -> W {
        let d = Background::fill(DEFAULT_FILL);
        let e = Background::fill(EXPLICIT_FILL);
        match self {
            Self::DefaultOnly => default(w, d),
            Self::ExplicitOnly => background(w, e),
            Self::ExplicitThenDefault => default(background(w, e), d),
            Self::DefaultThenExplicit => background(default(w, d), e),
        }
    }

    /// The fill the case paints: the default only when nothing explicit
    /// was set, in either order.
    const fn want(self) -> RgbaF32 {
        match self {
            Self::DefaultOnly => DEFAULT_FILL,
            _ => EXPLICIT_FILL,
        }
    }
}

/// A trigger the pointer rests on, for the tooltip.
fn hovered_trigger() -> ResponseSnapshot {
    ResponseSnapshot {
        id: WidgetId::from_hash("chrome-trigger"),
        state: ResponseState {
            rect: Some(Rect::new(20.0, 20.0, 40.0, 20.0)),
            pointer_over: true,
            ..ResponseState::default()
        },
    }
}

/// One widget kind: how to record it under a case, and the id and layer
/// of the node its chrome lands on.
struct Kind {
    name: &'static str,
    record: fn(&mut Ui, Chrome),
    chrome_node: fn() -> WidgetId,
    layer: Layer,
}

fn own_id() -> WidgetId {
    WidgetId::from_hash(ID)
}

/// Every widget with `background(bg)` has `default_background(bg)`
/// beside it, and the two resolve alike on all of them: the default fills
/// in only where the caller set no background, in either call order.
#[test]
fn default_background_yields_to_an_explicit_one_on_every_chrome_widget() {
    let kinds = [
        Kind {
            name: "Block",
            record: |ui, c| {
                c.apply(
                    Block::new().id(own_id()),
                    Block::background,
                    Block::default_background,
                )
                .show(ui);
            },
            chrome_node: own_id,
            layer: Layer::Main,
        },
        Kind {
            name: "Panel",
            record: |ui, c| {
                c.apply(
                    Panel::vstack().id(own_id()),
                    Panel::background,
                    Panel::default_background,
                )
                .show(ui, |_| {});
            },
            chrome_node: own_id,
            layer: Layer::Main,
        },
        Kind {
            name: "Grid",
            record: |ui, c| {
                c.apply(
                    Grid::new().id(own_id()),
                    Grid::background,
                    Grid::default_background,
                )
                .show(ui, |_| {});
            },
            chrome_node: own_id,
            layer: Layer::Main,
        },
        Kind {
            name: "Scroll",
            record: |ui, c| {
                c.apply(
                    Scroll::vertical().id(own_id()),
                    Scroll::background,
                    Scroll::default_background,
                )
                .show(ui, |_| {});
            },
            chrome_node: || own_id().with("viewport"),
            layer: Layer::Main,
        },
        Kind {
            name: "Popup",
            record: |ui, c| {
                c.apply(
                    Popup::at_point(Vec2::new(20.0, 20.0)).id(own_id()),
                    Popup::background,
                    Popup::default_background,
                )
                .show(ui, |_, _| {});
            },
            chrome_node: own_id,
            layer: Layer::Popup,
        },
        Kind {
            name: "Modal",
            record: |ui, c| {
                c.apply(
                    Modal::new().id(own_id()),
                    Modal::background,
                    Modal::default_background,
                )
                .show(ui, |_, _| {});
            },
            chrome_node: || own_id().with("panel"),
            layer: Layer::Modal,
        },
        Kind {
            name: "Tooltip",
            record: |ui, c| {
                let trigger = hovered_trigger();
                c.apply(
                    Tooltip::on(&trigger, "tip").delay(Duration::ZERO),
                    Tooltip::background,
                    Tooltip::default_background,
                )
                .show(ui);
            },
            chrome_node: || hovered_trigger().id.with("bubble"),
            layer: Layer::Tooltip,
        },
        Kind {
            name: "ContextMenu",
            record: |ui, c| {
                c.apply(
                    ContextMenu::for_id(own_id()),
                    ContextMenu::background,
                    ContextMenu::default_background,
                )
                .show(ui, |_, _| {});
            },
            chrome_node: || own_id().with("body"),
            layer: Layer::Menu,
        },
    ];
    let cases = [
        Chrome::DefaultOnly,
        Chrome::ExplicitOnly,
        Chrome::ExplicitThenDefault,
        Chrome::DefaultThenExplicit,
    ];
    for kind in &kinds {
        for case in cases {
            let mut h = UiHarness::new(SURFACE);
            ContextMenu::open(&mut h.ui, own_id(), Vec2::new(20.0, 20.0));
            // Two frames: the tooltip turns visible on the frame after the
            // pointer settles on its trigger.
            h.frame(|ui| (kind.record)(ui, case));
            h.frame(|ui| (kind.record)(ui, case));
            let node = h
                .node_of((kind.chrome_node)())
                .unwrap_or_else(|| panic!("{}: chrome node recorded", kind.name));
            assert_eq!(node.layer, kind.layer, "{}", kind.name);
            let fill =
                h.ui.tree(kind.layer)
                    .chrome(node.node)
                    .unwrap_or_else(|| panic!("{} {case:?}: paints chrome", kind.name))
                    .fill;
            let want = RgbaF16::from(case.want());
            assert!(
                matches!(fill, ShapeBrush::Solid(got) if got == want),
                "{} {case:?}: {fill:?}, want {want:?}",
                kind.name,
            );
        }
    }
}

/// A background is checked where it enters a widget, through either
/// setter: a NaN fill colour, a negative border width, a NaN corner and a
/// NaN shadow blur each panic with their kind's rule.
#[test]
fn chrome_setters_check_the_background() {
    use crate::internals::panic_probe;
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::math::domain;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;

    let bad: [(&str, Background); 4] = [
        (
            "a color must have finite channels",
            Background::fill(RgbaF32::new(f32::NAN, 0.0, 0.0, 1.0)),
        ),
        (
            domain::LENGTH_RULE,
            Background::fill(RgbaF32::WHITE).with_border(Stroke::new(RgbaF32::WHITE, -1.0)),
        ),
        (
            domain::LENGTH_RULE,
            Background::rounded(RgbaF32::WHITE, Corners::all(f32::NAN)),
        ),
        (
            domain::LENGTH_RULE,
            Background::fill(RgbaF32::WHITE).with_shadow(Shadow {
                blur: f32::NAN,
                ..Shadow::default()
            }),
        ),
    ];
    for (rule, bg) in bad {
        panic_probe::assert_panics_with(rule, || Panel::vstack().background(bg.clone()));
        panic_probe::assert_panics_with(rule, || Block::new().default_background(bg.clone()));
    }
}

/// Every widget setter this crate checks panics with its kind's rule on a
/// value outside it, and takes the boundary value.
#[test]
fn widget_setters_check_their_kinds() {
    use crate::internals::panic_probe;
    use crate::primitives::math::domain;
    use crate::primitives::paint::color::color_coords::ColorCoords;
    use crate::widgets::color_field::ColorField;
    use crate::widgets::drag_value::DragValue;
    use crate::widgets::separator::Separator;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::text::Text;

    const NAN_RED: RgbaF32 = RgbaF32::new(f32::NAN, 0.0, 0.0, 1.0);
    let length_cases: [fn(f32); 4] = [
        |v| drop(Spinner::new().diameter(v)),
        |v| drop(Spinner::new().thickness(v)),
        |v| drop(Separator::horizontal().thickness(v)),
        |v| drop(Text::new("t").font_size(v)),
    ];
    for set in length_cases {
        set(0.0);
        for bad in [f32::NAN, f32::INFINITY, -1.0] {
            panic_probe::assert_panics_with(domain::LENGTH_RULE, || set(bad));
        }
    }
    let color_cases: [fn(RgbaF32); 4] = [
        |c| drop(Spinner::new().color(c)),
        |c| drop(Separator::horizontal().color(c)),
        |c| drop(Text::new("t").color(c)),
        |c| drop(Modal::new().backdrop(c)),
    ];
    for set in color_cases {
        set(RgbaF32::new(2.0, 0.0, 0.0, 1.0));
        panic_probe::assert_panics_with("a color must have finite channels", || set(NAN_RED));
    }

    let mut coords = ColorCoords::default();
    let _ = ColorField::new(&mut coords).texel_size(16);
    for bad in [0, 3, 32] {
        panic_probe::assert_panics_with(domain::POWER_OF_TWO_RULE, || {
            let mut coords = ColorCoords::default();
            drop(ColorField::new(&mut coords).texel_size(bad));
        });
    }

    let mut value = 0.0_f64;
    let _ = DragValue::new(&mut value)
        .speed(f64::MIN_POSITIVE)
        .range(f64::NEG_INFINITY..=0.0);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        panic_probe::assert_panics_with(domain::POSITIVE_RULE, || {
            let mut value = 0.0_f64;
            drop(DragValue::new(&mut value).speed(bad));
        });
    }
    panic_probe::assert_panics_with("a drag range's ends must not be NaN", || {
        let mut value = 0.0_f64;
        drop(DragValue::new(&mut value).range(f64::NAN..=1.0));
    });
}
