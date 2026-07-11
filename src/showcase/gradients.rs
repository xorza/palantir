//! Gradient showcase. Three rows × three cells; each cell paints a
//! `Frame` with a `Background.fill` carrying one of the gradient
//! variants so the full path (composer → atlas → shader → premul) runs
//! every frame. Stop colours stay vivid so spread/interp differences
//! read at a glance.

use crate::showcase::support;
use crate::showcase::support::{cell_row, demo_cell};
use aperture::{
    Background, Brush, ColorU8, Configure, ConicGradient, Corners, Frame, Interp, LinearGradient,
    Panel, RadialGradient, Sizing, Spread, Stop, Ui,
};
use glam::Vec2;

const NAVY: ColorU8 = ColorU8::hex(0x1a1a2e);
const BLUE: ColorU8 = ColorU8::hex(0x4c5cdb);
const ORANGE: ColorU8 = ColorU8::hex(0xff7e44);
const YELLOW: ColorU8 = ColorU8::hex(0xfacc15);
const RED: ColorU8 = ColorU8::hex(0xff5e44);
const GREEN: ColorU8 = ColorU8::hex(0x46c46c);

pub fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        cell_row(ui, "row1", |ui| {
            demo_cell(ui, "linear — horizontal", horizontal);
            demo_cell(ui, "linear — vertical", vertical);
            demo_cell(ui, "linear — 45°", diagonal);
        });
        cell_row(ui, "row2", |ui| {
            demo_cell(ui, "radial — centred", radial_centered);
            demo_cell(ui, "radial — offset", radial_offset);
            demo_cell(ui, "radial — ellipse", radial_ellipse);
        });
        cell_row(ui, "row3", |ui| {
            demo_cell(ui, "conic — wheel", conic_wheel);
            demo_cell(ui, "conic — rotated 90°", conic_rotated);
            demo_cell(ui, "spread Reflect / Repeat · Oklab", spread_and_interp);
        });
    });
}

fn filled(brush: Brush) -> Background {
    Background {
        fill: brush,
        corners: Corners::all(8.0),
        ..Default::default()
    }
}

fn linear_filled(g: LinearGradient) -> Background {
    filled(Brush::Linear(g))
}

fn radial_filled(g: RadialGradient) -> Background {
    filled(Brush::Radial(g))
}

fn conic_filled(g: ConicGradient) -> Background {
    filled(Brush::Conic(g))
}

fn gradient_frame(ui: &mut Ui, bg: Background) {
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(bg)
        .show(ui);
}

fn horizontal(ui: &mut Ui) {
    gradient_frame(ui, linear_filled(LinearGradient::two_stop(0.0, NAVY, BLUE)));
}

fn vertical(ui: &mut Ui) {
    gradient_frame(
        ui,
        linear_filled(LinearGradient::two_stop(
            std::f32::consts::FRAC_PI_2,
            NAVY,
            BLUE,
        )),
    );
}

fn diagonal(ui: &mut Ui) {
    gradient_frame(
        ui,
        linear_filled(LinearGradient::two_stop(
            std::f32::consts::FRAC_PI_4,
            ORANGE,
            YELLOW,
        )),
    );
}

/// Radial centred at (0.5, 0.5) with a circular radius of 0.5 (touches
/// the bounding square mid-edges). Bright core, dark rim.
fn radial_centered(ui: &mut Ui) {
    gradient_frame(
        ui,
        radial_filled(RadialGradient::two_stop_centered(YELLOW, NAVY)),
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
    gradient_frame(ui, radial_filled(g));
}

/// Elliptical radius — wider horizontally than vertically. Stretches
/// the core into an oval.
fn radial_ellipse(ui: &mut Ui) {
    let g = RadialGradient::new(
        Vec2::splat(0.5),
        Vec2::new(0.55, 0.25),
        [Stop::new(0.0, GREEN), Stop::new(1.0, NAVY)],
    );
    gradient_frame(ui, radial_filled(g));
}

/// Conic colour-wheel centred in the cell. Six saturated stops sweep
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
    gradient_frame(ui, conic_filled(g));
}

/// Conic with a non-zero `start_angle` — the same sweep, rotated. Pin
/// for the `(theta - start_angle) / TAU` shader math.
fn conic_rotated(ui: &mut Ui) {
    let g = ConicGradient::new(
        Vec2::splat(0.5),
        std::f32::consts::FRAC_PI_2,
        [
            Stop::new(0.0, NAVY),
            Stop::new(0.5, YELLOW),
            Stop::new(1.0, NAVY),
        ],
    );
    gradient_frame(ui, conic_filled(g));
}

/// Three stacked cells: a `Reflect` radial (rings outside the radius
/// mirror back in), a `Repeat` linear (stripes), and an Oklab radial
/// (smooth perceptual midpoint). Confirms `Spread` + `Interp` reach
/// non-linear variants.
fn spread_and_interp(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(4.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            // Reflect radial: ring at r=0.25 mirrors out.
            let r = RadialGradient::new(
                Vec2::splat(0.5),
                Vec2::splat(0.25),
                [Stop::new(0.0, BLUE), Stop::new(1.0, ORANGE)],
            )
            .with_spread(Spread::Reflect);
            Frame::new()
                .id_salt("reflect-radial")
                .size((Sizing::FILL, Sizing::FILL))
                .background(radial_filled(r))
                .show(ui);

            // Repeat linear stripes.
            let mut l = LinearGradient::two_stop(0.0, NAVY, BLUE).with_spread(Spread::Repeat);
            l.stops[1].offset_u8 = (0.25 * 255.0 + 0.5) as u8;
            Frame::new()
                .id_salt("repeat-linear")
                .size((Sizing::FILL, Sizing::FILL))
                .background(linear_filled(l))
                .show(ui);

            // Oklab red→green linear — perceptual midpoint.
            let i = LinearGradient::two_stop(0.0, RED, GREEN).with_interp(Interp::Oklab);
            Frame::new()
                .id_salt("oklab-linear")
                .size((Sizing::FILL, Sizing::FILL))
                .background(linear_filled(i))
                .show(ui);
        });
}
