use crate::primitives::color::RgbaF32;
use crate::primitives::color::color_coords::ColorCoords;
use crate::primitives::color::color_model::ColorModel;
use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::image::Image;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;
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

#[derive(Debug)]
struct EditEdges {
    changed: bool,
    committed: bool,
}

fn frame(h: &mut UiHarness, id: WidgetId, state: &mut ColorCoords) -> EditEdges {
    h.frame_value(|ui| {
        let r = ColorField::new(state).id(id).show(ui);
        EditEdges {
            changed: r.changed,
            committed: r.committed,
        }
    })
}

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

#[test]
fn changed_and_committed_are_edges() {
    let id = WidgetId::from_hash("field-edges");
    let mut h = harness();
    let mut state = coords(0.3, 0.2, 0.2);
    frame(&mut h, id, &mut state);

    h.press_at(Vec2::new(104.0, 80.0));
    let EditEdges { changed, committed } = frame(&mut h, id, &mut state);
    assert!(changed, "the press moved the value");
    assert!(!committed, "a press is not the end of a gesture");

    let EditEdges { changed, committed } = frame(&mut h, id, &mut state);
    assert!(!changed, "a held pointer that has not moved writes nothing");
    assert!(!committed);

    h.release();
    let EditEdges { committed, .. } = frame(&mut h, id, &mut state);
    assert!(committed, "the release frame is the one edit");

    let EditEdges { changed, committed } = frame(&mut h, id, &mut state);
    assert!(!changed && !committed, "no residual signals");
}

/// The texture is sRGB-encoded, because that is what `Rgba8UnormSrgb` decodes
/// on sample. Writing the linear bytes `RgbaU8::from` produces would paint
/// the whole field far too bright, and nothing else in the crate would catch
/// it.
///
/// Two columns and three rows put a texel at `s = 0.25, v = 0.5` on hue 0. In
/// HSV that is `R = v`, `G = B = v(1 - s)` as **encoded** components: 0.5 and
/// 0.375, so 128 and 96. Read as linear the same colour would be 188 and 166.
#[test]
fn texels_are_srgb_encoded() {
    let mut image = Image::blank(UVec2::new(2, 3));
    fill(&mut image, ColorModel::Hsv, 0.0);
    assert_eq!(image.texels()[2], SrgbaU8::rgb(128, 96, 96));
}

#[test]
fn a_hue_change_repaints_the_whole_field() {
    let id = WidgetId::from_hash("field-repaint");
    // A surface the field is a small part of, so the damage stays partial
    // rather than tripping the full-repaint coverage threshold.
    let mut h = UiHarness::new(UVec2::new(800, 600));
    let mut state = coords(0.3, 0.5, 0.5);
    frame(&mut h, id, &mut state);
    frame(&mut h, id, &mut state);
    state.set_hue(0.9);
    let report = h.frame(|ui| {
        ColorField::new(&mut state).id(id).show(ui);
    });
    let field = h.rect(id).expect("the field was laid out");
    let Some(RenderPlan {
        damage: Damage::Partial(damage),
        ..
    }) = report.plan
    else {
        panic!(
            "a hue change must damage part of the frame, got {:?}",
            report.plan
        );
    };
    let covered = damage.region.iter_rects().any(|r| {
        r.min.x <= field.min.x
            && r.min.y <= field.min.y
            && r.max().x >= field.max().x
            && r.max().y >= field.max().y
    });
    assert!(
        covered,
        "the field {field:?} is not inside the damage {damage:?}"
    );
}

// An sRGB texture decodes before filtering, so interpolate linear texels.
fn sample(texels: &[RgbaF32], size: UVec2, u: f32, v: f32) -> RgbaF32 {
    #[derive(Debug)]
    struct AxisSample {
        low: u32,
        fraction: f32,
    }
    let axis = |fraction: f32, count: u32| {
        let at = (fraction * count as f32 - 0.5).clamp(0.0, (count - 1) as f32);
        let low = at.floor();
        AxisSample {
            low: low as u32,
            fraction: at - low,
        }
    };
    let AxisSample {
        low: x,
        fraction: fx,
    } = axis(u, size.x);
    let AxisSample {
        low: y,
        fraction: fy,
    } = axis(v, size.y);
    let x1 = (x + 1).min(size.x - 1);
    let y1 = (y + 1).min(size.y - 1);
    let mix = |a: RgbaF32, b: RgbaF32, t: f32| a.lerp(b, t);
    let top = mix(
        texels[(y * size.x + x) as usize],
        texels[(y * size.x + x1) as usize],
        fx,
    );
    let bottom = mix(
        texels[(y1 * size.x + x) as usize],
        texels[(y1 * size.x + x1) as usize],
        fx,
    );
    mix(top, bottom, fy)
}

#[derive(Debug, Default)]
struct SampleError {
    value: f32,
    at: Vec2,
}

fn worst_error(model: ColorModel, downsample: u32) -> SampleError {
    const SCALE: f32 = 1.5;
    let pixels = UVec2::new(
        (FIELD.x as f32 * SCALE) as u32,
        (FIELD.y as f32 * SCALE) as u32,
    );
    let size = UVec2::new(
        (pixels.x as f32 / downsample as f32).ceil() as u32,
        (pixels.y as f32 / downsample as f32).ceil() as u32,
    );
    let mut image = Image::blank(size);
    let mut texels = Vec::with_capacity((size.x * size.y) as usize);
    let mut worst = SampleError::default();
    for step in 0..12 {
        let hue = step as f32 / 12.0;
        fill(&mut image, model, hue);
        texels.clear();
        texels.extend(image.texels().iter().copied().map(RgbaF32::from_srgba));
        let slice = model.slice(hue);
        for row in 0..pixels.y {
            let v = (row as f32 + 0.5) / pixels.y as f32;
            for column in 0..pixels.x {
                let u = (column as f32 + 0.5) / pixels.x as f32;
                let shown = sample(&texels, size, u, v).to_srgba_u8();
                let want = slice.color(u, 1.0 - v).to_srgba_u8();
                for (a, b) in [(shown.r, want.r), (shown.g, want.g), (shown.b, want.b)] {
                    let error = (f32::from(a) - f32::from(b)).abs();
                    if error > worst.value {
                        worst = SampleError {
                            value: error,
                            at: Vec2::new(u, 1.0 - v),
                        };
                    }
                }
            }
        }
    }
    worst
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
    let errors = std::thread::scope(|scope| {
        let sweeps = ColorModel::ALL.map(|model| {
            [1, 4, 16].map(|downsample| scope.spawn(move || worst_error(model, downsample)))
        });
        sweeps.map(|model| model.map(|sweep| sweep.join().unwrap()))
    });
    for (model, [exact, sampled, coarse]) in ColorModel::ALL.into_iter().zip(errors) {
        let SampleError { value: worst, at } = sampled;
        let u = at.x;
        let v = at.y;
        assert!(worst <= 9.0, "{model:?} at 4: {worst}/255 at s={u} v={v}");
        assert!(
            v > 0.9,
            "{model:?}: worst error left the top edge, at v={v}"
        );
        assert_eq!(exact.value, 0.0, "{model:?} at 1 is exact");
        let coarse = coarse.value;
        assert!(
            coarse > worst,
            "{model:?} at 16 ({coarse}/255) should be worse than at 4 ({worst}/255)",
        );
    }
}
