//! Drop shadows from both directions. The first two sections push
//! `Shape::Shadow` directly and paint the card on top — exercising the
//! per-corner SDF, the σ → 0 sharp fallback, and multi-shadow stacking
//! by record order. The third attaches the shadow to widget chrome
//! (`Background { shadow }`), which routes through the encoder's chrome
//! branch and paints *before* the rect fill, so it composes correctly
//! under a semi-transparent fill.
//!
//! Every tile is on the bright surface: black-on-dark shadows don't read.

use crate::support::{demo_cell_light, section, tiles};
use glam::Vec2;
use palantir::{
    Background, Color, Configure, Corners, Panel, Rect, Shadow, ShadowShape, Shape, Sizing, Ui,
};

const CARD: Rect = Rect::new(22.0, 28.0, 124.0, 86.0);
const CARD_INK: Color = Color::hex(0xf2f2f7);

fn card_corners() -> Corners {
    Corners::all(12.0)
}

pub(crate) fn build(ui: &mut Ui) {
    section(
        ui,
        "elevation — drop shadow under a rounded card, the standard ladder",
        |ui| {
            tiles(ui, |ui| {
                demo_cell_light(ui, "soft — elevation 2", soft);
                demo_cell_light(ui, "elevated — offset 12, blur 20", elevated);
                demo_cell_light(ui, "tight — button rest state", tight);
                demo_cell_light(ui, "sharp — σ→0 fallback", sharp);
            });
        },
    );

    section(ui, "variants — colour, direction, and stacking", |ui| {
        tiles(ui, |ui| {
            demo_cell_light(ui, "glow — coloured, zero offset", glow);
            demo_cell_light(ui, "inset — pressed feel", inset);
            demo_cell_light(ui, "stacked — CSS box-shadow a, b, c", stacked);
        });
    });

    section(
        ui,
        "chrome — the same shadows attached to a widget's Background instead of \
         pushed as shapes",
        |ui| {
            tiles(ui, |ui| {
                demo_cell_light(ui, "chrome — soft", |ui| chrome_card(ui, chrome_soft()));
                demo_cell_light(ui, "chrome — elevated", |ui| {
                    chrome_card(ui, chrome_elevated());
                });
                demo_cell_light(ui, "chrome — inset", |ui| chrome_card(ui, chrome_inset()));
                demo_cell_light(ui, "chrome — translucent fill", |ui| {
                    chrome_card(ui, chrome_translucent());
                });
            });
        },
    );
}

fn shadow_shape(s: Shadow) -> ShadowShape {
    Shape::shadow(s).at(CARD).corners(card_corners())
}

fn card_fill(ui: &mut Ui) {
    ui.add_shape(Shape::rect(CARD).fill(CARD_INK).corners(card_corners()));
}

/// Standard soft drop shadow — Material Design "elevation 2".
fn soft(ui: &mut Ui) {
    ui.add_shape(shadow_shape(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.20),
        Vec2::new(0.0, 4.0),
        8.0,
    )));
    card_fill(ui);
}

/// Heavier drop, larger blur — "elevation 8" look.
fn elevated(ui: &mut Ui) {
    ui.add_shape(shadow_shape(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.28),
        Vec2::new(0.0, 12.0),
        20.0,
    )));
    card_fill(ui);
}

/// Tight, dense shadow hugging the shape — UI button rest state.
fn tight(ui: &mut Ui) {
    ui.add_shape(shadow_shape(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.35),
        Vec2::new(0.0, 1.0),
        2.0,
    )));
    card_fill(ui);
}

/// σ = 0 — sharp drop. Should match the rounded-rect SDF exactly,
/// shifted by `offset`. Pins the degenerate-blur code path visually.
fn sharp(ui: &mut Ui) {
    ui.add_shape(shadow_shape(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 1.0),
        Vec2::new(6.0, 6.0),
        2.0,
    )));
    card_fill(ui);
}

/// Coloured glow, zero offset — bloom feel.
fn glow(ui: &mut Ui) {
    ui.add_shape(shadow_shape(
        Shadow::drop(Color::rgba(0.4, 0.6, 1.0, 0.6), Vec2::ZERO, 18.0).with_spread(2.0),
    ));
    card_fill(ui);
}

/// Inset shadow — interior darkening, pressed-button feel.
fn inset(ui: &mut Ui) {
    card_fill(ui);
    ui.add_shape(shadow_shape(
        Shadow::drop(Color::rgba(0.0, 0.0, 0.0, 0.45), Vec2::new(0.0, 3.0), 8.0).inset(),
    ));
}

/// Multi-shadow stack — CSS `box-shadow: a, b, c`. Pushed in record
/// order, the deepest first; the composer batches them onto one draw.
fn stacked(ui: &mut Ui) {
    for (dy, blur, alpha) in [(18.0, 24.0, 0.18), (8.0, 10.0, 0.22), (1.0, 2.0, 0.30)] {
        ui.add_shape(shadow_shape(Shadow::drop(
            Color::rgba(0.0, 0.0, 0.0, alpha),
            Vec2::new(0.0, dy),
            blur,
        )));
    }
    card_fill(ui);
}

/// A centred card painted via `Background` (fill + radius + shadow)
/// instead of shape pushes — the encoder emits the shadow before the
/// chrome rect.
fn chrome_card(ui: &mut Ui, bg: Background) {
    Panel::zstack()
        .size((Sizing::FILL, Sizing::FILL))
        .padding(16.0)
        .show(ui, |ui| {
            Panel::zstack()
                .size((Sizing::fixed(112.0), Sizing::fixed(66.0)))
                .background(bg)
                .show(ui, |_| {});
        });
}

fn chrome_soft() -> Background {
    Background::rounded(CARD_INK, card_corners()).with_shadow(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.20),
        Vec2::new(0.0, 4.0),
        8.0,
    ))
}

fn chrome_elevated() -> Background {
    Background::rounded(CARD_INK, card_corners()).with_shadow(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.28),
        Vec2::new(0.0, 12.0),
        20.0,
    ))
}

fn chrome_inset() -> Background {
    Background::rounded(CARD_INK, card_corners()).with_shadow(
        Shadow::drop(Color::rgba(0.0, 0.0, 0.0, 0.45), Vec2::new(0.0, 3.0), 8.0).inset(),
    )
}

/// Semi-transparent chrome fill: the shadow paints UNDER the fill, so
/// the halo doesn't bleed through. This is the case the
/// shape-buffer-lowering route gets wrong; the encoder path is correct.
fn chrome_translucent() -> Background {
    Background::rounded(CARD_INK.with_alpha(0.4), card_corners()).with_shadow(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.5),
        Vec2::new(0.0, 6.0),
        12.0,
    ))
}
