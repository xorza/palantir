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
use crate::widgets::popup::popup_trigger::PopupTrigger;
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

/// A trigger the pointer rests on, for the tooltip and the popup trigger.
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
            name: "PopupTrigger",
            record: |ui, c| {
                let trigger = hovered_trigger();
                PopupTrigger::open(ui, trigger.id);
                c.apply(
                    PopupTrigger::on(&trigger),
                    PopupTrigger::background,
                    PopupTrigger::default_background,
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
/// setter: a NaN fill colour, a negative border width, a NaN corner, a
/// corner past the f16 range and a NaN shadow blur each panic with their
/// kind's rule.
#[test]
fn chrome_setters_check_the_background() {
    use crate::internals::panic_probe;
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::math::domain;
    use crate::primitives::packed::serde::LaneCodec;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;

    let bad: [(&str, Background); 5] = [
        (
            "a color must have finite channels",
            Background::fill(RgbaF32::new(f32::NAN, 0.0, 0.0, 1.0)),
        ),
        (
            domain::LENGTH_RULE,
            Background::fill(RgbaF32::WHITE).with_border(Stroke::new(RgbaF32::WHITE, -1.0)),
        ),
        (
            <Corners as LaneCodec>::LANE_RULE,
            Background::rounded(RgbaF32::WHITE, Corners::all(f32::NAN)),
        ),
        (
            <Corners as LaneCodec>::LANE_RULE,
            Background::rounded(RgbaF32::WHITE, Corners::all(1.0e5)),
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
    use crate::primitives::geometry::spacing::Spacing;
    use crate::primitives::layout::anchor::Anchor;
    use crate::primitives::math::domain;
    use crate::primitives::paint::color::color_coords::ColorCoords;
    use crate::scene::layer::Layer;
    use crate::widgets::color_field::ColorField;
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
    // A line height is positive, as a theme file states it: zero leading
    // stacks every line on the first.
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

    // The layout and overlay setters a frame calls: a pane floor and a
    // margin are lengths, a pan and a point are offsets, a cap is an
    // extent, a rect is geometry, and a slider step is positive.
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
    let offset: [fn(); 4] = [
        || drop(Scroll::both().pan_by(Vec2::new(f32::NAN, 0.0))),
        || {
            let _ = Anchor::at_point(Vec2::new(0.0, f32::INFINITY));
        },
        || drop(Popup::below(Rect::new(f32::NAN, 0.0, 4.0, 4.0))),
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

/// The frame property behind every coercing input: at its worst — NaN and
/// infinite fractions, stale and out-of-range indices, empty lists,
/// reversed ranges, a minimum above its maximum — each one still lays out
/// and paints, and no NaN reaches an arranged rect or a paint call.
///
/// The paint side reads the capture's `Debug` text, because it is the one
/// view that walks every payload of every call: a NaN prints as `NaN`
/// whatever field or lane it sits in.
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
                ComboBox::new(&mut empty, &[] as &[&str]).show(ui);
                TabbedView::new(&mut page, &OPTIONS)
                    .size((Sizing::fixed(200.0), Sizing::fixed(60.0)))
                    .show(ui, |ui, i| {
                        Text::new(OPTIONS[i]).show(ui);
                    });
                TabbedView::new(&mut no_page, &[] as &[&str])
                    .size((Sizing::fixed(200.0), Sizing::fixed(60.0)))
                    .show(ui, |_, _| {});
                let items = [TabItem::new(1, ui.intern("one"))];
                TabStrip::new(&items).selected(5).show(ui);
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

/// Every widget that declares its own input scope takes its keys while an
/// application root's scope encloses it — the case where reading as the
/// record position, the node around the widget, reads as the root and
/// misses the key the widget was granted.
#[test]
fn a_scoped_widget_takes_its_keys_under_an_enclosing_scope() {
    use crate::input::key_class::KeyFilter;
    use crate::input::keyboard::key::Key;
    use crate::widgets::button::Button;
    use crate::widgets::checkbox::Checkbox;
    use crate::widgets::color_button::ColorButton;
    use crate::widgets::combo_box::ComboBox;
    use crate::widgets::expander::Expander;
    use crate::widgets::splitter::Splitter;

    fn under_root<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
        Panel::vstack()
            .id(WidgetId::from_hash("app-root"))
            .input_scope(KeyFilter::ACCEL)
            .show(ui, body)
            .inner
    }
    /// Settle, focus `focus`, settle, press `key`, and record two frames —
    /// a splitter's ratio comes back on the frame after the key.
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

    let mut on = false;
    press(id, Key::Char(' '), |ui| {
        under_root(ui, |ui| {
            Checkbox::new(&mut on).id(id).show(ui);
        });
    });
    assert!(on, "Checkbox");

    let mut open = false;
    press(id.with("header"), Key::Enter, |ui| {
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
            ColorButton::new(&mut color).id(id).show(ui);
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
    assert!(ratio > 0.99, "Splitter: {ratio}");
}
