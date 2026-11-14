//! Gradient brushes. Each tile paints a `Block` whose `Chrome.fill` carries one gradient variant, exercising composer, atlas bake, shader sample and premultiplied blend every frame. Vivid stops make spread and interpolation differences read at a glance.

use crate::scaffold::api;
use crate::support;
use crate::support::{DemoCell, Kit, Knob, Section, TILE};
use palantir::glam::Vec2;
use palantir::{
    Block, Brush, Chrome, ColorButton, ColorPicker, ComboBox, Configure, Corners, Gradient,
    Interpolation, RgbaF32, Sizing, Spread, Stop, Ui, WidgetId,
};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

const NAVY: RgbaF32 = RgbaF32::hex(0x1a1a2e);
const BLUE: RgbaF32 = RgbaF32::hex(0x4c5cdb);
const ORANGE: RgbaF32 = RgbaF32::hex(0xff7e44);
const YELLOW: RgbaF32 = RgbaF32::hex(0xfacc15);
const RED: RgbaF32 = RgbaF32::hex(0xff5e44);
const GREEN: RgbaF32 = RgbaF32::hex(0x46c46c);

pub fn build(ui: &mut Ui, k: Kit) {
    Section::new("Linear")
        .apis(&[
            api!(type Gradient),
            api!(Gradient::linear as fn(f32, [RgbaF32; 2]) -> Gradient),
            api!(Gradient::horizontal as fn([RgbaF32; 2]) -> Gradient),
            api!(Gradient::vertical as fn([RgbaF32; 2]) -> Gradient),
        ])
        .show(ui, k, |ui| {
            support::note(ui, k, "The angle is in radians from the +x axis.");
            support::tiles(ui, |ui| {
                DemoCell::new("horizontal").show(ui, k, horizontal);
                DemoCell::new("vertical").show(ui, k, vertical);
                DemoCell::new("45°").show(ui, k, diagonal);
            });
        });

    Section::new("Radial")
        .apis(&[
            api!(Gradient::radial as fn(Vec2, Vec2, [RgbaF32; 2]) -> Gradient),
            api!(Gradient::centered_radial as fn([RgbaF32; 2]) -> Gradient),
        ])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "A center and a radius per axis, in the unit square of the box.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("centered, circular").show(ui, k, radial_centered);
                DemoCell::new("offset center, three stops").show(ui, k, radial_offset);
                DemoCell::new("elliptical radius").show(ui, k, radial_ellipse);
            });
        });

    Section::new("Conic")
        .apis(&[
            api!(Gradient::conic as fn(Vec2, f32, [RgbaF32; 2]) -> Gradient),
            api!(Gradient::centered_conic as fn([RgbaF32; 2]) -> Gradient),
        ])
        .show(ui, k, |ui| {
            support::note(ui, k, "A sweep about a center from a start angle.");
            support::tiles(ui, |ui| {
                DemoCell::new("color wheel").show(ui, k, conic_wheel);
                DemoCell::new("rotated 90°").show(ui, k, conic_rotated);
            });
        });

    Section::new("Spread and interpolation")
        .apis(&[api!(type Spread), api!(type Interpolation)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "What happens outside the stop range, and the color space the stops \
                 blend in.",
            );
            support::tiles(ui, |ui| {
                DemoCell::new("Spread::Reflect — rings mirror out").show(ui, k, reflect);
                DemoCell::new("Spread::Repeat — rings").show(ui, k, repeat);
                DemoCell::new("Interpolation::Oklab — perceptual midpoint").show(ui, k, oklab);
            });
        });

    live(ui, k);
}

static SPACES: [(&str, Interpolation); 2] = [
    ("linear RGB", Interpolation::Linear),
    ("Oklab", Interpolation::Oklab),
];

/// The live gradient's knobs: two stops, the angle and the space they blend in.
#[derive(Debug)]
struct LiveGradient {
    from: RgbaF32,
    to: RgbaF32,
    angle: f32,
    space: (&'static str, Interpolation),
}

impl Default for LiveGradient {
    fn default() -> Self {
        Self {
            from: RED,
            to: GREEN,
            angle: 0.0,
            space: SPACES[1],
        }
    }
}

/// One linear brush from two picked stops, turned by the angle knob, blended in the picked space.
fn live(ui: &mut Ui, k: Kit) {
    Section::new("Live gradient")
        .apis(&[api!(Gradient::with_interpolation)])
        .show(ui, k, |ui| {
            support::note(
                ui,
                k,
                "Pick two stops, turn the axis and change the space they blend in: red to green \
                 through linear RGB passes a muddy midpoint Oklab avoids.",
            );
            let id = WidgetId::from_hash("showcase::gradients::live");
            ui.with_state::<LiveGradient, _>(id, |ui, l| {
                support::row(ui, |ui| {
                    ColorButton::new(ColorPicker::new(&mut l.from))
                        .id_salt("from")
                        .show(ui);
                    ColorButton::new(ColorPicker::new(&mut l.to))
                        .id_salt("to")
                        .show(ui);
                    ComboBox::labeled(&mut l.space, SPACES, |space| space.0)
                        .size((Sizing::fixed(120.0), Sizing::HUG))
                        .show(ui);
                });
                Knob {
                    label: "angle",
                    range: 0.0..=360.0,
                    step: 5.0,
                    unit: "°",
                }
                .show(ui, k, &mut l.angle);
                let space = l.space.1;
                let brush = Gradient::linear((l.angle).to_radians(), [l.from, l.to])
                    .with_interpolation(space);
                DemoCell::new("the knobs' brush")
                    .size(360.0, TILE)
                    .show(ui, k, |ui| gradient_frame(ui, filled(brush)));
            });
        });
}

fn filled(brush: impl Into<Brush>) -> Chrome {
    Chrome::NONE
        .with_corners(Corners::all(support::RADIUS))
        .with_fill(brush)
}

fn gradient_frame(ui: &mut Ui, chrome: Chrome) {
    Block::new()
        .size((Sizing::FILL, Sizing::FILL))
        .chrome(chrome)
        .show(ui);
}

fn horizontal(ui: &mut Ui) {
    gradient_frame(ui, filled(Gradient::horizontal([NAVY, BLUE])));
}

fn vertical(ui: &mut Ui) {
    gradient_frame(ui, filled(Gradient::vertical([NAVY, BLUE])));
}

fn diagonal(ui: &mut Ui) {
    gradient_frame(ui, filled(Gradient::linear(FRAC_PI_4, [ORANGE, YELLOW])));
}

/// Radial centered at (0.5, 0.5), radius 0.5 (touches the square's mid-edges); bright core, dark rim.
fn radial_centered(ui: &mut Ui) {
    gradient_frame(ui, filled(Gradient::centered_radial([YELLOW, NAVY])));
}

/// Off-center radial: the core hugs the top-left, the rim reaches further along the diagonal.
fn radial_offset(ui: &mut Ui) {
    let g = Gradient::radial(
        Vec2::new(0.25, 0.3),
        Vec2::new(0.9, 0.9),
        [
            Stop::new(0.0, ORANGE),
            Stop::new(0.6, RED),
            Stop::new(1.0, NAVY),
        ],
    );
    gradient_frame(ui, filled(g));
}

/// Elliptical radius, wider than tall; stretches the core into an oval.
fn radial_ellipse(ui: &mut Ui) {
    let g = Gradient::radial(Vec2::splat(0.5), Vec2::new(0.55, 0.25), [GREEN, NAVY]);
    gradient_frame(ui, filled(g));
}

/// Conic color wheel: six saturated stops sweep clockwise from +x, stop 0 == stop 1 so the seam hides at angle 0.
fn conic_wheel(ui: &mut Ui) {
    let g = Gradient::centered_conic([
        RED,
        YELLOW,
        GREEN,
        RgbaF32::hex(0x22ccdd),
        BLUE,
        RgbaF32::hex(0xd14fdf),
        RED,
    ]);
    gradient_frame(ui, filled(g));
}

/// Conic with non-zero `start_angle`: the same sweep rotated; pins the `(theta - start_angle) / TAU` shader math.
fn conic_rotated(ui: &mut Ui) {
    let g = Gradient::conic(Vec2::splat(0.5), FRAC_PI_2, [NAVY, YELLOW, NAVY]);
    gradient_frame(ui, filled(g));
}

/// Radial whose stops end at r = 0.25; everything beyond mirrors back in.
fn reflect(ui: &mut Ui) {
    let g = Gradient::radial(Vec2::splat(0.5), Vec2::splat(0.25), [BLUE, ORANGE])
        .with_spread(Spread::Reflect);
    gradient_frame(ui, filled(g));
}

/// Radial whose stops end at r = 0.125; past it the ramp starts over, giving rings. A linear axis spans the box exactly, so `t` never leaves 0..1 and no spread mode shows.
fn repeat(ui: &mut Ui) {
    let g = Gradient::radial(Vec2::splat(0.5), Vec2::splat(0.125), [NAVY, BLUE])
        .with_spread(Spread::Repeat);
    gradient_frame(ui, filled(g));
}

/// Red to green in Oklab: no muddy gray midpoint as in a linear-RGB blend.
fn oklab(ui: &mut Ui) {
    let g = Gradient::horizontal([RED, GREEN]).with_interpolation(Interpolation::Oklab);
    gradient_frame(ui, filled(g));
}
