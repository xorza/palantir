use super::*;
use crate::common::hash;
use crate::primitives::recorded_text::RecordedText;

#[test]
fn cache_key_discriminates_every_shaping_axis() {
    // The renderer caches shaped buffers by key, so any input that changes
    // glyph positions has to change the key. Miss one and a buffer shaped
    // for other parameters gets replayed — measured rect against the wrong
    // rasterized glyphs.
    let mut c = CosmicMeasure::default();
    let base = c.measure("hi", shape(16.0)).buffer_key();

    for (label, variant, field, base_field) in [
        (
            "font size",
            shape(20.0),
            (|k: TextShapeKey| k.font_size_px().to_bits()) as fn(TextShapeKey) -> u32,
            base.font_size_px().to_bits(),
        ),
        (
            "line height",
            shape(16.0).leading(24.0),
            (|k: TextShapeKey| k.line_height_px().to_bits()) as fn(TextShapeKey) -> u32,
            base.line_height_px().to_bits(),
        ),
        (
            "family",
            shape(16.0).family(FontFamily::MONO),
            (|k: TextShapeKey| u32::from(k.family().raw())) as fn(TextShapeKey) -> u32,
            u32::from(base.family().raw()),
        ),
        (
            "weight",
            shape(16.0).weight(FontWeight::BOLD),
            (|k: TextShapeKey| u32::from(k.weight().value())) as fn(TextShapeKey) -> u32,
            u32::from(base.weight().value()),
        ),
        (
            "style",
            shape(16.0).style(FontStyle::Italic),
            (|k: TextShapeKey| k.style() as u32) as fn(TextShapeKey) -> u32,
            base.style() as u32,
        ),
    ] {
        let key = c.measure("hi", variant).buffer_key();
        assert_ne!(base, key, "{label} must enter the cache key");
        assert_ne!(
            field(key),
            base_field,
            "{label} is the discriminating field"
        );
    }

    // The axis values themselves are what land in the key, packed and
    // read back, so a shifted field can't silently remap cached buffers.
    assert_eq!(base.family(), FontFamily::SANS);
    assert_eq!(base.weight(), FontWeight::REGULAR);
    assert_eq!(base.style(), FontStyle::Normal);
    assert_eq!(
        base.text_hash,
        TextShapeKey::content_hash(hash::hash_str("hi")),
        "direct shaping and authoring use the same canonical text hash",
    );
    assert_eq!(
        c.measure("hi", shape(16.0)).buffer_key(),
        base,
        "the same request must be deterministic",
    );
}

/// A run with no key costs no more than one with a key, and no minted
/// key can hold the bit pattern that buys it.
///
/// The absent case is a `None` the type refuses to confuse with a real
/// key, so what is left to pin is the mapping that feeds the niche and
/// the width the niche saves. Both halves are load-bearing: without the
/// mapping, the string whose raw hash is zero could not be keyed at all;
/// without the width, `ShapedText` grows by eight bytes per recorded
/// run.
#[test]
fn an_absent_key_is_free_and_no_minted_key_claims_its_niche() {
    assert_eq!(
        size_of::<Option<TextShapeKey>>(),
        size_of::<TextShapeKey>(),
        "the non-zero text hash is what makes an absent key cost nothing",
    );
    assert_eq!(
        TextShapeKey::content_hash(0).get(),
        1,
        "a run whose text hashes to zero must not claim the niche",
    );
    for raw in [1, 2, u64::MAX] {
        assert_eq!(
            TextShapeKey::content_hash(raw).get(),
            raw,
            "every other hash is carried through unchanged",
        );
    }
    for face in [shape(16.0).font, shape(64.0).leading(90.0).font] {
        for raw in [0, 1, u64::MAX] {
            assert_eq!(
                TextShapeKey::unbounded(raw, face).text_hash,
                TextShapeKey::content_hash(raw),
                "a minted key carries the mapped hash, not the raw one",
            );
        }
    }
}

/// A face the shaper cannot be asked for measures to nothing, and the
/// run never reaches a dispatch.
///
/// `TextShapeRequest::unbounded` is the one screen, so this is the same
/// answer empty text gets: no buffer key and a zero extent. It has to
/// be an answer rather than a panic because `GlyphFont` is public and a
/// caller fills it — `Ui::probe_text` and `TextGlyphs` take one straight
/// from application arithmetic.
#[test]
fn invalid_metrics_measure_to_nothing_without_a_shaping_dispatch() {
    use crate::primitives::approx::EPS;
    use crate::primitives::size::Size;

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
            let shaped = shaper.measure("hi", params);
            assert_eq!(shaped.measured, Size::ZERO, "{label}: must measure nothing");
            assert!(
                shaped.key.is_none(),
                "{label}: an unshaped run carries no buffer key",
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
    let calls = text.shaper().measure_calls();

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
        "the layout-side fixture must refuse to build a request for an unusable face",
    );
    assert!(
        !text.has_entry(widget_id, 0),
        "invalid metrics entered the reuse cache",
    );
    assert_eq!(
        text.shaper().measure_calls(),
        calls,
        "invalid metrics reached a shaping dispatch",
    );
}

#[test]
fn bounded_width_canonicalizes_and_leaves_non_finite_values_unbound() {
    let base = shape(16.0).leading(19.2);
    let shaper = TextShaper::new();
    let natural = shaper.measure("hi", base);
    let unbounded = natural.buffer_key();
    assert!(
        unbounded.max_width_px().is_none(),
        "None is the unbounded form",
    );
    let zero = shaper.measure("hi", base.width(0.0)).buffer_key();
    assert_eq!(
        zero.max_width_px(),
        Some(0.0),
        "zero is a valid bounded width",
    );
    // Negative widths (over-constrained layouts) clamp to the zero-width key.
    let negative = shaper.measure("hi", base.width(-1.0)).buffer_key();
    assert_eq!(negative, zero);
    // A width that names no width binds nothing, so the run keeps the
    // unbounded shape it would have had with no width at all. Answered
    // rather than rejected because `TextRun::max_width_px` is a public
    // field a caller derives from an arranged rect.
    for (label, width) in [
        ("NaN", f32::NAN),
        ("positive infinity", f32::INFINITY),
        ("negative infinity", f32::NEG_INFINITY),
    ] {
        let bound = shaper.measure("hi", base.width(width));
        assert_eq!(
            bound.buffer_key(),
            unbounded,
            "{label}: must stay unbounded"
        );
        assert_eq!(bound.measured, natural.measured, "{label}");
    }
}

#[test]
fn above_epsilon_metrics_survive_cache_key_canonicalization() {
    use crate::primitives::approx::EPS;

    let mut cosmic = CosmicMeasure::default();
    let key = cosmic.measure("x", shape(EPS * 2.0)).buffer_key();
    // Both floored onto the key's 1/64-px grid rather than to zero, which
    // would name a face that shapes nothing.
    assert_eq!(key.font_size_px(), 1.0 / 64.0);
    assert_eq!(key.line_height_px(), 1.0 / 64.0);
    assert!(cosmic.shaped_run(key).is_some());
}

#[test]
fn cache_key_collapses_halign_when_unbounded() {
    // Halign only moves glyphs when there is a wrap target to align
    // within, so the key folds it down to Auto without one — single-line
    // callers don't pay an N-way cache split. With a target it must
    // discriminate, or two alignments share one shaped buffer.
    let mut c = CosmicMeasure::default();
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
    let baseline = text.shaper().measure_calls();

    let wrap_at = |text: &mut TextSystem, width, halign| {
        text.shape_run(
            slot(wid),
            "hi",
            params.width(width).halign(halign),
            TextWrap::Wrap,
        );
        text.shaper().measure_calls()
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

    let mut first_then_second = CosmicMeasure::default();
    let a = first_then_second.measure(text, first);
    let a_hit = first_then_second.measure(text, second);
    let mut second_then_first = CosmicMeasure::default();
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
        glyph_positions(&first_then_second, a.buffer_key()),
        glyph_positions(&second_then_first, b.buffer_key()),
    );
}

/// The key a recorded run shapes under, at the face these cases share.
fn key_for(recorded: &RecordedText) -> TextShapeKey {
    TextShapeKey::unbounded(
        recorded.hash,
        GlyphFont {
            size_px: 16.0,
            line_height_px: 19.2,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            style: FontStyle::Normal,
        },
    )
}

/// `ShapedTextRef` is the render-handoff pairing of a shaped-buffer key
/// with its record-store source bytes, and `resolve_request` restores the
/// exact request the backend replays.
#[test]
fn shaped_text_ref_resolves_the_recorded_pair() {
    let mut store = RecordStore::default();
    let interned = store.intern("hi");
    let recorded = store.record_text(interned);
    assert_eq!(recorded.hash, hash::hash_str("hi"));
    let key = key_for(&recorded);
    let text_ref = ShapedTextRef::new(key, &recorded);

    let interned = store.interned_text();
    let request = text_ref.resolve_request(&interned);
    assert_eq!(request.text, "hi");
    assert_eq!(request.key, key);
}

/// Pairing a key with a different run's source bytes would replay one
/// run's shaped buffer for another's text.
///
/// Debug-only: the encoder mints one of these per text run per frame, so
/// the compare runs at the frame's rate rather than a caller's.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "shaped-text key paired with a different run's source bytes")]
fn a_mismatched_key_and_source_are_rejected() {
    let mut store = RecordStore::default();
    let interned = store.intern("hi");
    let key = key_for(&store.record_text(interned));
    let other_interned = store.intern("bye");
    let other = store.record_text(other_interned);
    let _ = ShapedTextRef::new(key, &other);
}
