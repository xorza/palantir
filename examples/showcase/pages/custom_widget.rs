//! Authoring a widget from the published API, and nothing else.
//!
//! `Stepper` — `[ − ] value [ + ]` over a caller-owned `&mut i32` — is
//! built the way a widget in another crate would be: `Widget` plus the
//! `Configure` builder, `Ui::response_for` to read last frame, and
//! `Ui::add_shape` to paint the ± glyphs.
//!
//! It reaches no crate internal, but nothing here enforces that — the
//! showcase builds with `internals` on. The listing beside the demo is
//! the check, not the compiler.
//!
//! Its chrome reads the showcase's own `ELEM` ladder rather than hexes of
//! its own, so it rests, hovers and presses at the rungs every shipped
//! widget beside it does.

use crate::support;
use palantir::widget::{ConfigureWidget, LineCap, LineJoin, PolylineColors, Shape, Widget};
use palantir::{
    Align, Background, Configure, Corners, FontFamily, Panel, Response, ResponseState, Sense,
    Shadow, Sizing, Stroke, Text, Ui, VAlign, Vec2, WidgetId, fmt,
};

/// Large enough to read beside 13 px body copy, small enough that the pair
/// plus the value still reads as one control.
const BUTTON: f32 = 28.0;
/// Reserved, so the buttons hold still as the number travels from one digit
/// to three.
const VALUE_W: f32 = 34.0;
/// Fixed, so every stepper on the page starts at one x.
const LABEL_W: f32 = 62.0;
/// Sized to the longest entry in [`SURFACE`], so every description starts at
/// one x.
const ITEM_W: f32 = 146.0;
const WELL_GAP: f32 = 7.0;
const CODE_SIZE: f32 = 12.0;
/// Leaves a 12 px bar centred in a [`BUTTON`]-square node.
const GLYPH_INSET: f32 = 8.0;

/// What an outside crate would need to write the same widget.
const SURFACE: &[(&str, &str)] = &[
    (
        "Widget + Configure",
        "construct and configure what it records",
    ),
    ("Widget::resolve", "the stable id, resolved once and kept"),
    ("Widget::record", "open the node, run the body, close it"),
    ("Ui::response_for", "last frame's hover, press and click"),
    ("Ui::add_shape", "paint custom geometry — the ± glyphs"),
    ("WidgetId::with", "key child nodes off the parent id"),
    ("Response", "the value a caller chains on"),
];

/// Shown beside the demo, so the page carries what an application writes as
/// well as what it gets.
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

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::custom_widget::state");
    ui.with_state::<State, _>(state_id, page);
}

fn page(ui: &mut Ui, s: &mut State) {
    Panel::hstack()
        .id_salt("columns")
        .gap(24.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            support::column(ui, "col-l", |ui| demo(ui, s));
            support::column(ui, "col-r", surface);
        });
}

fn demo(ui: &mut Ui, s: &mut State) {
    support::section(ui, "stepper — two instances over separate values", |ui| {
        well(ui, "demo", |ui| {
            labelled(ui, "volume", |ui| {
                Stepper::new(&mut s.volume).range(0, 100).step(5).show(ui);
            });
            labelled(ui, "count", |ui| {
                Stepper::new(&mut s.count).range(-10, 10).show(ui);
            });
        });
    });

    support::section(ui, "the call site", |ui| {
        well(ui, "call-site", |ui| {
            for (i, line) in CALL_SITE.iter().enumerate() {
                Text::new(*line)
                    .id_salt(i)
                    .family(FontFamily::MONO)
                    .font_size(CODE_SIZE)
                    .color(support::INK)
                    .show(ui);
            }
        });
    });
}

fn surface(ui: &mut Ui) {
    support::section(ui, "the authoring surface it uses", |ui| {
        well(ui, "surface", |ui| {
            for (i, (item, what)) in SURFACE.iter().enumerate() {
                Panel::hstack()
                    .id_salt(i)
                    .size((Sizing::FILL, Sizing::HUG))
                    .gap(support::ROW_GAP)
                    .child_align(Align::v(VAlign::Center))
                    .show(ui, |ui| {
                        Text::new(*item)
                            .id_salt("item")
                            .family(FontFamily::MONO)
                            .font_size(CODE_SIZE)
                            .color(support::INK)
                            .min_size((ITEM_W, 0.0))
                            .show(ui);
                        Text::new(*what)
                            .id_salt("what")
                            .style(&support::caption_style())
                            .show(ui);
                    });
            }
        });
    });
}

#[track_caller]
fn well(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(WELL_GAP)
        .padding(14.0)
        .background(support::well_bg())
        .show(ui, body);
}

/// Centred across the control's height, so a 12 px label sits on the axis of
/// a 28 px button.
#[track_caller]
fn labelled(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(support::ROW_GAP)
        .child_align(Align::v(VAlign::Center))
        .show(ui, |ui| {
            Text::new(label)
                .id_salt("label")
                .style(&support::note_style())
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
    /// `#[track_caller]` so the auto-derived id reflects *this* call site:
    /// two `Stepper::new(...)`s on different lines get distinct ids, and so
    /// distinct per-widget state, for free.
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
        .child_align(Align::v(VAlign::Center))
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
        // Clicks apply *before* recording, so the new value paints this
        // frame rather than the next.
        let mut widget = self.widget;
        let id = widget.resolve(ui);
        let minus_id = id.with("minus");
        let plus_id = id.with("plus");
        let minus = ui.response_for(minus_id);
        let plus = ui.response_for(plus_id);
        if minus.clicked() {
            *self.value = (*self.value - self.step).max(self.min);
        }
        if plus.clicked() {
            *self.value = (*self.value + self.step).min(self.max);
        }

        // Straight into the record store — no `String` is built at all.
        let label = fmt!(ui, "{}", self.value);

        widget
            .show(ui, None, |ui| {
                step_button(ui, minus_id, minus, Glyph::Minus);
                Text::new(label)
                    .id(id.with("value"))
                    .style(&support::body_style())
                    .text_align(Align::CENTER)
                    .min_size((VALUE_W, 0.0))
                    .show(ui);
                step_button(ui, plus_id, plus, Glyph::Plus);
            })
            .response
    }
}

/// One method earns the whole chained-setter vocabulary for the builder.
impl Configure for Stepper<'_> {
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

enum Glyph {
    Minus,
    Plus,
}

/// `id` is an explicit child id rather than an auto-derived one, so the node
/// resolves to what [`Stepper::show`] already read a response for.
fn step_button(ui: &mut Ui, id: WidgetId, state: ResponseState, glyph: Glyph) {
    let fill = if state.pressed() {
        support::ELEM_STRONG
    } else if state.hovered() {
        support::ELEM_MID
    } else {
        support::ELEM
    };
    let chrome = Background {
        fill: fill.into(),
        stroke: Stroke::ZERO,
        corners: Corners::all(support::RADIUS),
        shadow: Shadow::NONE,
    };
    let widget = Widget::leaf()
        .id(id)
        .size((Sizing::fixed(BUTTON), Sizing::fixed(BUTTON)))
        .sense(Sense::CLICK);
    widget.record(ui, Some(&chrome), |ui| {
        // Node-local coordinates, 0..BUTTON on each axis.
        let mid = BUTTON / 2.0;
        let far = BUTTON - GLYPH_INSET;
        paint_bar(ui, &[Vec2::new(GLYPH_INSET, mid), Vec2::new(far, mid)]);
        if matches!(glyph, Glyph::Plus) {
            paint_bar(ui, &[Vec2::new(mid, GLYPH_INSET), Vec2::new(mid, far)]);
        }
    });
}

fn paint_bar(ui: &mut Ui, points: &[Vec2]) {
    ui.add_shape(
        Shape::polyline(points, PolylineColors::Single(support::INK), 2.0)
            .cap(LineCap::Round)
            .join(LineJoin::Round),
    );
}
