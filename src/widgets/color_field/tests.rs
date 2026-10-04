#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use crate::animation::animatable::Animatable;
use crate::damage::Damage;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::color_coords::ColorCoords;
use crate::primitives::paint::color::color_model::ColorModel;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::Image;
use crate::renderer::render_plan::RenderPlan;
use crate::widget_core::configure::Configure;
use crate::widgets::color_field::{ColorField, fill};
use glam::{UVec2, Vec2};
use std::array;
use std::thread;

const FIELD: UVec2 = UVec2::new(208, 160);

fn harness() -> UiHarness {
    UiHarness::new(FIELD)
}

fn coords(hue: f32, sat: f32, val: f32) -> ColorCoords {
    let mut c = ColorCoords::default();
    c.set_hue(hue);
    c.set_saturation(sat);
    c.set_value(val);
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
        assert_eq!(state.saturation(), sat, "saturation at {at:?}");
        assert_eq!(state.value(), val, "value at {at:?}");
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
    assert_eq!(state.saturation(), 1.0, "dragged past the right edge");
    assert_eq!(state.value(), 0.0, "dragged past the bottom edge");
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

/// Arrows step 0.005 and Shift-arrows ten steps, PageUp/PageDown page the
/// value axis by 0.1, and Home/End jump the saturation axis to its ends.
/// Each case starts from `s = v = 0.5`, so the expected axis is that plus
/// the key's travel, in the order the handler adds it.
#[test]
fn keys_walk_both_axes() {
    use crate::input::keyboard::key::Key;
    use crate::input::keyboard::modifiers::Modifiers;

    let shift = Modifiers::SHIFT;
    let cases = [
        (Modifiers::NONE, Key::ArrowRight, 0.5 + 0.005, 0.5),
        (shift, Key::ArrowLeft, 0.5 - 0.005 * 10.0, 0.5),
        (Modifiers::NONE, Key::ArrowUp, 0.5, 0.5 + 0.005),
        (Modifiers::NONE, Key::PageUp, 0.5, 0.5 + 0.1),
        (Modifiers::NONE, Key::PageDown, 0.5, 0.5 - 0.1),
        (Modifiers::NONE, Key::Home, 0.0, 0.5),
        (Modifiers::NONE, Key::End, 1.0, 0.5),
    ];
    let id = WidgetId::from_hash("field-keys");
    for (mods, key, sat, val) in cases {
        let mut h = harness();
        let mut state = coords(0.3, 0.5, 0.5);
        frame(&mut h, id, &mut state);
        h.set_focus(id);
        h.set_modifiers(mods);
        h.key(key);
        let EditEdges { changed, committed } = frame(&mut h, id, &mut state);
        assert_eq!(
            (state.saturation(), state.value()),
            (sat, val),
            "{mods:?} {key:?}"
        );
        assert!(changed && committed, "{key:?} is a whole edit");
    }
}

/// The texture is sRGB-encoded, because that is what `Rgba8UnormSrgb` decodes
/// on sample. Writing linear bytes instead would paint the whole field far too
/// bright, and nothing else in the crate would catch it.
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
    let mix = |a: RgbaF32, b: RgbaF32, t: f32| Animatable::lerp(a, b, t);
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

/// The texel sizes [`texel_size_four_tracks_the_exact_colour`]
/// compares: exact, the default, and coarse.
const TEXEL_SIZES: [u32; 3] = [1, 4, 16];

/// The worst channel error of the field at each of [`TEXEL_SIZES`] for one
/// `hue` slice, drawn at scale 1.5. The exact colour of a pixel is the same
/// at every factor, so it is converted once and compared three times.
fn worst_errors(model: ColorModel, hue: f32) -> [SampleError; 3] {
    const SCALE: f32 = 1.5;
    let pixels = UVec2::new(
        (FIELD.x as f32 * SCALE) as u32,
        (FIELD.y as f32 * SCALE) as u32,
    );
    let fields = TEXEL_SIZES.map(|texel_size| {
        let size = UVec2::new(
            (pixels.x as f32 / texel_size as f32).ceil() as u32,
            (pixels.y as f32 / texel_size as f32).ceil() as u32,
        );
        let mut image = Image::blank(size);
        fill(&mut image, model, hue);
        let texels: Vec<RgbaF32> = image
            .texels()
            .iter()
            .copied()
            .map(RgbaF32::from_srgba)
            .collect();
        (size, texels)
    });
    let slice = model.slice(hue);
    let mut worst = [(); 3].map(|()| SampleError::default());
    for row in 0..pixels.y {
        let v = (row as f32 + 0.5) / pixels.y as f32;
        for column in 0..pixels.x {
            let u = (column as f32 + 0.5) / pixels.x as f32;
            let want = slice.color(u, 1.0 - v).to_srgba_u8();
            for ((size, texels), worst) in fields.iter().zip(&mut worst) {
                let shown = sample(texels, *size, u, v).to_srgba_u8();
                for (a, b) in [(shown.r, want.r), (shown.g, want.g), (shown.b, want.b)] {
                    let error = (f32::from(a) - f32::from(b)).abs();
                    if error > worst.value {
                        *worst = SampleError {
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
/// [`ColorField::texel_size`](crate::ColorField::texel_size) for the table.
#[test]
fn texel_size_four_tracks_the_exact_colour() {
    // One thread per model and hue slice; each slice's worst is folded in
    // hue order, so a tie keeps the earlier slice's place.
    let errors = thread::scope(|scope| {
        let sweeps = ColorModel::ALL.map(|model| {
            array::from_fn::<_, 12, _>(|step| {
                scope.spawn(move || worst_errors(model, step as f32 / 12.0))
            })
        });
        sweeps.map(|slices| {
            let mut worst = [(); 3].map(|()| SampleError::default());
            for slice in slices {
                for (worst, slice) in worst.iter_mut().zip(slice.join().unwrap()) {
                    if slice.value > worst.value {
                        *worst = slice;
                    }
                }
            }
            worst
        })
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

/// The field's surface — its texture and the image behind it — lives on
/// the field's own id, so it leaves with the field rather than outliving
/// it.
#[test]
fn the_surface_leaves_with_the_field() {
    use crate::primitives::paint::color::color_model::ColorModel;
    use crate::widgets::color_surface::ColorSurface;

    let id = WidgetId::from_hash("leaving-field");
    let mut coords = ColorCoords::default();
    let mut h = UiHarness::new(UVec2::new(300, 300));
    h.frame(|ui| {
        ColorField::new(&mut coords).id(id).show(ui);
    });
    let surface = |h: &UiHarness| h.ui.state::<ColorSurface<(ColorModel, f32)>>(id).is_some();
    assert!(surface(&h), "built while the field records");
    h.frame(|_| {});
    assert!(!surface(&h), "swept with the field");
}
