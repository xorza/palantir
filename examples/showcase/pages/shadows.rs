//! Drop shadows pushed as `Shape::Shadow` and as widget chrome
//! (`Chrome { shadow }`), side by side. Tiles sit on a bright surface;
//! black-on-dark shadows don't read.

use crate::scaffold::api;
use crate::support;
use crate::support::{CELL_PADDING, DemoCell, Kit, Section, Surface};
use palantir::glam::Vec2;
use palantir::widget::{ShadowShape, Shape};
use palantir::{Block, Chrome, Configure, Corners, Panel, Rect, RgbaF32, Shadow, Sizing, Ui};

const CARD: Rect = Rect::new(22.0, 28.0, 124.0, 86.0);
const CARD_INK: RgbaF32 = RgbaF32::hex(0xf2f2f7);

fn card_corners() -> Corners {
    Corners::all(12.0)
}

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    Section::new("Elevation")
        .apis(&[api!(Shadow::drop)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A drop shadow pushed as a shape under a rounded card, from a button at rest up \
             to a floating panel, and the zero-blur case.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("tight — a button at rest")
                    .surface(Surface::LightWell)
                    .show(ui, k, tight);
                DemoCell::new("soft — elevation 2")
                    .surface(Surface::LightWell)
                    .show(ui, k, soft);
                DemoCell::new("elevated — offset 12, blur 20")
                    .surface(Surface::LightWell)
                    .show(ui, k, elevated);
                DemoCell::new("sharp — blur near 0")
                    .surface(Surface::LightWell)
                    .show(ui, k, sharp);
            });
        });

    Section::new("Color and stacking")
        .apis(&[api!(Shadow::with_spread)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A colored glow with no offset, and three shadows stacked in record order, \
                 as a CSS box-shadow list stacks them.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("glow — colored, spread 2")
                    .surface(Surface::LightWell)
                    .show(ui, k, glow);
                DemoCell::new("stacked — three layers")
                    .surface(Surface::LightWell)
                    .show(ui, k, stacked);
            });
        });

    Section::new("Shape or chrome")
        .apis(&[api!(Chrome::with_shadow), api!(Shadow::inset)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Each pair paints one shadow two ways: pushed as a shape with the card, and \
                 set on a widget's Chrome, which paints a drop shadow under its fill and \
                 an inset one over it, as CSS does. The two should match.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("soft — shape")
                    .surface(Surface::LightWell)
                    .show(ui, k, soft);
                DemoCell::new("soft — chrome")
                    .surface(Surface::LightWell)
                    .show(ui, k, |ui| {
                        chrome_card(ui, chrome(soft_shadow()));
                    });
                DemoCell::new("inset — shape")
                    .surface(Surface::LightWell)
                    .show(ui, k, inset);
                DemoCell::new("inset — chrome")
                    .surface(Surface::LightWell)
                    .show(ui, k, |ui| {
                        chrome_card(ui, chrome(inset_shadow()));
                    });
                DemoCell::new("translucent fill — chrome")
                    .surface(Surface::LightWell)
                    .show(ui, k, |ui| {
                        chrome_card(ui, chrome_translucent());
                    });
            });
        });
}

fn shadow_shape(s: Shadow) -> ShadowShape {
    Shape::shadow(s).at(CARD).corners(card_corners())
}

fn card_fill(ui: &mut Ui) {
    ui.add_shape(Shape::rect(CARD).fill(CARD_INK).corners(card_corners()));
}

/// Shadows painted more than once, so the shape and chrome routes use identical parameters.
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

/// σ = 0 sharp drop; should match the rounded-rect SDF shifted by `offset`.
fn sharp(ui: &mut Ui) {
    ui.add_shape(shadow_shape(Shadow::drop(
        RgbaF32::srgba(0.0, 0.0, 0.0, 1.0),
        Vec2::new(6.0, 6.0),
        2.0,
    )));
    card_fill(ui);
}

/// Colored glow, zero offset — bloom feel.
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

/// Multi-shadow stack, pushed deepest first; the composer batches them into one draw.
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

/// The card via `Chrome`: drop shadow emitted before the chrome rect, inset after. Placed on [`CARD`] exactly, so it lines up with its shape twin.
fn chrome_card(ui: &mut Ui, chrome: Chrome) {
    Panel::canvas()
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Block::new()
                .pos((CARD.min.x - CELL_PADDING, CARD.min.y - CELL_PADDING))
                .size((Sizing::fixed(CARD.size.w), Sizing::fixed(CARD.size.h)))
                .chrome(chrome)
                .show(ui);
        });
}

fn chrome(shadow: Shadow) -> Chrome {
    Chrome::rounded(CARD_INK, card_corners()).with_shadow(shadow)
}

/// Semi-transparent fill: the drop shadow is clipped inside the casting box (as CSS does), so the halo shows around, not through.
fn chrome_translucent() -> Chrome {
    Chrome::rounded(CARD_INK.with_alpha(0.4), card_corners()).with_shadow(Shadow::drop(
        RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
        Vec2::new(0.0, 6.0),
        12.0,
    ))
}
