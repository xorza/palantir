//! Linear-gradient showcase. Two rows × three cells; each cell paints
//! a sized `Frame` with `Background.fill: Brush::Linear(...)` so the
//! full slice-2 path runs (composer → atlas → shader → premul) every
//! frame. Stop colours stay vivid so spread/interp differences read at
//! a glance.

use palantir::{
    Background, Brush, Configure, Corners, Frame, Interp, LinearGradient, Panel, Sizing, Spread,
    Srgb8, Ui,
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
                    cell(ui, "3-stop", three_stop_rgb);
                    cell(ui, "spread modes", spread_modes);
                    cell(ui, "interp spaces", interp_spaces);
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

fn filled(g: LinearGradient) -> Background {
    Background {
        fill: Brush::Linear(g),
        radius: Corners::all(8.0),
        ..Default::default()
    }
}

fn horizontal(ui: &mut Ui) {
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(filled(LinearGradient::two_stop(0.0, NAVY, BLUE)))
        .show(ui);
}

fn vertical(ui: &mut Ui) {
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(filled(LinearGradient::two_stop(
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
        .background(filled(LinearGradient::two_stop(
            std::f32::consts::FRAC_PI_4,
            ORANGE,
            YELLOW,
        )))
        .show(ui);
}

fn three_stop_rgb(ui: &mut Ui) {
    Frame::new()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .background(filled(LinearGradient::three_stop(0.0, RED, YELLOW, GREEN)))
        .show(ui);
}

/// Three stacked cells, one per `Spread` mode. The gradient's stop
/// offsets only cover `0..0.5` of the parametric axis — outside that
/// range, `Pad` clamps, `Repeat` wraps, `Reflect` mirrors.
fn spread_modes(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(4.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for (i, (label, spread)) in [
                ("pad", Spread::Pad),
                ("repeat", Spread::Repeat),
                ("reflect", Spread::Reflect),
            ]
            .iter()
            .enumerate()
            {
                let mut g = LinearGradient::new(
                    0.0,
                    [
                        palantir::Stop::new(0.0, NAVY),
                        palantir::Stop::new(0.5, BLUE),
                    ],
                );
                g = g.with_spread(*spread);
                Frame::new()
                    .id_salt(("spread", i, *label))
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(filled(g))
                    .show(ui);
            }
        });
}

/// Three stacked cells comparing `Interp` spaces on the same
/// red→green 2-stop gradient. Differences show at the midpoint:
/// `Linear` muddies through dark olive, `Srgb` washes a bit lighter,
/// `Oklab` keeps perceived luminance up (closer to yellowish midpoint).
fn interp_spaces(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(4.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for (i, (label, interp)) in [
                ("linear", Interp::Linear),
                ("srgb", Interp::Srgb),
                ("oklab", Interp::Oklab),
            ]
            .iter()
            .enumerate()
            {
                let g = LinearGradient::two_stop(0.0, RED, GREEN).with_interp(*interp);
                Frame::new()
                    .id_salt(("interp", i, *label))
                    .size((Sizing::FILL, Sizing::FILL))
                    .background(filled(g))
                    .show(ui);
            }
        });
}
