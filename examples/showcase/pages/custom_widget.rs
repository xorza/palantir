//! Authoring a widget from the published API only: `Stepper` (`[ − ] value [ + ]` over a caller-owned `&mut i32`) is built as an outside crate would be.
//!
//! Nothing enforces that no crate internal is reached (the showcase builds with `internals`); the listing beside the demo is the check.

use crate::scaffold::api;
use crate::support;
use crate::support::{Col, Kit, RADIUS, ROW_GAP, Section};
use palantir::glam::Vec2;
use palantir::widget::{Configurator, LineCap, LineJoin, PolylineShape, Shape, Widget};
use palantir::{
    Align, Chrome, Configure, Corners, Key, Panel, Response, ResponseState, RgbaF32, Sense, Sizing,
    Stroke, Text, Ui, VAlign, WidgetId, fmt,
};

const BUTTON: f32 = 28.0;
/// Reserved, so the buttons hold still from one digit to three.
const VALUE_W: f32 = 34.0;
/// Fixed, so every stepper on the page starts at one x.
const LABEL_W: f32 = 62.0;
/// Sized to the longest entry in [`surface`]'s list, so descriptions start at one x.
const ITEM_W: f32 = 146.0;
/// Room around the buttons for the focus ring, which paints inside the widget's rect.
const RING_ROOM: f32 = 3.0;
const GLYPH_INSET: f32 = 8.0;

const CALL_SITE: &[&str] = &[
    "Stepper::new(&mut volume)",
    "    .range(0, 100)",
    "    .step(5)",
    "    .show(ui);",
];

#[derive(Debug)]
struct State {
    volume: i32,
    count: i32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            volume: 50,
            count: 0,
        }
    }
}

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    let state_id = WidgetId::from_hash("showcase::custom_widget::state");
    ui.with_state::<State, _>(state_id, |ui, s| page(ui, k, s));
}

fn page(ui: &mut Ui, k: Kit, s: &mut State) {
    support::cols(ui, |ui, col| match col {
        Col::Left => demo(ui, k, s),
        Col::Right => surface(ui, k),
    });
}

fn demo(ui: &mut Ui, k: Kit, s: &mut State) {
    Section::new("Stepper")
        .apis(&[api!(Widget::hstack), api!(Widget::key_pressed)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Two instances over separate values. Click a sign, or Tab to a stepper and \
                 use ↑ ↓ Home End.",
            );
            support::well(ui, k, |ui| {
                labeled(ui, k, "volume", |ui| {
                    Stepper::new(&mut s.volume).range(0, 100).step(5).show(ui);
                });
                labeled(ui, k, "count", |ui| {
                    Stepper::new(&mut s.count).range(-10, 10).show(ui);
                });
            });
        });

    Section::new("The call site").show(ui, k, |ui| {
        support::well(ui, k, |ui| {
            for (i, line) in CALL_SITE.iter().enumerate() {
                Text::new(*line)
                    .id_salt(i)
                    .style(&k.t.mono(k.t.ink))
                    .show(ui);
            }
        });
    });
}

/// A record body, named so the check can spell `Widget::record`'s signature.
type Body = fn(&mut Ui);

/// What an outside crate needs to write the same widget, each name checked by [`api!`].
fn surface(ui: &mut Ui, k: Kit) {
    let surface = [
        (api!(type Widget), "what it records, built and configured"),
        (api!(type Configure), "every shared setter, from one method"),
        (
            api!(Widget::resolve),
            "the stable id, resolved once and kept",
        ),
        (
            api!(Widget::record as fn(Widget, &mut Ui, Option<&Chrome>, Body)),
            "open the node, run the body, close it",
        ),
        (
            api!(Ui::response_for),
            "last frame's hover, press and click",
        ),
        (
            api!(Ui::add_shape as fn(&mut Ui, PolylineShape<'static>)),
            "paint custom geometry — the ± glyphs",
        ),
        (
            api!(Configure::focusable as fn(Stepper<'static>, bool) -> Stepper<'static>),
            "a Tab stop, with the framework's ring",
        ),
        (
            api!(Widget::key_pressed),
            "↑ ↓ Home End, read as the widget",
        ),
        (
            api!(WidgetId::with as fn(WidgetId, &'static str) -> WidgetId),
            "key child nodes off the parent id",
        ),
        (api!(Ui::theme), "the live theme its buttons wear"),
        (api!(type Response), "the value a caller chains on"),
    ];
    Section::new("The authoring surface it uses").show(ui, k, |ui| {
        support::well(ui, k, |ui| {
            for (i, (item, what)) in surface.into_iter().enumerate() {
                Panel::hstack()
                    .id_salt(i)
                    .size((Sizing::FILL, Sizing::HUG))
                    .gap(ROW_GAP)
                    .child_align(Align::v(VAlign::Center))
                    .show(ui, |ui| {
                        Text::new(item)
                            .style(&k.t.mono(k.t.ink))
                            .min_size((ITEM_W, 0.0))
                            .show(ui);
                        Text::new(what).style(&k.t.caption()).show(ui);
                    });
            }
        });
    });
}

#[track_caller]
fn labeled(ui: &mut Ui, k: Kit, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(ROW_GAP)
        .child_align(Align::v(VAlign::Center))
        .show(ui, |ui| {
            Text::new(label)
                .style(&k.t.caption())
                .min_size((LABEL_W, 0.0))
                .show(ui);
            body(ui);
        });
}

#[derive(Debug)]
struct Stepper<'a> {
    widget: Widget,
    value: &'a mut i32,
    min: i32,
    max: i32,
    step: i32,
}

impl<'a> Stepper<'a> {
    /// `#[track_caller]` so two `Stepper::new(...)`s on different lines get distinct ids and state.
    #[track_caller]
    fn new(value: &'a mut i32) -> Self {
        Self {
            widget: Widget::hstack(),
            value,
            min: i32::MIN,
            max: i32::MAX,
            step: 1,
        }
        .gap(6.0)
        .padding(RING_ROOM)
        .child_align(Align::v(VAlign::Center))
        .focusable(true)
    }

    fn range(mut self, lo: i32, hi: i32) -> Self {
        self.min = lo;
        self.max = hi.max(lo);
        self
    }

    fn step(mut self, s: i32) -> Self {
        self.step = s.max(1);
        self
    }

    fn show(self, ui: &mut Ui) -> Response<'_> {
        // Clicks apply before recording, so the new value paints this frame.
        let mut widget = self.widget;
        let id = widget.resolve(ui);
        let minus_id = id.with("minus");
        let plus_id = id.with("plus");
        let minus = ui.response_for(minus_id);
        let plus = ui.response_for(plus_id);
        if minus.clicked() {
            *self.value = self.value.saturating_sub(self.step).max(self.min);
        }
        if plus.clicked() {
            *self.value = self.value.saturating_add(self.step).min(self.max);
        }
        if ui.is_focus_within(id) {
            // Sampled, not short-circuited: each read also subscribes its chord.
            let up = widget.shortcut_pressed(ui, Key::ArrowUp);
            let down = widget.shortcut_pressed(ui, Key::ArrowDown);
            let home = widget.shortcut_pressed(ui, Key::Home);
            let end = widget.shortcut_pressed(ui, Key::End);
            if up {
                *self.value = self.value.saturating_add(self.step).min(self.max);
            }
            if down {
                *self.value = self.value.saturating_sub(self.step).max(self.min);
            }
            if home {
                *self.value = self.min;
            }
            if end {
                *self.value = self.max;
            }
        }

        // Straight into the record store — no `String` is built at all.
        let label = fmt!(ui, "{}", self.value);

        // No fill of its own: the corners serve the framework's focus ring.
        let chrome = Chrome::rounded(RgbaF32::TRANSPARENT, Corners::all(RADIUS + RING_ROOM));
        widget
            .show(ui, Some(&chrome), |ui| {
                StepButton {
                    id: minus_id,
                    state: minus,
                    glyph: Glyph::Minus,
                }
                .show(ui);
                Text::new(label)
                    .id(id.with("value"))
                    .text_align(Align::CENTER)
                    .min_size((VALUE_W, 0.0))
                    .show(ui);
                StepButton {
                    id: plus_id,
                    state: plus,
                    glyph: Glyph::Plus,
                }
                .show(ui);
            })
            .response
    }
}

/// One method earns the whole chained-setter vocabulary for the builder.
impl Configure for Stepper<'_> {
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[derive(Clone, Copy, Debug)]
enum Glyph {
    Minus,
    Plus,
}

/// One of the stepper's two buttons.
#[derive(Debug)]
struct StepButton {
    /// Explicit, so the node resolves to what [`Stepper::show`] read a response for.
    id: WidgetId,
    state: ResponseState,
    glyph: Glyph,
}

impl StepButton {
    fn show(self, ui: &mut Ui) {
        let Self { id, state, glyph } = self;
        // The theme's button looks, so the stepper follows a scheme switch as palantir's own
        // widgets do; only the corners are the stepper's.
        let looks = &ui.theme().button.unselected;
        let look = if state.pressed() {
            &looks.active
        } else if state.hovered() {
            &looks.hovered
        } else {
            &looks.normal
        };
        let chrome = Chrome {
            corners: Corners::all(RADIUS),
            ..look.chrome.clone()
        };
        let ink = ui.theme().text.color;
        let widget = Widget::leaf()
            .id(id)
            .size((Sizing::fixed(BUTTON), Sizing::fixed(BUTTON)))
            .sense(Sense::CLICK);
        widget.record(ui, Some(&chrome), |ui| {
            let mid = BUTTON / 2.0;
            let far = BUTTON - GLYPH_INSET;
            paint_bar(ui, ink, &[Vec2::new(GLYPH_INSET, mid), Vec2::new(far, mid)]);
            if matches!(glyph, Glyph::Plus) {
                paint_bar(ui, ink, &[Vec2::new(mid, GLYPH_INSET), Vec2::new(mid, far)]);
            }
        });
    }
}

fn paint_bar(ui: &mut Ui, ink: RgbaF32, points: &[Vec2]) {
    ui.add_shape(
        Shape::polyline(points, Stroke::new(ink, 2.0))
            .cap(LineCap::Round)
            .join(LineJoin::Round),
    );
}
