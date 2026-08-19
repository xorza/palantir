use super::*;

#[test]
fn cache_key_discriminates_every_shaping_axis() {
    // The renderer caches shaped buffers by key, so any input that changes
    // glyph positions has to change the key. Miss one and a buffer shaped
    // for other parameters gets replayed — measured rect against the wrong
    // rasterized glyphs.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let base = c.measure("hi", shape(16.0)).key;

    for (label, variant, field, base_field) in [
        (
            "font size",
            shape(20.0),
            (|k: TextShapeKey| k.size_q) as fn(TextShapeKey) -> u32,
            base.size_q,
        ),
        (
            "line height",
            shape(16.0).leading(24.0),
            (|k: TextShapeKey| k.lh_q) as fn(TextShapeKey) -> u32,
            base.lh_q,
        ),
        (
            "family",
            shape(16.0).family(FontFamily::Mono),
            (|k: TextShapeKey| k.family_q as u32) as fn(TextShapeKey) -> u32,
            base.family_q as u32,
        ),
        (
            "weight",
            shape(16.0).weight(FontWeight::Bold),
            (|k: TextShapeKey| k.weight_q as u32) as fn(TextShapeKey) -> u32,
            base.weight_q as u32,
        ),
    ] {
        let key = c.measure("hi", variant).key;
        assert_ne!(base, key, "{label} must enter the cache key");
        assert_ne!(
            field(key),
            base_field,
            "{label} is the discriminating field"
        );
    }

    // The enum discriminants themselves are what land in the key, so a
    // variant reorder can't silently remap cached buffers.
    assert_eq!(base.family_q, FontFamily::Sans as u8);
    assert_eq!(base.weight_q, FontWeight::Regular as u8);
    assert_eq!(
        base.text_hash,
        hash_str("hi"),
        "direct shaping and authoring use the same canonical text hash",
    );
    assert_eq!(
        c.measure("hi", shape(16.0)).key,
        base,
        "the same request must be deterministic",
    );
}

#[test]
fn text_shape_key_validity_is_tagged_by_text_hash() {
    assert!(TextShapeKey::INVALID.is_invalid());
    let real = TextShapeKey {
        text_hash: 1,
        ..TextShapeKey::INVALID
    };
    assert!(!real.is_invalid());
    let zero_hash = TextShapeKey {
        fit_q: LineFit::Ellipsis as u8,
        ..TextShapeKey::INVALID
    };
    assert!(zero_hash.is_invalid());
}

#[test]
fn invalid_metrics_panic_before_any_shaping_dispatch() {
    use crate::primitives::approx::EPS;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let cases = [
        ("zero font", 0.0, 16.0),
        ("negative font", -1.0, 16.0),
        ("sub-epsilon font", EPS * 0.5, 16.0),
        ("epsilon font", EPS, 16.0),
        ("NaN font", f32::NAN, 16.0),
        ("infinite font", f32::INFINITY, 16.0),
        ("zero line height", 16.0, 0.0),
        ("negative line height", 16.0, -1.0),
        ("sub-epsilon line height", 16.0, EPS * 0.5),
        ("epsilon line height", 16.0, EPS),
        ("NaN line height", 16.0, f32::NAN),
        ("infinite line height", 16.0, f32::INFINITY),
    ];
    let mono = TextShaper::test_mono();
    let cosmic = TextShaper::new();
    for (label, font_size_px, line_height_px) in cases {
        let params = shape(font_size_px).leading(line_height_px);
        for shaper in [&mono, &cosmic] {
            let calls = shaper.measure_calls();
            assert!(
                catch_unwind(AssertUnwindSafe(|| shaper.measure("hi", params))).is_err(),
                "{label}: invalid metrics must panic at request construction",
            );
            assert_eq!(
                shaper.measure_calls(),
                calls,
                "{label}: invalid metrics reached a shaping dispatch",
            );
        }
    }
}

#[test]
fn identity_cache_rejects_invalid_metrics_before_dispatch() {
    use crate::primitives::approx::EPS;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let mut text = TextSystem::cosmic();
    let widget_id = WidgetId::from_hash("invalid metrics");
    let calls = text.shaper.measure_calls();

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        text.shape_run(
            slot(widget_id),
            "hi",
            shape(EPS * 0.5)
                .leading(16.0)
                .width(40.0)
                .halign(HAlign::Center),
            TextWrap::Ellipsis,
        )
    }))
    .is_err();
    assert!(
        panicked,
        "invalid metrics must panic at request construction"
    );
    assert!(
        !text.has_entry(widget_id, 0),
        "invalid metrics entered the reuse cache",
    );
    assert_eq!(
        text.shaper.measure_calls(),
        calls,
        "invalid metrics reached a shaping dispatch",
    );
}

#[test]
fn bounded_width_canonicalizes_and_rejects_non_finite_values() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let base = shape(16.0).leading(19.2);
    let shaper = TextShaper::new();
    let unbounded = shaper.measure("hi", base);
    assert!(
        unbounded.key.max_width_px().is_none(),
        "None is the unbounded form",
    );
    let zero = shaper.measure("hi", base.width(0.0));
    assert_eq!(
        zero.key.max_width_px(),
        Some(0.0),
        "zero is a valid bounded width",
    );
    // Negative widths (over-constrained layouts) clamp to the zero-width key.
    let negative = shaper.measure("hi", base.width(-1.0));
    assert_eq!(negative.key, zero.key);
    for (label, width) in [
        ("NaN", f32::NAN),
        ("positive infinity", f32::INFINITY),
        ("negative infinity", f32::NEG_INFINITY),
    ] {
        let params = base.width(width);
        let calls = shaper.measure_calls();
        assert!(
            catch_unwind(AssertUnwindSafe(|| shaper.measure("hi", params))).is_err(),
            "{label}: non-finite width must panic at request construction",
        );
        assert_eq!(shaper.measure_calls(), calls, "{label}");
    }
}

#[test]
fn above_epsilon_metrics_survive_cache_key_canonicalization() {
    use crate::primitives::approx::EPS;

    let mut cosmic = CosmicMeasure::with_bundled_fonts();
    let result = cosmic.measure("x", shape(EPS * 2.0));
    assert!(!result.key.is_invalid());
    assert_eq!(result.key.size_q, 1);
    assert_eq!(result.key.lh_q, 1);
    assert!(cosmic.shaped_run(result.key).is_some());
}

#[test]
fn cache_key_collapses_halign_when_unbounded() {
    // Halign only moves glyphs when there is a wrap target to align
    // within, so the key folds it down to Auto without one — single-line
    // callers don't pay an N-way cache split. With a target it must
    // discriminate, or two alignments share one shaped buffer.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let key = |c: &mut CosmicMeasure, halign, max_width_px: Option<f32>| {
        let base = shape(16.0).halign(halign);
        c.measure(
            "hi",
            match max_width_px {
                Some(w) => base.width(w),
                None => base,
            },
        )
        .key
    };
    let unbounded_auto = key(&mut c, HAlign::Auto, None);
    let bounded_auto = key(&mut c, HAlign::Auto, Some(200.0));
    for halign in [HAlign::Left, HAlign::Center, HAlign::Right] {
        assert_eq!(
            key(&mut c, halign, None),
            unbounded_auto,
            "unbounded: {halign:?} must collapse to Auto in the key",
        );
        assert_ne!(
            key(&mut c, halign, Some(200.0)),
            bounded_auto,
            "wrap-bounded: {halign:?} must enter the key",
        );
    }
}

#[test]
fn bounded_identity_cache_keys_width_and_halign() {
    let mut text = TextSystem::cosmic();
    let wid = WidgetId::from_hash("w");
    let params = shape(16.0);
    text.shape_run(slot(wid), "hi", params, TextWrap::SingleLine);
    let baseline = text.shaper.measure_calls();

    let wrap_at = |text: &mut TextSystem, width, halign| {
        text.shape_run(
            slot(wid),
            "hi",
            params.width(width).halign(halign),
            TextWrap::Wrap,
        );
        text.shaper.measure_calls()
    };

    let after_left = wrap_at(&mut text, 200.0, HAlign::Left);
    assert_eq!(after_left, baseline + 1, "first wrap shape must dispatch");
    assert_eq!(
        wrap_at(&mut text, 200.0, HAlign::Left),
        after_left,
        "identical wrap call must hit cache",
    );
    assert_eq!(
        wrap_at(&mut text, 200.0, HAlign::Right),
        after_left + 1,
        "halign change at same target must bust wrap reuse",
    );
    assert_eq!(
        wrap_at(&mut text, 201.0, HAlign::Right),
        after_left + 2,
        "width change must bust wrap reuse",
    );
}

/// Inputs that quantize to one key must shape from that key's canonical
/// values, so whichever sub-bucket value inserts first cannot alter the
/// measured extent or glyph positions.
#[test]
fn quantized_key_shaping_is_insertion_order_independent() {
    let text = "canonical text wraps onto more than one aligned line";
    let first = shape(16.001)
        .leading(19.201)
        .width(101.001)
        .halign(HAlign::Right);
    let second = first.font_size(16.006).leading(19.206).width(101.006);

    let mut first_then_second = CosmicMeasure::with_bundled_fonts();
    let a = first_then_second.measure(text, first);
    let a_hit = first_then_second.measure(text, second);
    let mut second_then_first = CosmicMeasure::with_bundled_fonts();
    let b = second_then_first.measure(text, second);
    let b_hit = second_then_first.measure(text, first);

    assert_eq!(a.key, a_hit.key);
    assert_eq!(a.key, b.key);
    assert_eq!(a.key, b_hit.key);
    assert_eq!(a.size, a_hit.size);
    assert_eq!(a.size, b.size);
    assert_eq!(a.size, b_hit.size);
    assert_eq!(a.intrinsic_min, b.intrinsic_min);
    assert_eq!(
        glyph_positions(&first_then_second, a.key),
        glyph_positions(&second_then_first, b.key),
    );
}

/// `ShapedTextRef` is the render-handoff pairing of a shaped-buffer key
/// with its record-store source bytes: `new` pins the pairing against the
/// recorded content hash, `resolve_request` restores the exact request.
#[test]
fn shaped_text_ref_resolves_the_recorded_pair_and_rejects_mismatches() {
    let store = RecordStore::default();
    let recorded = store.record_text(store.intern_str("hi"));
    assert_eq!(recorded.hash, hash_str("hi"));
    let key = TextShapeKey::unbounded(
        recorded.hash,
        GlyphFont {
            size_px: 16.0,
            line_height_px: 19.2,
            family: FontFamily::Sans,
            weight: FontWeight::Regular,
        },
    );
    let text_ref = ShapedTextRef::new(key, &recorded);
    let other = store.record_text(store.intern_str("bye"));

    {
        let payloads = store.payloads.borrow();
        let interned = payloads.interned_text();
        let request = text_ref.resolve_request(&interned);
        assert_eq!(request.text, "hi");
        assert_eq!(request.key, key);
    }

    // Pairing a key with a different run's source bytes is the logic
    // error the constructor's O(1) hash comparison pins.
    let mismatch = std::panic::catch_unwind(|| ShapedTextRef::new(key, &other));
    assert!(
        mismatch.is_err(),
        "mismatched key/source pairing must panic"
    );
}
