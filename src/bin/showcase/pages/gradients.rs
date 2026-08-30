//! Gradient brushes. Each tile paints a `Frame` whose `Background.fill`
//! carries one gradient variant, so the full path — composer, atlas
//! bake, shader sample, premultiplied blend — runs every frame. Stop
//! colours stay vivid so spread and interpolation differences read at a
//! glance.

use crate::support;
use crate::support::{demo_cell, section, tiles};
use palantir::{
    Background, Brush, ColorU8, Configure, ConicGradient, Corners, Frame, Interp, LinearGradient,
    RadialGradient, Sizing, Spread, Stop, Ui, Vec2,
};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

const NAVY: ColorU8 = ColorU8::hex(0x1a1a2e);
const BLUE: ColorU8 = ColorU8::hex(0x4c5cdb);
const ORANGE: ColorU8 = ColorU8::hex(0xff7e44);
const YELLOW: ColorU8 = ColorU8::hex(0xfacc15);
const RED: ColorU8 = ColorU8::hex(0xff5e44);
const GREEN: ColorU8 = ColorU8::hex(0x46c46c);

pub(crate) fn build(ui: &mut Ui) {
    section(ui, "linear — angle in radians from the +x axis", |ui| {
        tiles(ui, |ui| {
            demo_cell(ui, "horizontal", horizontal);
            demo_cell(ui, "vertical", vertical);
            demo_cell(ui, "45°", diagonal);
        });
    });

    section(
        ui,
        "radial — centre and per-axis radius in unit space",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "centred, circular", radial_centered);
                demo_cell(ui, "offset centre, three stops", radial_offset);
                demo_cell(ui, "elliptical radius", radial_ellipse);
            });
        },
    );

    section(
        ui,
        "conic — sweep about a centre from a start angle",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "colour wheel", conic_wheel);
                demo_cell(ui, "rotated 90°", conic_rotated);
            });
        },
    );

    section(
        ui,
        "spread & interpolation — what happens outside the stop range, and the \
         space stops blend in",
        |ui| {
            tiles(ui, |ui| {
                demo_cell(ui, "Spread::Reflect — rings mirror out", reflect);
                demo_cell(ui, "Spread::Repeat — stripes", repeat);
                demo_cell(ui, "Interp::Oklab — perceptual midpoint", oklab);
            });
        },
    );
}

fn filled(brush: Brush) -> Background {
    Background {
        fill: brush,
        corners: Corners::all(support::RADIUS),
        ..Default::default()
    }
}

fn gradient_frame(ui: &mut Ui, bg: Background) {
    Frame::new()
        .size((Sizing::FILL, Sizing::FILL))
        .background(bg)
        .show(ui);
}

fn horizontal(ui: &mut Ui) {
    gradient_frame(
        ui,
        filled(Brush::Linear(LinearGradient::two_stop(0.0, NAVY, BLUE))),
    );
}

fn vertical(ui: &mut Ui) {
    gradient_frame(
        ui,
        filled(Brush::Linear(LinearGradient::two_stop(
            FRAC_PI_2, NAVY, BLUE,
        ))),
    );
}

fn diagonal(ui: &mut Ui) {
    gradient_frame(
        ui,
        filled(Brush::Linear(LinearGradient::two_stop(
            FRAC_PI_4, ORANGE, YELLOW,
        ))),
    );
}

/// Radial centred at (0.5, 0.5) with a circular radius of 0.5 (touches
/// the bounding square mid-edges). Bright core, dark rim.
fn radial_centered(ui: &mut Ui) {
    gradient_frame(
        ui,
        filled(Brush::Radial(RadialGradient::two_stop_centered(
            YELLOW, NAVY,
        ))),
    );
}

/// Off-centre radial — the bright core hugs the top-left, the rim
/// reaches further along the diagonal.
fn radial_offset(ui: &mut Ui) {
    let g = RadialGradient::new(
        Vec2::new(0.25, 0.3),
        Vec2::new(0.9, 0.9),
        [
            Stop::new(0.0, ORANGE),
            Stop::new(0.6, RED),
            Stop::new(1.0, NAVY),
        ],
    );
    gradient_frame(ui, filled(Brush::Radial(g)));
}

/// Elliptical radius — wider horizontally than vertically. Stretches
/// the core into an oval.
fn radial_ellipse(ui: &mut Ui) {
    let g = RadialGradient::new(
        Vec2::splat(0.5),
        Vec2::new(0.55, 0.25),
        [Stop::new(0.0, GREEN), Stop::new(1.0, NAVY)],
    );
    gradient_frame(ui, filled(Brush::Radial(g)));
}

/// Conic colour-wheel centred in the tile. Six saturated stops sweep
/// CCW from the positive-x axis, with stop 0 == stop 1 so the seam
/// hides at angle 0.
fn conic_wheel(ui: &mut Ui) {
    let g = ConicGradient::new(
        Vec2::splat(0.5),
        0.0,
        [
            Stop::new(0.0, RED),
            Stop::new(0.166, YELLOW),
            Stop::new(0.333, GREEN),
            Stop::new(0.5, ColorU8::hex(0x22ccdd)),
            Stop::new(0.666, BLUE),
            Stop::new(0.833, ColorU8::hex(0xd14fdf)),
            Stop::new(1.0, RED),
        ],
    );
    gradient_frame(ui, filled(Brush::Conic(g)));
}

/// Conic with a non-zero `start_angle` — the same sweep, rotated. Pin
/// for the `(theta - start_angle) / TAU` shader math.
fn conic_rotated(ui: &mut Ui) {
    let g = ConicGradient::new(
        Vec2::splat(0.5),
        FRAC_PI_2,
        [
            Stop::new(0.0, NAVY),
            Stop::new(0.5, YELLOW),
            Stop::new(1.0, NAVY),
        ],
    );
    gradient_frame(ui, filled(Brush::Conic(g)));
}

/// Radial whose stops end at r = 0.25; everything beyond mirrors back in.
fn reflect(ui: &mut Ui) {
    let g = RadialGradient::new(
        Vec2::splat(0.5),
        Vec2::splat(0.25),
        [Stop::new(0.0, BLUE), Stop::new(1.0, ORANGE)],
    )
    .with_spread(Spread::Reflect);
    gradient_frame(ui, filled(Brush::Radial(g)));
}

/// Linear whose stops end at 25% of the axis; the ramp tiles from there.
fn repeat(ui: &mut Ui) {
    let g = LinearGradient::builder(0.0)
        .stop(0.0, NAVY)
        .stop(0.25, BLUE)
        .with_spread(Spread::Repeat)
        .build();
    gradient_frame(ui, filled(Brush::Linear(g)));
}

/// Red to green in Oklab — no muddy grey through the middle the way a
/// straight linear-RGB blend gives.
fn oklab(ui: &mut Ui) {
    let g = LinearGradient::two_stop(0.0, RED, GREEN).with_interp(Interp::Oklab);
    gradient_frame(ui, filled(Brush::Linear(g)));
}
