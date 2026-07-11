//! Drop-shadow showcase. The first two rows drop shadows under a
//! rounded card via direct `Shape::Shadow` pushes, then paint the card
//! on top — exercising the per-corner SDF, the σ → 0 sharp fallback,
//! and multi-shadow stacking via record order. The last row attaches
//! the shadow to widget chrome (`Background { shadow }`), which routes
//! through the encoder's chrome branch — painted *before* the rect
//! fill, so it composes correctly under semi-transparent fills.
//! Light cell surfaces because black-on-dark shadows don't read.

use crate::showcase::support;
use crate::showcase::support::{cell_row, demo_cell_light};
use aperture::{Background, Color, Configure, Corners, Panel, Rect, Shadow, Shape, Sizing, Ui};
use glam::Vec2;

pub fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        cell_row(ui, "row1", |ui| {
            demo_cell_light(ui, "soft — elevation 2", soft);
            demo_cell_light(ui, "elevated — offset 12, blur 20", elevated);
            demo_cell_light(ui, "tight — button rest state", tight);
            demo_cell_light(ui, "sharp — σ→0 fallback", sharp);
        });
        cell_row(ui, "row2", |ui| {
            demo_cell_light(ui, "glow — colored, zero offset", glow);
            demo_cell_light(ui, "inset — pressed feel", inset);
            demo_cell_light(ui, "stacked — CSS box-shadow a, b, c", stacked);
        });
        cell_row(ui, "row3", |ui| {
            demo_cell_light(ui, "chrome — soft", |ui| chrome_card(ui, chrome_soft()));
            demo_cell_light(ui, "chrome — elevated", |ui| {
                chrome_card(ui, chrome_elevated());
            });
            demo_cell_light(ui, "chrome — inset", |ui| chrome_card(ui, chrome_inset()));
            demo_cell_light(ui, "chrome — translucent fill", |ui| {
                chrome_card(ui, chrome_translucent());
            });
        });
    });
}

fn shadow_shape(s: Shadow) -> Shape<'static> {
    Shape::shadow(s).at(card_rect()).corners(corners())
}

fn card_rect() -> Rect {
    Rect::new(20.0, 20.0, 160.0, 100.0)
}

fn corners() -> Corners {
    Corners::all(12.0)
}

fn card_fill(ui: &mut Ui) {
    ui.add_shape(
        Shape::rect(card_rect())
            .fill(Color::rgb(0.95, 0.95, 0.97))
            .corners(corners()),
    );
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
/// order, the deepest first; composer batches them onto one draw.
fn stacked(ui: &mut Ui) {
    ui.add_shape(shadow_shape(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.18),
        Vec2::new(0.0, 24.0),
        32.0,
    )));
    ui.add_shape(shadow_shape(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.22),
        Vec2::new(0.0, 8.0),
        10.0,
    )));
    ui.add_shape(shadow_shape(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.30),
        Vec2::new(0.0, 1.0),
        2.0,
    )));
    card_fill(ui);
}

/// A centered card painted via `Background` (fill + radius + shadow)
/// instead of shape pushes — the encoder emits the shadow before the
/// chrome rect.
fn chrome_card(ui: &mut Ui, bg: Background) {
    Panel::zstack()
        .auto_id()
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
    Background::rounded(Color::rgb(0.95, 0.95, 0.97), Corners::all(12.0)).with_shadow(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.20),
        Vec2::new(0.0, 4.0),
        8.0,
    ))
}

fn chrome_elevated() -> Background {
    Background::rounded(Color::rgb(0.95, 0.95, 0.97), Corners::all(12.0)).with_shadow(Shadow::drop(
        Color::rgba(0.0, 0.0, 0.0, 0.28),
        Vec2::new(0.0, 12.0),
        20.0,
    ))
}

fn chrome_inset() -> Background {
    Background::rounded(Color::rgb(0.95, 0.95, 0.97), Corners::all(12.0)).with_shadow(
        Shadow::drop(Color::rgba(0.0, 0.0, 0.0, 0.45), Vec2::new(0.0, 3.0), 8.0).inset(),
    )
}

/// Semi-transparent chrome fill: the shadow paints UNDER the fill,
/// so the halo doesn't bleed through. This is the case the
/// shape-buffer-lowering route gets wrong; encoder-path is correct.
fn chrome_translucent() -> Background {
    Background::rounded(Color::rgba(0.95, 0.95, 0.97, 0.4), Corners::all(12.0)).with_shadow(
        Shadow::drop(Color::rgba(0.0, 0.0, 0.0, 0.5), Vec2::new(0.0, 6.0), 12.0),
    )
}
