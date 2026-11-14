//! App-written effects: a background, the two stock press ripples, a ring past a card's edge and
//! water over a picture, each a WGSL `fn effect` the page loads once.

use crate::scaffold::api;
use crate::support;
use crate::support::{CELL_PADDING, DemoCell, Kit, Section};
use palantir::glam::Vec2;
use palantir::widget::EffectPaint;
use palantir::widget::{GlyphFont, PaintAnimation, PaintRepeat, Shape};
use palantir::{
    BackdropEffect, BackdropReach, Button, ButtonTheme, Configure, Corners, EffectError,
    EffectName, EffectParams, EffectSource, Panel, PressEffect, PressParams, Rect, ReducedEffect,
    RgbaF32, Sizing, Ui,
};
use std::time::Duration;

/// [`Ui::load_effect`] for the background's parameters, which its chip names.
type LoadSky = fn(&Ui, EffectName<Sky>, EffectSource<Sky>) -> Result<(), EffectError>;
/// [`EffectSource::reach`] over the water's parameters.
type WaterReach = fn(WaterSource, fn(&Water) -> BackdropReach) -> WaterSource;
type WaterSource = EffectSource<Water, BackdropEffect>;

/// The size of a demo cell.
const CELL: f32 = 260.0;

/// The colors a background blends.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, EffectParams)]
#[repr(C)]
struct Sky {
    low: RgbaF32,
    high: RgbaF32,
}

/// A ring's colors and how far its glow runs.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, EffectParams)]
#[repr(C)]
struct Ring {
    color_a: RgbaF32,
    color_b: RgbaF32,
    /// The halo's reach, logical px, where the glow has faded to nothing.
    reach: f32,
    /// The distance over which the glow falls to 1/e.
    glow: f32,
    pad0: f32,
    pad1: f32,
}

/// Water's tint and its rings.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable, EffectParams)]
#[repr(C)]
struct Water {
    tint: RgbaF32,
    /// How far a ring bends what is under it, logical px.
    amplitude: f32,
    /// The distance between two rings.
    wavelength: f32,
    pad0: f32,
    pad1: f32,
}

const AURORA: EffectName<Sky> = EffectName::named("showcase.aurora").expect("the name fits");
const DUSK: EffectName<Sky> = EffectName::named("showcase.dusk").expect("the name fits");
const RING: EffectName<Ring> = EffectName::named("showcase.ring").expect("the name fits");
const WATER: EffectName<Water, BackdropEffect> =
    EffectName::named("showcase.water").expect("the name fits");

/// The ring's glow and the water's bend, logical px: how far each paints or reads past its card.
const RING_REACH: f32 = 28.0;
const WATER_AMPLITUDE: f32 = 6.0;

pub(crate) fn build(ui: &mut Ui, k: Kit) {
    load(ui);
    Section::new("Background")
        .apis(&[
            api!(Ui::load_effect as LoadSky),
            api!(EffectPaint::new as fn(EffectName<Sky>, Sky) -> EffectPaint<Sky>),
            api!(
                EffectSource::power_saver
                    as fn(EffectSource<Sky>, ReducedEffect<Sky>) -> EffectSource<Sky>
            ),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Two colors swept by crossing waves, read from the effect's time. It holds still \
                 while motion is reduced, and under the power saver it paints a plain blend its \
                 source names in its place.",
            );
            DemoCell::new("aurora")
                .size(CELL * 2.0, CELL * 0.6)
                .show(ui, k, |ui| {
                    if let Some(shape) = ui.add_shape(
                        Shape::rect().corners(Corners::all(10.0)),
                        EffectPaint::new(AURORA, sky(k)),
                    ) {
                        shape.animate(looping(AURORA_LOOP_S));
                    }
                });
        });

    Section::new("Press ripples")
        .apis(&[
            api!(type PressEffect),
            api!(EffectName::RIPPLE as EffectName<PressParams>),
            api!(EffectName::SPARKLE as EffectName<PressParams>),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A theme slot names the effect each press draws, over the chrome and inside its \
                 corners. Press and hold: Material's flat ripple grows from the press toward the \
                 center, Android's sparkle ripple adds a ring of sparkles.",
            );
            // The ink of the shown scheme, so each scheme's presses read on its ground; the
            // sparkles at Android's α, 141/255.
            let color = k.t.ink.with_alpha(0.2);
            let highlight = k.t.ink.with_alpha(141.0 / 255.0);
            let flat = pressing(
                ui,
                PressEffect {
                    effect: EffectName::RIPPLE,
                    color,
                    highlight,
                    enter: Duration::from_millis(225),
                    exit: Duration::from_millis(150),
                },
            );
            let sparkle = pressing(
                ui,
                PressEffect {
                    effect: EffectName::SPARKLE,
                    color,
                    highlight,
                    enter: Duration::from_millis(450),
                    exit: Duration::from_millis(375),
                },
            );
            support::row(ui, |ui| {
                Button::new()
                    .id_salt("flat")
                    .label("flat ripple")
                    .size((160.0, 48.0))
                    .style(&flat)
                    .show(ui);
                Button::new()
                    .id_salt("sparkle")
                    .label("sparkle ripple")
                    .size((160.0, 48.0))
                    .style(&sparkle)
                    .show(ui);
            });
        });

    Section::new("Ring")
        .apis(&[api!(
            EffectPaint::outside as fn(EffectPaint<Ring>, f32) -> EffectPaint<Ring>
        )])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A halo paints outside its shape alone: the glow runs 28 px out from the card, \
                 and its color turns once every 6 s.",
            );
            DemoCell::new("ring").size(CELL, CELL).show(ui, k, |ui| {
                Panel::canvas()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| ring(ui, k));
            });
        });

    Section::new("Water")
        .apis(&[
            api!(
                EffectPaint::new
                    as fn(
                        EffectName<Water, BackdropEffect>,
                        Water,
                    ) -> EffectPaint<Water, BackdropEffect>
            ),
            api!(EffectSource::reach as WaterReach),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A backdrop effect reads what is painted under it. Rings run out from the center \
                 and bend the picture along their slope, reading up to 6 px past each pixel, the \
                 reach its source reads from the paint's amplitude.",
            );
            DemoCell::new("water").size(CELL, CELL).show(ui, k, |ui| {
                Panel::canvas()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| water(ui, k));
            });
        });
}

/// Load the page's effects, once for the app.
fn load(ui: &Ui) {
    if ui.has_effect(AURORA) {
        return;
    }
    let loads = [
        ui.load_effect(DUSK, EffectSource::new(include_str!("dusk.wgsl"))),
        ui.load_effect(
            AURORA,
            EffectSource::new(include_str!("aurora.wgsl")).power_saver(ReducedEffect::Other(DUSK)),
        ),
        ui.load_effect(RING, EffectSource::new(include_str!("ring.wgsl"))),
        // The waves bend each read up to their amplitude past the rect, so the capture reaches
        // as far.
        ui.load_effect(
            WATER,
            EffectSource::new(include_str!("water.wgsl"))
                .reach(|water: &Water| BackdropReach::Px(water.amplitude)),
        ),
    ];
    for load in loads {
        load.expect("the page's effects meet the contract");
    }
}

/// Each loop's length, seconds, which its WGSL's `LOOP_S` holds too, so its waves join with no
/// seam where the clock wraps.
const AURORA_LOOP_S: u64 = 20;
const RING_LOOP_S: u64 = 6;
const WATER_LOOP_S: u64 = 4;

/// A clock that loops every `period_s` seconds, for an effect that reads only its time.
fn looping(period_s: u64) -> PaintAnimation {
    PaintAnimation::clock()
        .with_period(Duration::from_secs(period_s))
        .with_repeat(PaintRepeat::Forever)
}

/// The aurora's colors: the accent over the page's ground.
const fn sky(k: Kit) -> Sky {
    Sky {
        low: k.t.ground,
        high: k.t.accent,
    }
}

/// The theme's button, its presses drawn in `press`, in each one's Material or Android times.
fn pressing(ui: &Ui, press: PressEffect) -> ButtonTheme {
    let mut theme = ui.theme().button.clone();
    theme.defaults.press = Some(press);
    theme
}

/// A card in the cell's middle, ringed by a halo that glows around it.
fn ring(ui: &mut Ui, k: Kit) {
    let inner = CELL - 2.0 * CELL_PADDING;
    let side = inner - 2.0 * (RING_REACH + 16.0);
    let card = Rect::new((inner - side) / 2.0, (inner - side) / 2.0, side, side);
    let corners = Corners::all(16.0);
    if let Some(shape) = ui.add_shape(
        Shape::rect().at(card).corners(corners),
        EffectPaint::new(
            RING,
            Ring {
                color_a: k.t.accent,
                color_b: RgbaF32::new(0.95, 0.35, 0.55, 1.0),
                reach: RING_REACH,
                glow: 8.0,
                pad0: 0.0,
                pad1: 0.0,
            },
        )
        .outside(RING_REACH),
    ) {
        shape.animate(looping(RING_LOOP_S));
    }
    ui.add_shape(Shape::rect().at(card).corners(corners), k.t.raised);
    let label = ui.intern("ring");
    ui.add_shape(
        Shape::text(label, GlyphFont::new(18.0))
            .at_origin(Vec2::new(card.min.x + 16.0, card.min.y + 32.0)),
        k.t.ink,
    );
}

/// Colored bars and a word under a backdrop of water.
fn water(ui: &mut Ui, k: Kit) {
    let size = CELL - 2.0 * CELL_PADDING;
    for (i, color) in (0..10).zip(k.swatch.all().iter().cycle()) {
        let x = i as f32 * size / 10.0;
        ui.add_shape(
            Shape::rect().at(Rect::new(x, 0.0, size / 20.0, size)),
            *color,
        );
    }
    let label = ui.intern("water");
    ui.add_shape(
        Shape::text(label, GlyphFont::new(40.0)).at_origin(Vec2::new(size * 0.22, size * 0.55)),
        k.t.ink,
    );
    let backdrop = Rect::new(size * 0.1, size * 0.1, size * 0.8, size * 0.8);
    if let Some(shape) = ui.add_shape(
        Shape::rect().at(backdrop).corners(Corners::all(20.0)),
        EffectPaint::new(
            WATER,
            Water {
                tint: RgbaF32::new(0.1, 0.45, 0.7, 0.18),
                amplitude: WATER_AMPLITUDE,
                wavelength: 36.0,
                pad0: 0.0,
                pad1: 0.0,
            },
        ),
    ) {
        shape.animate(looping(WATER_LOOP_S));
    }
}

#[cfg(test)]
mod tests {
    use crate::pages::effects::{AURORA_LOOP_S, RING_LOOP_S, WATER_LOOP_S};

    /// Each effect's WGSL loops over the clock's period, so its waves join where the clock wraps.
    #[test]
    fn each_wgsl_loops_over_its_clock() {
        for (wgsl, loop_s) in [
            (include_str!("aurora.wgsl"), AURORA_LOOP_S),
            (include_str!("ring.wgsl"), RING_LOOP_S),
            (include_str!("water.wgsl"), WATER_LOOP_S),
        ] {
            let line = format!("const LOOP_S: f32 = {loop_s}.0;");
            assert!(wgsl.contains(&line), "{line} in {wgsl}");
        }
    }
}
