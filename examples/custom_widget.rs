//! Authoring a **custom widget** from aperture's public API.
//!
//! `Stepper` — `[ − ]  value  [ + ]` over a `&mut i32` — is built without
//! touching any crate internals. It exercises the full widget-authoring
//! surface, so this example doubles as a compile-time proof that the
//! surface is complete:
//!
//! * `Element::new` + the `Configure` builder (implemented for `Element`
//!   itself) — construct and configure layout nodes.
//! * `Ui::widget_id` — resolve a stable [`WidgetId`] once per widget.
//! * `Ui::node` — open a node, run its body, close it.
//! * `Ui::response_for` — read last frame's interaction (hover/click/…).
//! * `Ui::add_shape` — paint custom geometry (the ± glyphs).
//! * `WidgetId::with` — key child nodes off the parent id.
//! * `Response` — the return value callers chain on.
//!
//! Run with: `cargo run --example custom_widget`

use aperture::{
    Align, App, Background, Color, Configure, Corners, Element, HostHandle, LayoutMode, LineCap,
    LineJoin, Panel, PolylineColors, Response, ResponseState, Sense, Shadow, Shape, Sizing, Stroke,
    Text, Ui, VAlign, Vec2, WidgetId, WindowToken, WinitHost, WinitHostConfig,
};

/// A horizontal integer stepper bound to a caller-owned `&mut i32`.
pub struct Stepper<'a> {
    element: Element,
    value: &'a mut i32,
    min: i32,
    max: i32,
    step: i32,
}

impl<'a> Stepper<'a> {
    /// `#[track_caller]` so the auto-derived id reflects *this* call site
    /// — two `Stepper::new(...)`s on different source lines get distinct
    /// ids (and therefore distinct per-widget state) for free.
    #[track_caller]
    pub fn new(value: &'a mut i32) -> Self {
        Self {
            element: Element::new(LayoutMode::HStack),
            value,
            min: i32::MIN,
            max: i32::MAX,
            step: 1,
        }
        // `Stepper` implements `Configure` (below), so the layout setters
        // are available on the builder itself.
        .gap(8.0)
        .child_align(Align::v(VAlign::Center))
    }

    pub fn range(mut self, lo: i32, hi: i32) -> Self {
        self.min = lo;
        self.max = hi.max(lo);
        self
    }

    pub fn step(mut self, s: i32) -> Self {
        self.step = s.max(1);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        // 1) Resolve the container id once, then read last frame's
        //    interaction for the two buttons (keyed off it) and apply
        //    clicks *before* recording — so the new value paints this frame.
        let id = ui.widget_id(&self.element);
        let minus_id = id.with("minus");
        let plus_id = id.with("plus");
        let minus = ui.response_for(minus_id);
        let plus = ui.response_for(plus_id);
        if minus.clicked && !minus.disabled {
            *self.value = (*self.value - self.step).max(self.min);
        }
        if plus.clicked && !plus.disabled {
            *self.value = (*self.value + self.step).min(self.max);
        }

        // Intern the formatted number into the per-frame arena (no
        // lingering `String` alloc) and reuse the theme's text style.
        let label = ui.intern(&self.value.to_string());
        let label_style = ui.theme.text;

        // 2) Open the container and record its three children.
        ui.node(id, self.element, None, |ui| {
            step_button(ui, minus_id, minus, Glyph::Minus);
            Text::new(label)
                .id(id.with("value"))
                .style(label_style)
                .text_align(Align::v(VAlign::Center))
                .show(ui);
            step_button(ui, plus_id, plus, Glyph::Plus);
        });

        // 3) Hand back a Response for the container so callers can chain
        //    `.hovered()` etc.; the `&mut i32` mutation is the real effect.
        Response::lazy(id, ui)
    }
}

/// The container builder gets every chained setter (`.gap`, `.padding`,
/// `.id_salt`, `.size`, …) for free by implementing just `element_mut`.
impl Configure for Stepper<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

enum Glyph {
    Minus,
    Plus,
}

/// One 24×24 button leaf: state-driven chrome plus a glyph painted with
/// `Ui::add_shape`. `id` is an explicit child id (`parent.with(...)`), so
/// it goes straight to `node` — no `widget_id` round-trip needed.
fn step_button(ui: &mut Ui, id: WidgetId, state: ResponseState, glyph: Glyph) {
    let fill = if state.pressed {
        Color::rgb_u8(0x3a, 0x3a, 0x52)
    } else if state.hovered {
        Color::rgb_u8(0x33, 0x33, 0x48)
    } else {
        Color::rgb_u8(0x26, 0x26, 0x3a)
    };
    let chrome = Background {
        fill: fill.into(),
        stroke: Stroke::ZERO,
        corners: Corners::all(5.0),
        shadow: Shadow::NONE,
    };
    let el = Element::new(LayoutMode::Leaf)
        .id(id)
        .size((Sizing::Fixed(24.0), Sizing::Fixed(24.0)))
        .sense(Sense::CLICK);
    ui.node(id, el, Some(&chrome), |ui| {
        // Glyphs in node-local coordinates (0..24 on each axis). A
        // horizontal bar is the minus; the plus adds a vertical bar.
        let horiz = [Vec2::new(7.0, 12.0), Vec2::new(17.0, 12.0)];
        paint_bar(ui, &horiz);
        if matches!(glyph, Glyph::Plus) {
            let vert = [Vec2::new(12.0, 7.0), Vec2::new(12.0, 17.0)];
            paint_bar(ui, &vert);
        }
    });
}

fn paint_bar(ui: &mut Ui, points: &[Vec2]) {
    ui.add_shape(Shape::Polyline {
        points,
        colors: PolylineColors::Single(Color::WHITE),
        width: 2.0,
        cap: LineCap::Round,
        join: LineJoin::Round,
    });
}

// ── demo app ──────────────────────────────────────────────────────────

struct Demo {
    volume: i32,
    count: i32,
}

impl Demo {
    fn new(_ui: &mut Ui, _handle: HostHandle<Self>) -> Self {
        Demo {
            volume: 50,
            count: 0,
        }
    }
}

impl App for Demo {
    fn frame(&mut self, _win: WindowToken, ui: &mut Ui) {
        Panel::vstack()
            .auto_id()
            .gap(16.0)
            .padding(24.0)
            .show(ui, |ui| {
                Text::new("Custom Stepper widget").auto_id().show(ui);

                Text::new(format!("volume: {}", self.volume))
                    .auto_id()
                    .show(ui);
                Stepper::new(&mut self.volume)
                    .range(0, 100)
                    .step(5)
                    .show(ui);

                Text::new(format!("count: {}", self.count))
                    .auto_id()
                    .show(ui);
                Stepper::new(&mut self.count).range(-10, 10).show(ui);
            });
    }
}

fn main() {
    WinitHost::new(
        WindowToken(0),
        WinitHostConfig::new("custom widget"),
        Demo::new,
    )
    .run();
}
