//! The chart's shapes: asterism lines, stars and their glows, deep-sky marks, the plan's rings
//! and the selection's.

use palantir::widget::Shape;
use palantir::{Brush, Corners, RadialGradient, Rect, RgbaF32, Stroke, Ui};

use crate::app::model::Observatory;
use crate::app::model::catalog::ObjectKind;
use crate::app::model::sky::{self, CHART};

/// Stars brighter than this get a glow.
const GLOW_MAG: f32 = 2.0;
/// The glow's color classes over B−V −0.4 … 2.0, so the gradient atlas holds one row a class.
const GLOW_CLASSES: f32 = 16.0;

/// A star's color, red under night vision so the chart keeps the dark adaptation too.
fn star_color(bv: f32, night: bool) -> RgbaF32 {
    let color = sky::star_color(bv);
    if night {
        RgbaF32::new(color.luminance().sqrt(), 0.0, 0.0, 1.0)
    } else {
        color
    }
}

/// The glow of a bright star: its color class's radial gradient, so stars of one class share a
/// gradient and the atlas holds one row a class.
fn glow(bv: f32, night: bool) -> Brush {
    let class = ((bv + 0.4) / 2.4 * GLOW_CLASSES)
        .floor()
        .clamp(0.0, GLOW_CLASSES - 1.0);
    let center = (class + 0.5) / GLOW_CLASSES * 2.4 - 0.4;
    let color = star_color(center, night);
    Brush::Radial(RadialGradient::two_stop(
        color.with_alpha(0.45),
        color.with_alpha(0.0),
    ))
}

/// The asterism lines, the stars and deep-sky objects in `view`, the plan's rings and the
/// selection's.
pub(super) fn sky(ui: &mut Ui, model: &Observatory, view: Rect, zoom: f32, night: bool) {
    let scale = zoom.sqrt();
    let reach = Rect::new(
        view.min.x - 20.0,
        view.min.y - 20.0,
        view.size.w + 40.0,
        view.size.h + 40.0,
    );
    let line_ink = if night {
        RgbaF32::new(0.35, 0.02, 0.02, 0.6)
    } else {
        RgbaF32::new(0.45, 0.55, 0.75, 0.45)
    };
    for &[a, b] in model.catalog.lines() {
        let (a, b) = (model.catalog.get(a), model.catalog.get(b));
        let (pa, pb) = (sky::chart_point(a.ra, a.dec), sky::chart_point(b.ra, b.dec));
        // A pair across the chart's RA seam would draw across the whole chart.
        if (pa.x - pb.x).abs() > CHART.x * 0.5 || !(reach.contains(pa) || reach.contains(pb)) {
            continue;
        }
        ui.add_shape(Shape::line(pa, pb, Stroke::new(line_ink, 1.0 / scale)));
    }
    for (_, object) in model.catalog.iter() {
        let at = sky::chart_point(object.ra, object.dec);
        if !reach.contains(at) {
            continue;
        }
        if object.kind == ObjectKind::Star {
            let radius = sky::star_radius(object.mag) / scale;
            if object.mag < GLOW_MAG {
                let halo = radius * 4.0;
                ui.add_shape(
                    Shape::rect(Rect::new(at.x - halo, at.y - halo, 2.0 * halo, 2.0 * halo))
                        .fill(glow(object.bv, night)),
                );
            }
            ui.add_shape(
                Shape::rect(Rect::new(
                    at.x - radius,
                    at.y - radius,
                    2.0 * radius,
                    2.0 * radius,
                ))
                .corners(Corners::all(radius))
                .fill(star_color(object.bv, night)),
            );
        } else {
            let ink = match (object.kind, night) {
                (_, true) => RgbaF32::new(0.6, 0.05, 0.05, 1.0),
                (ObjectKind::Galaxy, false) => RgbaF32::hex(0xd9a46a),
                (ObjectKind::Nebula, false) => RgbaF32::hex(0x6ad9b4),
                _ => RgbaF32::hex(0x9ab4ff),
            };
            ui.add_shape(Shape::circle(
                at,
                6.0 / scale,
                Stroke::new(ink, 1.2 / scale),
            ));
        }
    }
    let ring_ink = if night {
        RgbaF32::new(0.8, 0.1, 0.1, 1.0)
    } else {
        RgbaF32::hex(0xffd166)
    };
    for entry in model.plan.entries() {
        let object = model.catalog.get(entry.object);
        let at = sky::chart_point(object.ra, object.dec);
        ui.add_shape(Shape::circle(
            at,
            11.0 / scale,
            Stroke::new(ring_ink, 1.5 / scale),
        ));
    }
    if let Some(id) = model.selection {
        let object = model.catalog.get(id);
        let at = sky::chart_point(object.ra, object.dec);
        let ink = if night { ring_ink } else { RgbaF32::WHITE };
        ui.add_shape(Shape::circle(
            at,
            16.0 / scale,
            Stroke::new(ink, 2.0 / scale),
        ));
    }
}
