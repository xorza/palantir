//! Drop-shadow showcase. Each cell drops one (or more) shadows under
//! a rounded card via direct `Shape::Shadow` pushes, then paints the
//! card on top. Exercises the full encode → compose → shader path
//! including the per-corner SDF, the σ → 0 sharp fallback, and
//! multi-shadow stacking via record order.

use glam::Vec2;
use palantir::{Background, Color, Configure, Corners, Panel, Rect, Shadow, Shape, Sizing, Ui};

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .padding(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::hstack()
                .id_salt("row1")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "soft", soft);
                    cell(ui, "elevated", elevated);
                    cell(ui, "tight", tight);
                });
            Panel::hstack()
                .id_salt("row2")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "sharp", sharp);
                    cell(ui, "glow", glow);
                    cell(ui, "inset", inset);
                    cell(ui, "stacked", stacked);
                });
            // Chrome-attached shadows: `Background { shadow: Some(_) }`
            // routes through the encoder's chrome branch — paints
            // before the rect fill, so it composes correctly under
            // semi-transparent fills. Compare with row 1's
            // `Shape::Shadow` route (paints over fill via shape walk).
            Panel::hstack()
                .id_salt("row3-chrome")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    chrome_cell(ui, "soft", chrome_soft());
                    chrome_cell(ui, "elevated", chrome_elevated());
                    chrome_cell(ui, "inset", chrome_inset());
                    chrome_cell(ui, "translucent", chrome_translucent());
                });
        });
}

fn cell(ui: &mut Ui, id: &'static str, paint: impl Fn(&mut Ui)) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(24.0)
        .show(ui, paint);
}

fn card_rect() -> Rect {
    Rect::new(20.0, 20.0, 160.0, 100.0)
}

fn radius() -> Corners {
    Corners::all(12.0)
}

fn card_fill(ui: &mut Ui) {
    ui.add_shape(Shape::RoundedRect {
        local_rect: Some(card_rect()),
        radius: radius(),
        fill: Color::rgb(0.95, 0.95, 0.97).into(),
        stroke: Default::default(),
    });
}

/// Standard soft drop shadow — Material Design "elevation 2".
fn soft(ui: &mut Ui) {
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.0, 0.0, 0.0, 0.20),
        offset: Vec2::new(0.0, 4.0),
        blur: 8.0,
        spread: 0.0,
        inset: false,
    });
    card_fill(ui);
}

/// Heavier drop, larger blur — "elevation 8" look.
fn elevated(ui: &mut Ui) {
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.0, 0.0, 0.0, 0.28),
        offset: Vec2::new(0.0, 12.0),
        blur: 20.0,
        spread: 0.0,
        inset: false,
    });
    card_fill(ui);
}

/// Tight, dense shadow hugging the shape — UI button rest state.
fn tight(ui: &mut Ui) {
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.0, 0.0, 0.0, 0.35),
        offset: Vec2::new(0.0, 1.0),
        blur: 2.0,
        spread: 0.0,
        inset: false,
    });
    card_fill(ui);
}

/// σ = 0 — sharp drop. Should match the rounded-rect SDF exactly,
/// shifted by `offset`. Pins the degenerate-blur code path visually.
fn sharp(ui: &mut Ui) {
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.0, 0.0, 0.0, 1.0),
        offset: Vec2::new(6.0, 6.0),
        blur: 2.0,
        spread: 0.0,
        inset: false,
    });
    card_fill(ui);
}

/// Coloured glow, zero offset — bloom feel.
fn glow(ui: &mut Ui) {
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.4, 0.6, 1.0, 0.6),
        offset: Vec2::ZERO,
        blur: 18.0,
        spread: 2.0,
        inset: false,
    });
    card_fill(ui);
}

/// Inset shadow — interior darkening, pressed-button feel.
fn inset(ui: &mut Ui) {
    card_fill(ui);
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.0, 0.0, 0.0, 0.45),
        offset: Vec2::new(0.0, 3.0),
        blur: 8.0,
        spread: 0.0,
        inset: true,
    });
}

/// One chrome-cell: a centered card sized to roughly match `card_rect`,
/// painted via `Background` (fill + radius + shadow) instead of
/// shape pushes. Demonstrates the option-1 path: shadow emitted by
/// the encoder before the chrome rect.
fn chrome_cell(ui: &mut Ui, id: &'static str, bg: Background) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(24.0)
        .show(ui, |ui| {
            Panel::zstack()
                .auto_id()
                .size((Sizing::Fixed(140.0), Sizing::Fixed(80.0)))
                .background(bg)
                .show(ui, |_| {});
        });
}

fn chrome_soft() -> Background {
    Background {
        fill: Color::rgb(0.95, 0.95, 0.97).into(),
        stroke: Default::default(),
        radius: Corners::all(12.0),
        shadow: Some(Shadow {
            color: Color::rgba(0.0, 0.0, 0.0, 0.20),
            offset: Vec2::new(0.0, 4.0),
            blur: 8.0,
            spread: 0.0,
            inset: false,
        }),
    }
}

fn chrome_elevated() -> Background {
    Background {
        fill: Color::rgb(0.95, 0.95, 0.97).into(),
        stroke: Default::default(),
        radius: Corners::all(12.0),
        shadow: Some(Shadow {
            color: Color::rgba(0.0, 0.0, 0.0, 0.28),
            offset: Vec2::new(0.0, 12.0),
            blur: 20.0,
            spread: 0.0,
            inset: false,
        }),
    }
}

fn chrome_inset() -> Background {
    Background {
        fill: Color::rgb(0.95, 0.95, 0.97).into(),
        stroke: Default::default(),
        radius: Corners::all(12.0),
        shadow: Some(Shadow {
            color: Color::rgba(0.0, 0.0, 0.0, 0.45),
            offset: Vec2::new(0.0, 3.0),
            blur: 8.0,
            spread: 0.0,
            inset: true,
        }),
    }
}

/// Semi-transparent chrome fill: the shadow paints UNDER the fill,
/// so the halo doesn't bleed through. This is the case the
/// shape-buffer-lowering route gets wrong; encoder-path is correct.
fn chrome_translucent() -> Background {
    Background {
        fill: Color::rgba(0.95, 0.95, 0.97, 0.4).into(),
        stroke: Default::default(),
        radius: Corners::all(12.0),
        shadow: Some(Shadow {
            color: Color::rgba(0.0, 0.0, 0.0, 0.5),
            offset: Vec2::new(0.0, 6.0),
            blur: 12.0,
            spread: 0.0,
            inset: false,
        }),
    }
}

/// Multi-shadow stack — CSS `box-shadow: a, b, c`. Pushed in record
/// order, the deepest first; composer batches them onto one draw.
fn stacked(ui: &mut Ui) {
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.0, 0.0, 0.0, 0.18),
        offset: Vec2::new(0.0, 24.0),
        blur: 32.0,
        spread: 0.0,
        inset: false,
    });
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.0, 0.0, 0.0, 0.22),
        offset: Vec2::new(0.0, 8.0),
        blur: 10.0,
        spread: 0.0,
        inset: false,
    });
    ui.add_shape(Shape::Shadow {
        local_rect: Some(card_rect()),
        radius: radius(),
        color: Color::rgba(0.0, 0.0, 0.0, 0.30),
        offset: Vec2::new(0.0, 1.0),
        blur: 2.0,
        spread: 0.0,
        inset: false,
    });
    card_fill(ui);
}
