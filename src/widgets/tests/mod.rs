//! Rules the bundled widgets keep alike, as no trait holds them in step: chrome defaults here,
//! `style` setters, and what a hostile theme file renders as.

mod style_override;
mod theme_file_values;

use crate::input::interaction::response_state::ResponseState;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
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
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::scroll::Scroll;
use crate::widgets::tooltip::Tooltip;
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(400, 300);
const ID: &str = "chrome-default";

#[derive(Clone, Copy, Debug)]
enum SetterOrder {
    DefaultOnly,
    ExplicitOnly,
    ExplicitThenDefault,
    DefaultThenExplicit,
}

const DEFAULT_FILL: RgbaF32 = RgbaF32::new(0.9, 0.1, 0.1, 1.0);
const EXPLICIT_FILL: RgbaF32 = RgbaF32::new(0.1, 0.1, 0.9, 1.0);

impl SetterOrder {
    fn apply<W>(self, w: W, chrome: fn(W, Chrome) -> W, default: fn(W, Chrome) -> W) -> W {
        let d = Chrome::fill(DEFAULT_FILL);
        let e = Chrome::fill(EXPLICIT_FILL);
        match self {
            Self::DefaultOnly => default(w, d),
            Self::ExplicitOnly => chrome(w, e),
            Self::ExplicitThenDefault => default(chrome(w, e), d),
            Self::DefaultThenExplicit => chrome(default(w, d), e),
        }
    }

    const fn want(self) -> RgbaF32 {
        match self {
            Self::DefaultOnly => DEFAULT_FILL,
            _ => EXPLICIT_FILL,
        }
    }
}

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

#[derive(Debug)]
struct Kind {
    name: &'static str,
    record: fn(&mut Ui, SetterOrder),
    chrome_node: fn() -> WidgetId,
    layer: Layer,
}

fn own_id() -> WidgetId {
    WidgetId::from_hash(ID)
}

/// Every widget with `chrome(chrome)` has `default_chrome(chrome)`; the default fills in only
/// where the caller set no chrome, in either call order.
#[test]
fn default_chrome_yields_to_an_explicit_one_on_every_chrome_widget() {
    let kinds = [
        Kind {
            name: "Block",
            record: |ui, c| {
                c.apply(
                    Block::new().id(own_id()),
                    Block::chrome,
                    Block::default_chrome,
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
                    Panel::chrome,
                    Panel::default_chrome,
                )
                .show(ui, |_| {});
            },
            chrome_node: own_id,
            layer: Layer::Main,
        },
        Kind {
            name: "Grid",
            record: |ui, c| {
                c.apply(Grid::new().id(own_id()), Grid::chrome, Grid::default_chrome)
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
                    Scroll::chrome,
                    Scroll::default_chrome,
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
                    Popup::new(Anchor::at_point(Vec2::new(20.0, 20.0))).id(own_id()),
                    Popup::chrome,
                    Popup::default_chrome,
                )
                .show(ui, |_, _| {});
            },
            chrome_node: own_id,
            layer: Layer::Popup,
        },
        Kind {
            name: "PopupTrigger",
            record: |ui, c| {
                let trigger = hovered_trigger();
                PopupTrigger::open(ui, trigger.id);
                c.apply(
                    PopupTrigger::on(&trigger),
                    PopupTrigger::chrome,
                    PopupTrigger::default_chrome,
                )
                .show(ui, |_, _| {});
            },
            chrome_node: || hovered_trigger().id.with("popup"),
            layer: Layer::Popup,
        },
        Kind {
            name: "Modal",
            record: |ui, c| {
                c.apply(
                    Modal::new().id(own_id()),
                    Modal::chrome,
                    Modal::default_chrome,
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
                    Tooltip::chrome,
                    Tooltip::default_chrome,
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
                    ContextMenu::chrome,
                    ContextMenu::default_chrome,
                )
                .show(ui, |_, _| {});
            },
            chrome_node: || own_id().with("body"),
            layer: Layer::Menu,
        },
    ];
    let cases = [
        SetterOrder::DefaultOnly,
        SetterOrder::ExplicitOnly,
        SetterOrder::ExplicitThenDefault,
        SetterOrder::DefaultThenExplicit,
    ];
    for kind in &kinds {
        for case in cases {
            let mut h = UiHarness::new(SURFACE);
            ContextMenu::open(&mut h.ui, own_id(), Vec2::new(20.0, 20.0));
            // Two frames: the tooltip turns visible the frame after the pointer settles.
            h.frame(|ui| (kind.record)(ui, case));
            h.frame(|ui| (kind.record)(ui, case));
            let node = h
                .node_of((kind.chrome_node)())
                .unwrap_or_else(|| panic!("{}: chrome node recorded", kind.name));
            assert_eq!(node.plane.layer, kind.layer, "{}", kind.name);
            let fill =
                h.ui.tree(kind.layer)
                    .chrome(node.node)
                    .unwrap_or_else(|| panic!("{} {case:?}: paints chrome", kind.name))
                    .fill;
            let want = case.want().to_f16();
            assert!(
                matches!(fill, ShapeBrush::Solid(got) if got == want),
                "{} {case:?}: {fill:?}, want {want:?}",
                kind.name,
            );
        }
    }
}

/// Chrome values are checked where they enter a widget through either setter: NaN fill, negative border
/// width, NaN or f16-overflowing corner and NaN shadow blur each panic with their kind's rule.
#[test]
fn chrome_setters_check_the_chrome() {
    use crate::internals::panic_probe;
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::math::domain;
    use crate::primitives::packed::serde::LaneCodec;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;

    let bad: [(&str, Chrome); 6] = [
        (
            domain::COLOR_RULE,
            Chrome::fill(RgbaF32::new(f32::NAN, 0.0, 0.0, 1.0)),
        ),
        (
            domain::LENGTH_RULE,
            Chrome::fill(RgbaF32::WHITE).with_border(Stroke::new(RgbaF32::WHITE, -1.0)),
        ),
        (
            <Corners as LaneCodec>::LANE_RULE,
            Chrome::rounded(RgbaF32::WHITE, Corners::all(f32::NAN)),
        ),
        (
            <Corners as LaneCodec>::LANE_RULE,
            Chrome::rounded(RgbaF32::WHITE, Corners::all(1.0e5)),
        ),
        (
            domain::LENGTH_RULE,
            Chrome::fill(RgbaF32::WHITE).with_shadow(Shadow {
                blur: f32::NAN,
                ..Shadow::default()
            }),
        ),
        (
            domain::LANE_RULE,
            Chrome::fill(RgbaF32::WHITE).with_shadow(Shadow {
                spread: 65520.0,
                ..Shadow::default()
            }),
        ),
    ];
    for (rule, chrome) in bad {
        panic_probe::assert_panics_with(rule, || Panel::vstack().chrome(chrome.clone()));
        panic_probe::assert_panics_with(rule, || Block::new().default_chrome(chrome.clone()));
    }
}

/// Every widget setter this crate checks panics with its kind's rule on a
/// value outside it, and takes the boundary value.
#[test]
fn widget_setters_check_their_kinds() {
    use crate::internals::panic_probe;
    use crate::primitives::geometry::spacing::Spacing;
    use crate::primitives::geometry::translate_scale::TranslateScale;
    use crate::primitives::layout::anchor::Anchor;
    use crate::primitives::math::domain;
    use crate::primitives::paint::color::color_coords::ColorCoords;
    use crate::scene::layer::Layer;
    use crate::widgets::color_field::ColorField;
    use crate::widgets::color_strip::ColorStrip;
    use crate::widgets::dock::dock_state::DockState;
    use crate::widgets::dock::dock_view::DockView;
    use crate::widgets::drag_value::DragValue;
    use crate::widgets::separator::Separator;
    use crate::widgets::slider::Slider;
    use crate::widgets::spinner::Spinner;
    use crate::widgets::splitter::Splitter;
    use crate::widgets::text::Text;
    use crate::widgets::text_edit::TextEdit;

    const NAN_RED: RgbaF32 = RgbaF32::new(f32::NAN, 0.0, 0.0, 1.0);
    let length_cases: [fn(f32); 5] = [
        |v| drop(Spinner::new().diameter(v)),
        |v| drop(Spinner::new().width(v)),
        |v| drop(Separator::horizontal().thickness(v)),
        |v| drop(Text::new("t").font_size(v)),
        |v| {
            let mut buf = String::new();
            drop(TextEdit::new(&mut buf).font_size(v));
        },
    ];
    for set in length_cases {
        set(0.0);
        for bad in [f32::NAN, f32::INFINITY, -1.0] {
            panic_probe::assert_panics_with(domain::LENGTH_RULE, || set(bad));
        }
    }
    let positive_cases: [fn(f32); 2] = [
        |v| drop(Text::new("t").line_height_factor(v)),
        |v| {
            let mut buf = String::new();
            drop(TextEdit::new(&mut buf).line_height_factor(v));
        },
    ];
    for set in positive_cases {
        set(f32::MIN_POSITIVE);
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            panic_probe::assert_panics_with(domain::POSITIVE_RULE, || set(bad));
        }
    }
    let color_cases: [fn(RgbaF32); 5] = [
        |c| drop(Spinner::new().color(c)),
        |c| drop(Separator::horizontal().color(c)),
        |c| drop(Text::new("t").color(c)),
        |c| drop(Modal::new().backdrop(c)),
        |c| {
            let mut buf = String::new();
            drop(TextEdit::new(&mut buf).color(c));
        },
    ];
    for set in color_cases {
        set(RgbaF32::new(2.0, 0.0, 0.0, 1.0));
        panic_probe::assert_panics_with(domain::COLOR_RULE, || set(NAN_RED));
    }

    let texel_cases: [fn(u32); 2] = [
        |n| {
            let mut coords = ColorCoords::default();
            drop(ColorField::new(&mut coords).texel_size(n));
        },
        |n| {
            let mut coords = ColorCoords::default();
            drop(ColorStrip::for_hue(&mut coords).texel_size(n));
        },
    ];
    for set in texel_cases {
        set(16);
        for bad in [0, 3, 32] {
            panic_probe::assert_panics_with(domain::POWER_OF_TWO_RULE, || set(bad));
        }
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

    let mut ratio = 0.5_f32;
    let _ = Splitter::row(&mut ratio).min_pane(0.0);
    let length: [fn(); 4] = [
        || {
            let mut ratio = 0.5_f32;
            drop(Splitter::row(&mut ratio).min_pane(-1.0));
        },
        || {
            let state = DockState::new("kinds.dock", 0_u32);
            let mut operations = Vec::new();
            drop(DockView::new(&state, &mut operations).min_pane(f32::NAN));
        },
        || drop(Scroll::both().content_margin(Spacing::new(0.0, -1.0, 0.0, 0.0))),
        || {
            let _ = Anchor::below(Rect::ZERO).with_gap(f32::INFINITY);
        },
    ];
    for set in length {
        panic_probe::assert_panics_with(domain::LENGTH_RULE, set);
    }
    drop(DockState::new("kinds.dock", 0_u32).with_edge_fraction(0.5));
    for bad in [-0.1, 0.6, f32::NAN] {
        panic_probe::assert_panics_with("edge_fraction must be in 0..=0.5", || {
            drop(DockState::new("kinds.dock", 0_u32).with_edge_fraction(bad));
        });
    }
    let offset: [fn(); 5] = [
        || drop(Scroll::both().pan_by(Vec2::new(f32::NAN, 0.0))),
        || {
            let mut h = UiHarness::new(SURFACE);
            Scroll::set_offset(
                &mut h.ui,
                WidgetId::from_hash("s"),
                Vec2::new(0.0, f32::NAN),
            );
        },
        || {
            let _ = Anchor::at_point(Vec2::new(0.0, f32::INFINITY));
        },
        || {
            let _ = Anchor::below(Rect::new(f32::NAN, 0.0, 4.0, 4.0));
        },
        || {
            let mut h = UiHarness::new(SURFACE);
            ContextMenu::open(
                &mut h.ui,
                WidgetId::from_hash("m"),
                Vec2::new(f32::NAN, 0.0),
            );
        },
    ];
    for set in offset {
        panic_probe::assert_panics_with(domain::OFFSET_RULE, set);
    }
    panic_probe::assert_panics_with("a `Scroll` owns its descendants' transform", || {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            Scroll::vertical()
                .transform(TranslateScale::from_scale(2.0))
                .show(ui, |_| {});
        });
    });
    panic_probe::assert_panics_with(domain::OFFSET_RULE, || {
        let mut h = UiHarness::new(SURFACE);
        h.ui.layer(Layer::Popup)
            .fixed_at(Vec2::new(f32::NAN, 0.0))
            .show(|_| {});
    });
    panic_probe::assert_panics_with(domain::EXTENT_RULE, || {
        let mut h = UiHarness::new(SURFACE);
        h.ui.layer(Layer::Popup).max_size((-1.0, 10.0)).show(|_| {});
    });
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        panic_probe::assert_panics_with(domain::POSITIVE_RULE, || {
            let mut value = 0.5_f64;
            drop(Slider::new(&mut value, 0.0..=1.0).step(bad));
        });
    }
}

/// At worst-case input (NaN/infinite fractions, stale indices, empty lists, reversed ranges) each
/// coercing widget still lays out and paints, and no NaN reaches a rect or paint call (the capture's
/// `Debug` text shows a NaN in any field).
#[test]
fn coercing_inputs_at_their_worst_paint_no_nan() {
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::layout::track::Track;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::paint::brush::gradient::stops::Stop;
    use crate::primitives::paint::color::color_coords::ColorCoords;
    use crate::primitives::paint::color::okhsv::Okhsv;
    use crate::scene::layer::Layer;
    use crate::shape::Shape;
    use crate::widgets::color_field::ColorField;
    use crate::widgets::combo_box::ComboBox;
    use crate::widgets::drag_value::DragValue;
    use crate::widgets::progress_bar::ProgressBar;
    use crate::widgets::slider::Slider;
    use crate::widgets::splitter::Splitter;
    use crate::widgets::tabs::tab_item::TabItem;
    use crate::widgets::tabs::tab_strip::TabStrip;
    use crate::widgets::tabs::tabbed_view::TabbedView;
    use crate::widgets::text::Text;

    const OPTIONS: [&str; 2] = ["a", "b"];
    let mut h = UiHarness::new(UVec2::new(800, 1200));
    let mut ratio = f32::NAN;
    let (mut stale, mut empty, mut page, mut no_page) = (9_usize, 3_usize, 9_usize, 0_usize);
    let mut slid = f64::NAN;
    let mut dragged = f64::NAN;
    let mut coords = ColorCoords::Okhsv(Okhsv::new(f32::NAN, f32::INFINITY, f32::NAN));
    let gradient = LinearGradient::new(
        0.0,
        [
            Stop::new(f32::NAN, RgbaF32::WHITE),
            Stop::new(f32::INFINITY, RgbaF32::BLACK),
        ],
    );
    let mut scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("worst"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ProgressBar::new(f32::NAN).show(ui);
                ProgressBar::new(f32::INFINITY).show(ui);
                Splitter::row(&mut ratio)
                    .size((Sizing::fixed(200.0), Sizing::fixed(40.0)))
                    .show(ui, |_, _| {});
                ComboBox::new(&mut stale, &OPTIONS).show(ui);
                let no_options: [&str; 0] = [];
                ComboBox::new(&mut empty, &no_options).show(ui);
                TabbedView::new(&mut page, &OPTIONS)
                    .size((Sizing::fixed(200.0), Sizing::fixed(60.0)))
                    .show(ui, |ui, i| {
                        Text::new(OPTIONS[i]).show(ui);
                    });
                TabbedView::new(&mut no_page, &no_options)
                    .size((Sizing::fixed(200.0), Sizing::fixed(60.0)))
                    .show(ui, |_, _| {});
                let items = [TabItem::new(1, ui.intern("one"))];
                TabStrip::new(&items)
                    .selected(5)
                    .show(ui, |ui, chip| chip.show_label(ui));
                Slider::new(&mut slid, 10.0..=0.0).show(ui);
                DragValue::new(&mut dragged).range(5.0..=-5.0).show(ui);
                ColorField::new(&mut coords).show(ui);
                Grid::new()
                    .cols([Track::fixed(10.0).with_min(30.0).with_max(20.0)])
                    .rows([Track::HUG])
                    .show(ui, |ui| {
                        Block::new()
                            .min_size((50.0, 10.0))
                            .max_size((20.0, 5.0))
                            .show(ui);
                    });
                Block::new()
                    .size((Sizing::fixed(40.0), Sizing::fixed(10.0)))
                    .show(ui);
                ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 40.0, 10.0)).fill(gradient.clone()));
            });
    };
    h.frame(&mut scene);
    h.frame(&mut scene);

    for layer in Layer::PAINT_ORDER {
        for rect in &h.ui.layout(layer).rect {
            assert!(
                rect.min.is_finite() && rect.size.w.is_finite() && rect.size.h.is_finite(),
                "{layer:?}: {rect:?}",
            );
        }
    }
    let paint = format!("{:?}", h.encode_paint());
    assert!(!paint.contains("NaN"), "a NaN reached paint");
    assert_eq!(
        (stale, empty, page, no_page),
        (9, 3, 9, 0),
        "no index was written back"
    );
}

/// A widget with its own input scope takes its keys under an application root's scope.
#[test]
fn a_scoped_widget_takes_its_keys_under_an_enclosing_scope() {
    use crate::input::key_class::KeyFilter;
    use crate::input::keyboard::key::Key;
    use crate::primitives::layout::axis::Axis;
    use crate::primitives::paint::color::color_coords::ColorCoords;
    use crate::primitives::paint::color::color_model::ColorModel;
    use crate::widgets::axis_keys::UNIT_STEP;
    use crate::widgets::button::Button;
    use crate::widgets::checkbox::Checkbox;
    use crate::widgets::color_button::ColorButton;
    use crate::widgets::color_field::ColorField;
    use crate::widgets::color_picker::ColorPicker;
    use crate::widgets::color_strip::ColorStrip;
    use crate::widgets::color_swatch::ColorSwatch;
    use crate::widgets::combo_box::ComboBox;
    use crate::widgets::expander::Expander;
    use crate::widgets::slider::Slider;
    use crate::widgets::splitter::Splitter;

    fn under_root<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
        Panel::vstack()
            .id(WidgetId::from_hash("app-root"))
            .input_scope(KeyFilter::ACCEL)
            .show(ui, body)
            .inner
    }
    /// The axis widgets take their arrows from a parent that walks focus with them, as from the
    /// root's scope: each moves where the parent would have moved focus.
    fn under_arrows<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
        under_root(ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("arrow-row"))
                .arrow_focus(Axis::X)
                .show(ui, body)
                .inner
        })
    }
    /// Settle, focus `focus`, settle, press `key`, record two frames (a splitter's ratio lands a frame later).
    fn press(focus: WidgetId, key: Key, mut record: impl FnMut(&mut Ui)) {
        let mut h = UiHarness::new(SURFACE);
        h.frame(&mut record);
        h.set_focus(focus);
        h.frame(&mut record);
        h.key(key);
        h.frame(&mut record);
        h.frame(&mut record);
    }
    let id = own_id();

    let mut clicks = 0;
    press(id, Key::Char(' '), |ui| {
        let clicked = under_root(ui, |ui| Button::new().id(id).size(40.0).show(ui).clicked());
        clicks += usize::from(clicked);
    });
    assert_eq!(clicks, 1, "Button");

    let mut clicks = 0;
    press(id, Key::Enter, |ui| {
        let clicked = under_root(ui, |ui| {
            ColorSwatch::new(RgbaF32::WHITE).id(id).show(ui).clicked()
        });
        clicks += usize::from(clicked);
    });
    assert_eq!(clicks, 1, "ColorSwatch");

    let mut on = false;
    press(id, Key::Char(' '), |ui| {
        under_root(ui, |ui| {
            Checkbox::new(&mut on).id(id).show(ui);
        });
    });
    assert!(on, "Checkbox");

    let mut open = false;
    press(Expander::header_id(id), Key::Enter, |ui| {
        under_root(ui, |ui| {
            Expander::new("x").id(id).open(&mut open).show(ui, |_| {});
        });
    });
    assert!(open, "Expander");

    let mut picked = 0usize;
    press(id, Key::ArrowDown, |ui| {
        under_root(ui, |ui| {
            ComboBox::new(&mut picked, &["a", "b"]).id(id).show(ui);
        });
    });
    assert_eq!(picked, 1, "ComboBox");

    let mut color = RgbaF32::WHITE;
    let mut opened = false;
    press(id, Key::Enter, |ui| {
        under_root(ui, |ui| {
            ColorButton::new(ColorPicker::new(&mut color))
                .id(id)
                .show(ui);
        });
        opened |= PopupTrigger::is_open(ui, id);
    });
    assert!(opened, "ColorButton");

    let mut ratio = 0.5;
    press(id.with("divider"), Key::End, |ui| {
        under_root(ui, |ui| {
            Splitter::row(&mut ratio)
                .id(id)
                .size((200.0, 50.0))
                .show(ui, |_, _| {});
        });
    });
    // End sends the seam past the last free pixel, which clamps to the whole span; the arranged
    // ratio read back next frame is `first / (first + 0)`.
    assert_eq!(ratio, 1.0, "Splitter");

    let mut value = 50.0;
    press(id, Key::ArrowRight, |ui| {
        under_arrows(ui, |ui| {
            Slider::new(&mut value, 0.0..=100.0).id(id).show(ui);
        });
    });
    assert_eq!(value, 51.0, "Slider: a hundredth of the range");

    let mut coords = ColorCoords::from_color(RgbaF32::srgb(0.5, 0.25, 0.25), ColorModel::Hsv, 0.0);
    let before = coords.saturation();
    press(id, Key::ArrowRight, |ui| {
        under_arrows(ui, |ui| {
            ColorField::new(&mut coords).id(id).show(ui);
        });
    });
    assert_eq!(coords.saturation(), before + UNIT_STEP, "ColorField");

    let mut color = RgbaF32::WHITE;
    press(id, Key::ArrowLeft, |ui| {
        under_arrows(ui, |ui| {
            ColorStrip::for_alpha(&mut color).id(id).show(ui);
        });
    });
    assert_eq!(color.a, 1.0 - UNIT_STEP, "ColorStrip");
}
