use super::*;
use crate::common::hash;
use crate::internals::panic_probe;
use crate::primitives::text::recorded_text::RecordedText;

#[test]
fn cache_key_discriminates_every_shaping_axis() {
    type Field = fn(TextShapeKey) -> u32;
    // Shaped buffers are cached by key, so any input that changes glyph positions must change the key.
    let mut c = CosmicMeasure::default();
    let base = c.measure("hi", shape(16.0)).buffer_key();

    let cases: [(&str, _, Field, _); 5] = [
        (
            "font size",
            shape(20.0),
            |k: TextShapeKey| k.font_size().to_bits(),
            base.font_size().to_bits(),
        ),
        (
            "line height",
            shape(16.0).leading(24.0),
            |k: TextShapeKey| k.line_height().to_bits(),
            base.line_height().to_bits(),
        ),
        (
            "family",
            shape(16.0).family(FontFamily::MONO),
            |k: TextShapeKey| u32::from(k.family().get()),
            u32::from(base.family().get()),
        ),
        (
            "weight",
            shape(16.0).weight(FontWeight::BOLD),
            |k: TextShapeKey| u32::from(k.weight().get()),
            u32::from(base.weight().get()),
        ),
        (
            "slant",
            shape(16.0).slant(FontSlant::Italic),
            |k: TextShapeKey| k.slant() as u32,
            base.slant() as u32,
        ),
    ];
    for (label, variant, field, base_field) in cases {
        let key = c.measure("hi", variant).buffer_key();
        assert_ne!(base, key, "{label} must enter the cache key");
        assert_ne!(
            field(key),
            base_field,
            "{label} is the discriminating field"
        );
    }

    assert_eq!(base.family(), FamilyKey::from_raw(0));
    assert_eq!(base.weight(), FontWeight::REGULAR);
    assert_eq!(base.slant(), FontSlant::Normal);
    assert_eq!(
        base.text_hash,
        TextShapeKey::content_hash(hash::str("hi")),
        "direct shaping and authoring use the same canonical text hash",
    );
    assert_eq!(
        c.measure("hi", shape(16.0)).buffer_key(),
        base,
        "the same request must be deterministic",
    );
}

/// A run with no key costs no more than one with a key, and no minted key holds the niche's bit pattern: without the mapping a string with raw hash zero cannot be keyed; without the width `ShapedText` grows by eight bytes per run.
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
                TextShapeKey::unbounded(raw, keyed(face)).text_hash,
                TextShapeKey::content_hash(raw),
                "a minted key carries the mapped hash, not the raw one",
            );
        }
    }
}

/// A face the shaper cannot be asked for measures to nothing and never reaches a dispatch (no buffer key, zero extent). An answer rather than a panic, since `GlyphFont` is public.
#[test]
fn invalid_metrics_measure_to_nothing_without_a_shaping_dispatch() {
    use crate::primitives::geometry::size::Size;
    use crate::primitives::math::domain::EPS;

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
    for (label, font_size, line_height) in cases {
        let params = shape(font_size).leading(line_height);
        for shaper in [&mono, &cosmic] {
            let calls = shaper.measure_calls();
            let shaped = shaper.measure("hi", params);
            assert_eq!(
                shaped.extent.size,
                Size::ZERO,
                "{label}: must measure nothing"
            );
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
    use crate::primitives::math::domain::EPS;
    let mut text = TextSystem::cosmic();
    let widget_id = WidgetId::from_hash("invalid metrics");
    let calls = text.shaper().measure_calls();

    panic_probe::assert_panics_with("a shaping fixture needs text and a usable face", || {
        text.shape_run(
            slot(widget_id),
            "hi",
            shape(EPS * 0.5)
                .leading(16.0)
                .width(40.0)
                .halign(HAlign::Center),
            TextWrap::Ellipsis,
        )
    });
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
        unbounded.max_width().is_none(),
        "None is the unbounded form",
    );
    let zero = shaper.measure("hi", base.width(0.0)).buffer_key();
    assert_eq!(zero.max_width(), Some(0.0), "zero is a valid bounded width");
    // Negative widths (over-constrained layouts) clamp to the zero-width key.
    let negative = shaper.measure("hi", base.width(-1.0)).buffer_key();
    assert_eq!(negative, zero);
    // A width that names no width binds nothing: the run keeps its unbounded shape, since `TextRun::max_width` is a public field.
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
        assert_eq!(bound.extent.size, natural.extent.size, "{label}");
    }
}

#[test]
fn above_epsilon_metrics_survive_cache_key_canonicalization() {
    use crate::primitives::math::domain::EPS;

    let mut cosmic = CosmicMeasure::default();
    let key = cosmic.measure("x", shape(EPS * 2.0)).buffer_key();
    // Both floor onto the key's 1/64-px grid, not zero.
    assert_eq!(key.font_size(), 1.0 / 64.0);
    assert_eq!(key.line_height(), 1.0 / 64.0);
    assert!(cosmic.resident_run(key).is_some());
}

#[test]
fn cache_key_collapses_halign_when_unbounded() {
    // Halign moves glyphs only with a wrap target, so the key folds it to Auto without one; with a target it must discriminate.
    let mut c = CosmicMeasure::default();
    let key = |c: &mut CosmicMeasure, halign, max_width: Option<f32>| {
        let base = shape(16.0).halign(halign);
        c.measure(
            "hi",
            match max_width {
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
    let widget_id = WidgetId::from_hash("w");
    let params = shape(16.0);
    text.shape_run(slot(widget_id), "hi", params, TextWrap::SingleLine);
    let baseline = text.shaper().measure_calls();

    let wrap_at = |text: &mut TextSystem, width, halign| {
        text.shape_run(
            slot(widget_id),
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

/// Inputs that quantize to one key shape from its canonical values.
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
        glyph_poss(&first_then_second, a.buffer_key()),
        glyph_poss(&second_then_first, b.buffer_key()),
    );
}

fn key_for(recorded: &RecordedText) -> TextShapeKey {
    TextShapeKey::unbounded(
        recorded.hash,
        keyed(GlyphFont {
            size: 16.0,
            line_height: 19.2,
            family: FontFamily::SANS,
            weight: FontWeight::REGULAR,
            slant: FontSlant::Normal,
        }),
    )
}

/// `ShapedTextRef` pairs a shaped-buffer key with its source bytes; `resolve_request` restores the exact request the backend replays.
#[test]
fn shaped_text_ref_resolves_the_recorded_pair() {
    let mut store = RecordStore::default();
    let interned = store.intern("hi");
    let recorded = store.record_text(interned);
    assert_eq!(recorded.hash, hash::str("hi"));
    let key = key_for(&recorded);
    let text_ref = ShapedTextRef::new(key, &recorded);

    let interned = store.interned_text();
    let request = text_ref.resolve_request(&interned);
    assert_eq!(request.text, "hi");
    assert_eq!(request.key, key);
}

/// Pairing a key with another run's bytes would replay the wrong buffer. Debug-only: the encoder mints one per text run per frame.
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

/// A key carries a hash of its text, so two texts can share one. A hit compares the bytes, and a
/// lookup that finds another text's buffer evicts it and shapes its own. Forged here by pairing one
/// key with two texts: each line ending is part of the bytes. A truncated entry compares the source
/// it was cut from, not the `prefix + "…"` its buffer holds.
#[test]
fn a_hit_is_verified_against_the_bytes() {
    let face = shape(14.0).leading(18.0);
    for (text, other) in [
        ("hello", "world"),
        ("a\nb", "a\r\nb"),
        ("a\r\nb\n", "a\r\nb"),
        ("x\r", "x\n"),
        ("one\n\rtwo", "one\ntwo"),
    ] {
        let mut c = CosmicMeasure::default();
        let key = c.measure(text, face).buffer_key();
        let before = c.cache_counts();
        c.root(TextShapeRequest { text, key }, WrapFloor::Skip);
        let genuine = c.cache_counts() - before;
        assert_eq!(
            (genuine.hits, genuine.shapes, genuine.collisions),
            (1, 0, 0),
            "{text:?} hits its own buffer",
        );

        let forged = TextShapeRequest { text: other, key };
        let root = c.root(forged, WrapFloor::Skip);
        let collided = c.cache_counts() - before;
        assert_eq!(
            (collided.shapes, collided.collisions),
            (1, 1),
            "{other:?} under {text:?}'s key reshapes",
        );
        let fresh = CosmicMeasure::default().measure(other, face);
        assert_eq!(root.extent.size, fresh.size, "{other:?}");
        assert!(c.shaped_run(forged).is_some());
        assert!(c.shaped_run(TextShapeRequest { text, key }).is_none());
    }

    let long = "a label far too long for its slot";
    let narrow = face.width(60.0);
    let mut c = CosmicMeasure::default();
    let key = c
        .truncate(long, narrow, LineFit::Ellipsis)
        .fitted
        .buffer_key();
    let before = c.cache_counts();
    c.resolve(TextShapeRequest { text: long, key });
    let genuine = c.cache_counts() - before;
    assert_eq!(
        (genuine.hits, genuine.collisions),
        (1, 0),
        "the cut compares its source"
    );
    let other = "another label too long for its slot";
    let extent = c.resolve(TextShapeRequest { text: other, key });
    assert!((c.cache_counts() - before).collisions >= 1);
    let fresh = CosmicMeasure::default().measure_truncated(other, narrow, LineFit::Ellipsis);
    assert_eq!(extent.size, fresh.size);
}
