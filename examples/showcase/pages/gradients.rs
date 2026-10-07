//! Gradient brushes. Each tile paints a `Block` whose `Background.fill` carries one gradient variant, exercising composer, atlas bake, shader sample and premultiplied blend every frame. Vivid stops make spread and interpolation differences read at a glance.

use crate::support;
use crate::support::{api, demo_cell, note, section, tiles};
use palantir::{
    Background, Block, Brush, Configure, ConicGradient, Corners, Interpolation, LinearGradient,
    RadialGradient, RgbaF32, Sizing, Spread, Stop, Ui, Vec2,
};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

const NAVY: RgbaF32 = RgbaF32::hex(0x1a1a2e);
const BLUE: RgbaF32 = RgbaF32::hex(0x4c5cdb);
const ORANGE: RgbaF32 = RgbaF32::hex(0xff7e44);
const YELLOW: RgbaF32 = RgbaF32::hex(0xfacc15);
const RED: RgbaF32 = RgbaF32::hex(0xff5e44);
const GREEN: RgbaF32 = RgbaF32::hex(0x46c46c);

pub(crate) fn build(ui: &mut Ui) {
    section(
        ui,
        "Linear",
        &[api!(type LinearGradient), api!(LinearGradient::two_stop)],
        |ui| {
            note(ui, "The angle is in radians from the +x axis.");
            tiles(ui, |ui| {
                demo_cell(ui, "horizontal", horizontal);
                demo_cell(ui, "vertical", vertical);
                demo_cell(ui, "45°", diagonal);
            });
        },
    );

    section(ui, "Radial", &[api!(type RadialGradient)], |ui| {
        note(
            ui,
            "A centre and a radius per axis, in the unit square of the box.",
        );
        tiles(ui, |ui| {
            demo_cell(ui, "centred, circular", radial_centered);
            demo_cell(ui, "offset centre, three stops", radial_offset);
            demo_cell(ui, "elliptical radius", radial_ellipse);
        });
    });

    section(ui, "Conic", &[api!(type ConicGradient)], |ui| {
        note(ui, "A sweep about a centre from a start angle.");
        tiles(ui, |ui| {
            demo_cell(ui, "colour wheel", conic_wheel);
            demo_cell(ui, "rotated 90°", conic_rotated);
        });
    });

    section(
        ui,
        "Spread and interpolation",
        &[api!(type Spread), api!(type Interpolation)],
        |ui| {
            note(
                ui,
                "What happens outside the stop range, and the colour space the stops \
                 blend in.",
            );
            tiles(ui, |ui| {
                demo_cell(ui, "Spread::Reflect — rings mirror out", reflect);
                demo_cell(ui, "Spread::Repeat — rings", repeat);
                demo_cell(ui, "Interpolation::Oklab — perceptual midpoint", oklab);
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
    Block::new()
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

/// Radial centred at (0.5, 0.5), radius 0.5 (touches the square's mid-edges); bright core, dark rim.
fn radial_centered(ui: &mut Ui) {
    gradient_frame(
        ui,
        filled(Brush::Radial(RadialGradient::two_stop(YELLOW, NAVY))),
    );
}

/// Off-centre radial: the core hugs the top-left, the rim reaches further along the diagonal.
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

/// Elliptical radius, wider than tall; stretches the core into an oval.
fn radial_ellipse(ui: &mut Ui) {
    let g = RadialGradient::new(
        Vec2::splat(0.5),
        Vec2::new(0.55, 0.25),
        [Stop::new(0.0, GREEN), Stop::new(1.0, NAVY)],
    );
    gradient_frame(ui, filled(Brush::Radial(g)));
}

/// Conic colour wheel: six saturated stops sweep clockwise from +x, stop 0 == stop 1 so the seam hides at angle 0.
fn conic_wheel(ui: &mut Ui) {
    let g = ConicGradient::new(
        Vec2::splat(0.5),
        0.0,
        [
            Stop::new(0.0, RED),
            Stop::new(0.166, YELLOW),
            Stop::new(0.333, GREEN),
            Stop::new(0.5, RgbaF32::hex(0x22ccdd)),
            Stop::new(0.666, BLUE),
            Stop::new(0.833, RgbaF32::hex(0xd14fdf)),
            Stop::new(1.0, RED),
        ],
    );
    gradient_frame(ui, filled(Brush::Conic(g)));
}

/// Conic with non-zero `start_angle`: the same sweep rotated; pins the `(theta - start_angle) / TAU` shader math.
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

/// Radial whose stops end at r = 0.125; past it the ramp starts over, giving rings. A linear axis spans the box exactly, so `t` never leaves 0..1 and no spread mode shows.
fn repeat(ui: &mut Ui) {
    let g = RadialGradient::new(
        Vec2::splat(0.5),
        Vec2::splat(0.125),
        [Stop::new(0.0, NAVY), Stop::new(1.0, BLUE)],
    )
    .with_spread(Spread::Repeat);
    gradient_frame(ui, filled(Brush::Radial(g)));
}

/// Red to green in Oklab: no muddy grey midpoint as in a linear-RGB blend.
fn oklab(ui: &mut Ui) {
    let g = LinearGradient::two_stop(0.0, RED, GREEN).with_interpolation(Interpolation::Oklab);
    gradient_frame(ui, filled(Brush::Linear(g)));
}
