use crate::primitives::approx::{EPS, FloatHash, approx_zero, canon_bits, noop_f32};
use crate::primitives::rect::Rect;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher as _;

fn finish_hash(write: impl FnOnce(&mut DefaultHasher)) -> u64 {
    let mut hasher = DefaultHasher::new();
    write(&mut hasher);
    hasher.finish()
}

/// **The NaN audit.** Every *paint* no-op predicate — "would this
/// put down a texel" — must answer `true` for a NaN anywhere in its
/// inputs, because a NaN that survives the gate goes on to poison a
/// bbox, a damage rect, or a shader lane, and does it silently.
///
/// Deliberately excluded are the predicates that ask a *different*
/// question: `approx_zero`, `Size::approx_zero`, `Rect::approx_zero`,
/// `Corners::approx_zero`, and `TranslateScale::is_noop` all mean "is
/// this value ≈ this constant", and they gate **fast paths**, not
/// paint. Answering `true` there would route a NaN *into* the sharp /
/// identity shortcut instead of away from it — the opposite of safe.
/// They are covered by the shape-level gate instead, which drops a
/// NaN before any of them is ever reached.
#[test]
fn every_paint_noop_predicate_treats_nan_as_invisible() {
    use crate::primitives::brush::{Brush, CurveBrush};
    use crate::primitives::color::{Color, ColorF16};
    use crate::primitives::mesh::Mesh;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::size::Size;
    use crate::primitives::stroke::Stroke;
    use crate::scene::shapes::paint::{LoweredShadow, ShapeStroke};
    use glam::Vec2;

    const N: f32 = f32::NAN;
    let nan_color = Color::rgba(0.0, 0.0, 0.0, N);
    let mut nan_mesh = Mesh::new();
    nan_mesh.vertex(Vec2::new(N, 0.0), Color::WHITE);
    nan_mesh.vertex(Vec2::ZERO, Color::WHITE);
    nan_mesh.vertex(Vec2::X, Color::WHITE);
    nan_mesh.triangle(0, 1, 2);

    let cases: &[(&str, bool)] = &[
        ("noop_f32", noop_f32(N)),
        ("Size::is_paint_empty/w", Size::new(N, 4.0).is_paint_empty()),
        ("Size::is_paint_empty/h", Size::new(4.0, N).is_paint_empty()),
        (
            "Rect::is_paint_empty/size",
            Rect::new(0.0, 0.0, N, 4.0).is_paint_empty(),
        ),
        (
            "Rect::is_paint_empty/min",
            Rect::new(N, 0.0, 4.0, 4.0).is_paint_empty(),
        ),
        ("Color::is_noop", nan_color.is_noop()),
        ("ColorF16::is_noop", ColorF16::from(nan_color).is_noop()),
        (
            "Stroke::is_noop/width",
            Stroke::solid(Color::WHITE, N).is_noop(),
        ),
        (
            "Stroke::is_noop/color",
            Stroke::solid(nan_color, 2.0).is_noop(),
        ),
        (
            "ShapeStroke::is_noop/width",
            ShapeStroke::from(Stroke::solid(Color::WHITE, N)).is_noop(),
        ),
        (
            "ShapeStroke::is_noop/color",
            ShapeStroke::from(Stroke::solid(nan_color, 2.0)).is_noop(),
        ),
        ("Brush::is_noop", Brush::Solid(nan_color).is_noop()),
        (
            "CurveBrush::is_noop",
            CurveBrush::Solid(nan_color).is_noop(),
        ),
        (
            "Shadow::is_noop/color",
            Shadow {
                color: nan_color,
                ..Shadow::default()
            }
            .is_noop(),
        ),
        (
            "Shadow::is_noop/blur",
            Shadow {
                color: Color::WHITE,
                blur: N,
                ..Shadow::default()
            }
            .is_noop(),
        ),
        (
            "Shadow::is_noop/offset",
            Shadow {
                color: Color::WHITE,
                offset: Vec2::new(N, 0.0),
                ..Shadow::default()
            }
            .is_noop(),
        ),
        (
            "Shadow::is_noop/spread",
            Shadow {
                color: Color::WHITE,
                spread: N,
                ..Shadow::default()
            }
            .is_noop(),
        ),
        ("Mesh::is_noop", nan_mesh.is_noop()),
        // The convenience constructors pre-cache their own bbox
        // instead of routing through `Mesh::vertex`, so they need
        // covering separately — a bare fold there is how a NaN
        // vertex reaches the shader with a finite box.
        (
            "Mesh::filled_triangle/is_noop",
            Mesh::filled_triangle(Vec2::new(N, 0.0), Vec2::ZERO, Vec2::X, Color::WHITE).is_noop(),
        ),
        (
            "Mesh::filled_polygon/is_noop",
            Mesh::filled_polygon(
                &[Vec2::new(N, 0.0), Vec2::ZERO, Vec2::X, Vec2::Y],
                Color::WHITE,
            )
            .is_noop(),
        ),
        // Chrome has no record-level gate to fall back on — it does
        // not pass through `Shapes::add` — so these four are the
        // only thing standing between a NaN `Background` and the
        // shader.
        (
            "ColorF16::is_noop/red",
            ColorF16::from(Color::rgba(N, 0.0, 0.0, 1.0)).is_noop(),
        ),
        (
            "Color::is_noop/red",
            Color::rgba(N, 0.0, 0.0, 1.0).is_noop(),
        ),
        (
            "LoweredShadow::is_noop/blur",
            LoweredShadow::from(Shadow {
                color: Color::WHITE,
                blur: N,
                ..Shadow::default()
            })
            .is_noop(),
        ),
        (
            "LoweredShadow::is_noop/offset",
            LoweredShadow::from(Shadow {
                color: Color::WHITE,
                offset: Vec2::new(N, 0.0),
                ..Shadow::default()
            })
            .is_noop(),
        ),
    ];

    let missed: Vec<&str> = cases
        .iter()
        .filter(|(_, is_noop)| !is_noop)
        .map(|(label, _)| *label)
        .collect();
    assert!(
        missed.is_empty(),
        "these paint no-op predicates let a NaN through: {missed:?}",
    );
}

#[test]
fn approx_zero_handles_boundary_sign_and_nan() {
    let cases: &[(&str, f32, bool)] = &[
        ("exact_zero", 0.0, true),
        ("neg_zero", -0.0, true),
        ("at_eps", EPS, true),
        ("at_neg_eps", -EPS, true),
        ("just_above_eps", EPS * 1.1, false),
        ("just_below_neg_eps", -EPS * 1.1, false),
        ("nan", f32::NAN, false),
    ];
    for (label, v, want) in cases {
        assert_eq!(approx_zero(*v), *want, "case: {label}");
    }
}

#[test]
fn exact_hash_helpers_collapse_only_signed_zero() {
    let positive = Rect::new(0.0, 0.0, 0.0, 0.0);
    let negative = Rect::new(-0.0, -0.0, -0.0, -0.0);
    let sub_eps = Rect::new(EPS * 0.5, 0.0, 0.0, 0.0);

    assert_eq!(
        finish_hash(|h| positive.hash_eq(h)),
        finish_hash(|h| negative.hash_eq(h)),
    );
    assert_ne!(
        finish_hash(|h| positive.hash_eq(h)),
        finish_hash(|h| sub_eps.hash_eq(h)),
    );
}

#[test]
fn visual_hash_helpers_collapse_zero_noise_and_nan_payloads() {
    let zero = Rect::ZERO;
    let sub_eps = Rect::new(EPS * 0.5, -EPS * 0.5, EPS, -EPS);
    assert_eq!(
        finish_hash(|h| zero.hash_visual(h)),
        finish_hash(|h| sub_eps.hash_visual(h)),
    );

    let nan_a = f32::from_bits(0x7fc0_0001);
    let nan_b = f32::from_bits(0x7fc0_0002);
    assert_eq!(canon_bits(nan_a), canon_bits(nan_b));
    assert_eq!(
        finish_hash(|h| nan_a.hash_visual(h)),
        finish_hash(|h| nan_b.hash_visual(h)),
    );
}
