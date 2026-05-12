//! Gradient showcase. Three rows × three cells; each cell paints a
//! sized `Frame` with a `Background.fill` carrying one of the gradient
//! variants so the full path (composer → atlas → shader → premul) runs
//! every frame. Stop colours stay vivid so spread/interp differences
//! read at a glance.

use glam::Vec2;
use palantir::{
    Background, Brush, Configure, ConicGradient, Corners, Frame, Interp, LinearGradient, Panel,
    RadialGradient, Sizing, Spread, Srgb8, Ui,
};

const NAVY: Srgb8 = Srgb8::hex(0x1a1a2e);
const BLUE: Srgb8 = Srgb8::hex(0x4c5cdb);
const ORANGE: Srgb8 = Srgb8::hex(0xff7e44);
const YELLOW: Srgb8 = Srgb8::hex(0xfacc15);
const RED: Srgb8 = Srgb8::hex(0xff5e44);
const GREEN: Srgb8 = Srgb8::hex(0x46c46c);

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
                    cell(ui, "horizontal", horizontal);
                    cell(ui, "vertical", vertical);
                    cell(ui, "45-degree", diagonal);
                });
            Panel::hstack()
                .id_salt("row2")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "radial-centred", radial_centered);
                    cell(ui, "radial-offset", radial_offset);
                    cell(ui, "radial-ellipse", radial_ellipse);
                });
            Panel::hstack()
                .id_salt("row3")
                .gap(16.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    cell(ui, "conic-wheel", conic_wheel);
                    cell(ui, "conic-rotated", conic_rotated);
                    cell(ui, "spread+interp", spread_and_interp);
                });
        });
}

/// One panel cell containing a single gradient-filled frame.
fn cell(ui: &mut Ui, id: &'static str, paint: impl Fn(&mut Ui)) {
    Panel::zstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .padding(8.0)
        .show(ui, paint);
}

fn filled(brush: Brush) -> Background {
    Background {
        fill: brush,
        radius: Corners::all(8.0),
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

fn horizontal(ui: &mut Ui) {
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(linear_filled(LinearGradient::two_stop(0.0, NAVY, BLUE)))
        .show(ui);
}

fn vertical(ui: &mut Ui) {
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(linear_filled(LinearGradient::two_stop(
            std::f32::consts::FRAC_PI_2,
            NAVY,
            BLUE,
        )))
        .show(ui);
}

fn diagonal(ui: &mut Ui) {
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(linear_filled(LinearGradient::two_stop(
            std::f32::consts::FRAC_PI_4,
            ORANGE,
            YELLOW,
        )))
        .show(ui);
}

/// Radial centred at (0.5, 0.5) with a circular radius of 0.5 (touches
/// the bounding square mid-edges). Bright core, dark rim.
fn radial_centered(ui: &mut Ui) {
    let g = RadialGradient::two_stop_centered(YELLOW, NAVY);
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(radial_filled(g))
        .show(ui);
}

/// Off-centre radial — the bright core hugs the top-left, the rim
/// reaches further along the diagonal.
fn radial_offset(ui: &mut Ui) {
    let g = RadialGradient::new(
        Vec2::new(0.25, 0.3),
        Vec2::new(0.9, 0.9),
        [
            palantir::Stop::new(0.0, ORANGE),
            palantir::Stop::new(0.6, RED),
            palantir::Stop::new(1.0, NAVY),
        ],
    );
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(radial_filled(g))
        .show(ui);
}

/// Elliptical radius — wider horizontally than vertically. Stretches
/// the core into an oval.
fn radial_ellipse(ui: &mut Ui) {
    let g = RadialGradient::new(
        Vec2::splat(0.5),
        Vec2::new(0.55, 0.25),
        [
            palantir::Stop::new(0.0, GREEN),
            palantir::Stop::new(1.0, NAVY),
        ],
    );
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(radial_filled(g))
        .show(ui);
}

/// Conic colour-wheel centred in the cell. Six saturated stops sweep
/// CCW from the positive-x axis, with stop 0 == stop 1 so the seam
/// hides at angle 0.
fn conic_wheel(ui: &mut Ui) {
    let g = ConicGradient::new(
        Vec2::splat(0.5),
        0.0,
        [
            palantir::Stop::new(0.0, RED),
            palantir::Stop::new(0.166, YELLOW),
            palantir::Stop::new(0.333, GREEN),
            palantir::Stop::new(0.5, Srgb8::hex(0x22ccdd)),
            palantir::Stop::new(0.666, BLUE),
            palantir::Stop::new(0.833, Srgb8::hex(0xd14fdf)),
            palantir::Stop::new(1.0, RED),
        ],
    );
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(conic_filled(g))
        .show(ui);
}

/// Conic with a non-zero `start_angle` — the same sweep, rotated. Pin
/// for the `(theta - start_angle) / TAU` shader math.
fn conic_rotated(ui: &mut Ui) {
    let g = ConicGradient::new(
        Vec2::splat(0.5),
        std::f32::consts::FRAC_PI_2,
        [
            palantir::Stop::new(0.0, NAVY),
            palantir::Stop::new(0.5, YELLOW),
            palantir::Stop::new(1.0, NAVY),
        ],
    );
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(conic_filled(g))
        .show(ui);
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
                [
                    palantir::Stop::new(0.0, BLUE),
                    palantir::Stop::new(1.0, ORANGE),
                ],
            )
            .with_spread(Spread::Reflect);
            Frame::new()
                .id_salt("reflect-radial")
                .size((Sizing::FILL, Sizing::FILL))
                .background(radial_filled(r))
                .show(ui);

            // Repeat linear stripes.
            let l = LinearGradient::two_stop(0.0, NAVY, BLUE).with_spread(Spread::Repeat);
            let mut l = l;
            l.stops[1].offset = 0.25;
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
