//! Drop shadows from both directions. The first two sections push
//! `Shape::Shadow` directly and paint the card on top — exercising the
//! per-corner SDF, the σ → 0 sharp fallback, and multi-shadow stacking
//! by record order. The third pairs each shape-pushed shadow with the
//! same shadow on widget chrome (`Background { shadow }`), which routes
//! through the encoder's chrome branch and paints *before* the rect fill,
//! so it composes correctly under a semi-transparent fill.
//!
//! Every tile is on the bright surface: black-on-dark shadows don't read.

use crate::support::{CELL_PADDING, api, demo_cell_light, note, section, tiles};
use palantir::widget::{ShadowShape, Shape};
use palantir::{
    Background, Block, Configure, Corners, Panel, Rect, RgbaF32, Shadow, Sizing, Ui, Vec2,
};

const CARD: Rect = Rect::new(22.0, 28.0, 124.0, 86.0);
const CARD_INK: RgbaF32 = RgbaF32::hex(0xf2f2f7);

fn card_corners() -> Corners {
    Corners::all(12.0)
}

pub(crate) fn build(ui: &mut Ui) {
    section(ui, "Elevation", &[api!(Shadow::drop)], |ui| {
        note(
            ui,
            "A drop shadow pushed as a shape under a rounded card, from a button at rest up \
             to a floating panel, and the zero-blur case.",
        );
        tiles(ui, |ui| {
            demo_cell_light(ui, "tight — a button at rest", tight);
            demo_cell_light(ui, "soft — elevation 2", soft);
            demo_cell_light(ui, "elevated — offset 12, blur 20", elevated);
            demo_cell_light(ui, "sharp — blur near 0", sharp);
        });
    });

    section(
        ui,
        "Colour and stacking",
        &[api!(Shadow::with_spread)],
        |ui| {
            note(
                ui,
                "A coloured glow with no offset, and three shadows stacked in record order, \
                 as a CSS box-shadow list stacks them.",
            );
            tiles(ui, |ui| {
                demo_cell_light(ui, "glow — coloured, spread 2", glow);
                demo_cell_light(ui, "stacked — three layers", stacked);
            });
        },
    );

    section(
        ui,
        "Shape or chrome",
        &[api!(Background::with_shadow), api!(Shadow::inset)],
        |ui| {
            note(
                ui,
                "Each pair paints one shadow two ways: pushed as a shape with the card, and \
                 set on a widget's Background, which paints its shadow before its fill. The \
                 two should match.",
            );
            tiles(ui, |ui| {
                demo_cell_light(ui, "soft — shape", soft);
                demo_cell_light(ui, "soft — chrome", |ui| {
                    chrome_card(ui, chrome(soft_shadow()));
                });
                demo_cell_light(ui, "inset — shape", inset);
                demo_cell_light(ui, "inset — chrome", |ui| {
                    chrome_card(ui, chrome(inset_shadow()));
                });
                demo_cell_light(ui, "translucent fill — chrome", |ui| {
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

/// The shadows this page paints more than once, each written once. The
/// shape route and the chrome route have to paint the *same* shadow or
/// the comparison the page makes says nothing, so neither route spells
/// the parameters itself.
const fn soft_shadow() -> Shadow {
    Shadow::drop(
        RgbaF32::srgba(0.0, 0.0, 0.0, 0.20),
        Vec2::new(0.0, 4.0),
        8.0,
    )
}

const fn elevated_shadow() -> Shadow {
    Shadow::drop(
        RgbaF32::srgba(0.0, 0.0, 0.0, 0.28),
        Vec2::new(0.0, 12.0),
        20.0,
    )
}

const fn inset_shadow() -> Shadow {
    Shadow::drop(
        RgbaF32::srgba(0.0, 0.0, 0.0, 0.45),
        Vec2::new(0.0, 3.0),
        8.0,
    )
    .inset()
}

/// Standard soft drop shadow — Material Design "elevation 2".
fn soft(ui: &mut Ui) {
    ui.add_shape(shadow_shape(soft_shadow()));
    card_fill(ui);
}

/// Heavier drop, larger blur — "elevation 8" look.
fn elevated(ui: &mut Ui) {
    ui.add_shape(shadow_shape(elevated_shadow()));
    card_fill(ui);
}

/// Tight, dense shadow hugging the shape — UI button rest state.
fn tight(ui: &mut Ui) {
    ui.add_shape(shadow_shape(Shadow::drop(
        RgbaF32::srgba(0.0, 0.0, 0.0, 0.35),
        Vec2::new(0.0, 1.0),
        2.0,
    )));
    card_fill(ui);
}

/// σ = 0 — sharp drop. Should match the rounded-rect SDF exactly,
/// shifted by `offset`. Pins the degenerate-blur code path visually.
fn sharp(ui: &mut Ui) {
    ui.add_shape(shadow_shape(Shadow::drop(
        RgbaF32::srgba(0.0, 0.0, 0.0, 1.0),
        Vec2::new(6.0, 6.0),
        2.0,
    )));
    card_fill(ui);
}

/// Coloured glow, zero offset — bloom feel.
fn glow(ui: &mut Ui) {
    ui.add_shape(shadow_shape(
        Shadow::drop(RgbaF32::srgba(0.4, 0.6, 1.0, 0.6), Vec2::ZERO, 18.0).with_spread(2.0),
    ));
    card_fill(ui);
}

/// Inset shadow — interior darkening, pressed-button feel.
fn inset(ui: &mut Ui) {
    card_fill(ui);
    ui.add_shape(shadow_shape(inset_shadow()));
}

/// Multi-shadow stack — CSS `box-shadow: a, b, c`. Pushed in record
/// order, the deepest first; the composer batches them onto one draw.
fn stacked(ui: &mut Ui) {
    for (dy, blur, alpha) in [(18.0, 24.0, 0.18), (8.0, 10.0, 0.22), (1.0, 2.0, 0.30)] {
        ui.add_shape(shadow_shape(Shadow::drop(
            RgbaF32::srgba(0.0, 0.0, 0.0, alpha),
            Vec2::new(0.0, dy),
            blur,
        )));
    }
    card_fill(ui);
}

/// The card painted via `Background` (fill + radius + shadow) instead of
/// shape pushes — the encoder emits the shadow before the chrome rect.
/// Placed on [`CARD`] exactly, so it sits where its shape twin does: the
/// shapes are in the cell's own coordinates, and the canvas starts inside
/// the cell's padding.
fn chrome_card(ui: &mut Ui, bg: Background) {
    Panel::canvas()
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Block::new()
                .position((CARD.min.x - CELL_PADDING, CARD.min.y - CELL_PADDING))
                .size((Sizing::fixed(CARD.size.w), Sizing::fixed(CARD.size.h)))
                .background(bg)
                .show(ui);
        });
}

fn chrome(shadow: Shadow) -> Background {
    Background::rounded(CARD_INK, card_corners()).with_shadow(shadow)
}

/// Semi-transparent chrome fill: the shadow paints UNDER the fill, so
/// the halo doesn't bleed through. This is the case the
/// shape-buffer-lowering route gets wrong; the encoder path is correct.
fn chrome_translucent() -> Background {
    Background::rounded(CARD_INK.with_alpha(0.4), card_corners()).with_shadow(Shadow::drop(
        RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
        Vec2::new(0.0, 6.0),
        12.0,
    ))
}
