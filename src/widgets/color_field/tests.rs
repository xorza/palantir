use crate::primitives::color::Color;
use crate::primitives::color::color_coords::ColorCoords;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::harness::UiHarness;
use crate::widgets::color_field::{ColorField, fill};
use glam::{UVec2, Vec2};

const FIELD: UVec2 = UVec2::new(208, 160);

fn harness() -> UiHarness {
    UiHarness::new(FIELD)
}

fn coords(hue: f32, sat: f32, val: f32) -> ColorCoords {
    let mut c = ColorCoords::default();
    c.set_hue(hue);
    c.set_sat(sat);
    c.set_val(val);
    c
}

/// One frame of a field bound to `state`, reporting what it wrote.
fn frame(h: &mut UiHarness, id: WidgetId, state: &mut ColorCoords) -> (bool, bool) {
    h.frame_value(|ui| {
        let r = ColorField::new(state).id(id).show(ui);
        (r.changed, r.committed)
    })
}

/// The pointer maps onto the axes with no drift: the top-left corner is the
/// grey-and-bright end exactly, and a quarter across is a quarter.
#[test]
fn the_pointer_maps_onto_the_axes() {
    let id = WidgetId::from_hash("field-mapping");
    let places = [
        (Vec2::new(0.0, 0.0), 0.0, 1.0),
        (Vec2::new(52.0, 40.0), 0.25, 0.75),
        (Vec2::new(104.0, 80.0), 0.5, 0.5),
    ];
    for (at, sat, val) in places {
        let mut h = harness();
        let mut state = coords(0.3, 0.9, 0.1);
        frame(&mut h, id, &mut state);
        h.press_at(at);
        frame(&mut h, id, &mut state);
        assert_eq!(state.sat(), sat, "saturation at {at:?}");
        assert_eq!(state.val(), val, "value at {at:?}");
    }
}

/// Dragging past a corner clamps to it, which is the only way a pointer
/// reaches an axis end — the last pixel's centre is half a pixel short of it.
/// The gamut edge lives at `s = 1`, so a picker that could not clamp could
/// not reach it.
#[test]
fn a_drag_past_the_edge_clamps_to_it() {
    let id = WidgetId::from_hash("field-clamp");
    let mut h = harness();
    let mut state = coords(0.3, 0.5, 0.5);
    frame(&mut h, id, &mut state);
    h.press_at(Vec2::new(104.0, 80.0));
    frame(&mut h, id, &mut state);
    h.drag_to(Vec2::new(400.0, 400.0));
    frame(&mut h, id, &mut state);
    assert_eq!(state.sat(), 1.0, "dragged past the right edge");
    assert_eq!(state.val(), 0.0, "dragged past the bottom edge");
}

/// A press writes and a release commits, and neither fires on a frame where
/// nothing moved.
#[test]
fn changed_and_committed_are_edges() {
    let id = WidgetId::from_hash("field-edges");
    let mut h = harness();
    let mut state = coords(0.3, 0.2, 0.2);
    frame(&mut h, id, &mut state);

    h.press_at(Vec2::new(104.0, 80.0));
    let (changed, committed) = frame(&mut h, id, &mut state);
    assert!(changed, "the press moved the value");
    assert!(!committed, "a press is not the end of a gesture");

    let (changed, committed) = frame(&mut h, id, &mut state);
    assert!(!changed, "a held pointer that has not moved writes nothing");
    assert!(!committed);

    h.release();
    let (_, committed) = frame(&mut h, id, &mut state);
    assert!(committed, "the release frame is the one edit");

    let (changed, committed) = frame(&mut h, id, &mut state);
    assert!(!changed && !committed, "no residual signals");
}

/// The texture is sRGB-encoded, because that is what `Rgba8UnormSrgb` decodes
/// on sample. Writing the linear bytes `ColorU8::from` produces would paint
/// the whole field far too bright, and nothing else in the crate would catch
/// it.
///
/// Two columns and three rows put a texel at `s = 0.25, v = 0.5` on hue 0. In
/// HSV that is `R = v`, `G = B = v(1 - s)` as **encoded** components: 0.5 and
/// 0.375, so 128 and 96. Read as linear the same colour would be 188 and 166.
#[test]
fn texels_are_srgb_encoded() {
    let mut texels = Vec::new();
    fill(&mut texels, UVec2::new(2, 3), ColorModel::Hsv, 0.0);
    let middle_row = 2 * 4;
    assert_eq!(&texels[middle_row..middle_row + 4], &[128, 96, 96, 255]);
}

/// Decode one texel of an `Rgba8UnormSrgb` texture the way a sampler does.
fn texel(texels: &[u8], size: UVec2, column: u32, row: u32) -> Color {
    let at = ((row * size.x + column) * 4) as usize;
    Color::rgb_u8(texels[at], texels[at + 1], texels[at + 2])
}

/// What the sampler shows at a fraction across a `size`-texel image: decode
/// to linear, then filter. An sRGB texture decodes *before* it filters, which
/// is why this interpolates linear values rather than bytes.
fn sample(texels: &[u8], size: UVec2, u: f32, v: f32) -> Color {
    let axis = |fraction: f32, count: u32| {
        let at = (fraction * count as f32 - 0.5).clamp(0.0, (count - 1) as f32);
        let low = at.floor();
        (low as u32, (at - low).clamp(0.0, 1.0))
    };
    let (x, fx) = axis(u, size.x);
    let (y, fy) = axis(v, size.y);
    let x1 = (x + 1).min(size.x - 1);
    let y1 = (y + 1).min(size.y - 1);
    let mix = |a: Color, b: Color, t: f32| a.lerp(b, t);
    let top = mix(texel(texels, size, x, y), texel(texels, size, x1, y), fx);
    let bottom = mix(texel(texels, size, x, y1), texel(texels, size, x1, y1), fx);
    mix(top, bottom, fy)
}

/// Worst error, in 8-bit sRGB units, between the sampled texture and the
/// exact colour, over one field's worth of pixels at twelve hues.
fn worst_error(model: ColorModel, downsample: u32) -> f32 {
    worst_error_at(model, downsample).0
}

fn worst_error_at(model: ColorModel, downsample: u32) -> (f32, f32, f32) {
    const SCALE: f32 = 1.5;
    let pixels = UVec2::new(
        (FIELD.x as f32 * SCALE) as u32,
        (FIELD.y as f32 * SCALE) as u32,
    );
    let size = UVec2::new(
        (pixels.x as f32 / downsample as f32).ceil() as u32,
        (pixels.y as f32 / downsample as f32).ceil() as u32,
    );
    let mut texels = Vec::new();
    let mut worst = 0.0_f32;
    let (mut worst_u, mut worst_v) = (0.0_f32, 0.0_f32);
    for step in 0..12 {
        let hue = step as f32 / 12.0;
        texels.clear();
        fill(&mut texels, size, model, hue);
        let slice = model.slice(hue);
        for row in 0..pixels.y {
            let v = (row as f32 + 0.5) / pixels.y as f32;
            for column in 0..pixels.x {
                let u = (column as f32 + 0.5) / pixels.x as f32;
                let shown = sample(&texels, size, u, v).to_srgb_u8();
                let want = slice.color(u, 1.0 - v).to_srgb_u8();
                for (a, b) in [(shown.r, want.r), (shown.g, want.g), (shown.b, want.b)] {
                    let error = (f32::from(a) - f32::from(b)).abs();
                    if error > worst {
                        worst = error;
                        worst_u = u;
                        worst_v = 1.0 - v;
                    }
                }
            }
        }
    }
    (worst, worst_u, worst_v)
}

/// The default resolution holds the field within nine 8-bit steps of the
/// exact colour, one texel per pixel is exact, and a coarse one is worse.
/// All three matter: the last is what proves the knob does the work the
/// first credits it with, and the middle proves the error is the sampling
/// rather than the conversion.
///
/// The bound is where it is because the worst pixel sits on the top edge,
/// `v = 1`, where the ramp along the gamut boundary is steepest — Okhsv at
/// the saturated corner, HSV at the white one. See
/// [`ColorField::downsample`](crate::ColorField::downsample) for the table.
#[test]
fn downsample_four_tracks_the_exact_colour() {
    for model in ColorModel::ALL {
        let (worst, u, v) = worst_error_at(model, 4);
        assert!(worst <= 9.0, "{model:?} at 4: {worst}/255 at s={u} v={v}");
        assert!(
            v > 0.9,
            "{model:?}: worst error left the top edge, at v={v}"
        );
        let _ = u;
        assert_eq!(worst_error(model, 1), 0.0, "{model:?} at 1 is exact");
        let coarse = worst_error(model, 16);
        assert!(
            coarse > worst,
            "{model:?} at 16 ({coarse}/255) should be worse than at 4 ({worst}/255)",
        );
    }
}
