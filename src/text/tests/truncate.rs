use super::*;

#[test]
fn fitting_truncate_returns_the_unbounded_root_without_reshaping() {
    let shaper = TextShaper::new();
    let mut text = TextSystem::new(shaper.clone());
    let wid = WidgetId::from_hash("fitting truncate");
    let fitting = TestShape {
        max_width_px: Some(200.0),
        halign: HAlign::Center,
        ..shape(16.0)
    };
    let unbounded_shape = TestShape {
        max_width_px: None,
        halign: HAlign::Auto,
        ..fitting
    };

    for (ordinal, wrap) in [(0u16, TextWrap::Truncate), (1, TextWrap::Ellipsis)] {
        let run_slot = slot_at(wid, ordinal);
        let fit = wrap.line_fit().unwrap();
        let natural = text.shape_run(run_slot, "ok", unbounded_shape, wrap);
        let calls = text.shaper.measure_calls();

        let fitted = text.shape_run(run_slot, "ok", fitting, wrap);
        assert_eq!(
            fitted.key, natural.key,
            "a fitting {wrap:?} must reuse the unbounded root's identity",
        );
        assert_eq!(fitted.size, natural.size);
        assert_eq!(fitted.intrinsic_min, natural.intrinsic_min);
        assert_eq!(
            text.shaper.measure_calls(),
            calls,
            "a fitting {wrap:?} must not dispatch a second shape",
        );
        let bounded_key = fitting.request("ok", fit).key;
        assert!(
            !text.shaper.has_cosmic_buffer(bounded_key),
            "a fitting {wrap:?} must not mint a bounded cache entry",
        );

        // Over-wide text still resolves through the truncating path.
        let narrow = TestShape {
            max_width_px: Some(20.0),
            ..fitting
        };
        let truncated = text.shape_run(run_slot, "wider than twenty", narrow, wrap);
        assert_ne!(truncated.key, truncated.key.unbounded_version());
        assert_eq!(truncated.key.fit_q, fit as u8);
        assert!(truncated.size.w <= 20.0);
    }

    // A multi-line source collapses to its first line under Clip/Ellipsis,
    // so the unbounded root cannot stand in even when its widest line fits.
    let multiline = text.shape_run(slot_at(wid, 2), "a\nb", fitting, TextWrap::Ellipsis);
    let bounded_key = fitting.request("a\nb", LineFit::Ellipsis).key;
    assert_eq!(
        multiline.key, bounded_key,
        "multi-line text must resolve through the truncating path",
    );
    // One line at the 16 px leading `fitting` carries — the collapse is
    // what makes a two-line source measure a single line tall.
    assert_eq!(multiline.size.h, 16.0);
}

#[test]
fn cosmic_ellipsis_elides_long_line_to_width() {
    // A label wider than the committed width truncates to one line that
    // fits, with a trailing ellipsis. Pins the "labels never overflow
    // their box" contract the Button relies on.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "Screenshot 2026-05-28 at 01.21.25.png";
    let w = 120.0;
    let elided = measure_truncated(
        &mut c,
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
        LineFit::Ellipsis,
    );
    // Precondition: the natural single line genuinely overflows `w`.
    let full = c.measure(long, shape(16.0));
    assert!(
        full.size.w > w,
        "precondition: natural line ({}) must overflow the cap ({w})",
        full.size.w,
    );
    // Elided result fits the cap (ceil tolerance) and stays one line.
    assert!(
        elided.size.w <= w + 1.0,
        "elided width {} must fit cap {w}",
        elided.size.w,
    );
    assert!(
        elided.size.h <= (16.0 * LINE_HEIGHT_MULT).ceil() + 0.5,
        "elided run must be a single line, got h={}",
        elided.size.h,
    );
    assert_eq!(
        elided.intrinsic_min,
        Some(0.0),
        "an elided run has zero min floor"
    );
    let zero_width = measure_truncated(
        &mut c,
        long,
        TestShape {
            max_width_px: Some(0.0),
            ..shape(16.0)
        },
        LineFit::Ellipsis,
    );
    assert_eq!(
        zero_width.size.w, 0.0,
        "an ellipsis that cannot fit collapses to zero width",
    );
    // The elided buffer must not collide with the *wrapped* buffer at the
    // same width — they hold different strings, so distinct cache keys.
    let wrapped = c.measure(
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
    );
    assert_ne!(
        elided.key, wrapped.key,
        "elision and wrap must key distinct cache slots at the same width",
    );
}

#[test]
fn fitting_prefix_cuts_on_logical_cluster_boundaries() {
    // Hand-built glyph runs, so the cut is checked against arithmetic
    // rather than against whatever the installed fonts happen to measure.
    // Each entry is (start, end, advance) in *visual* order.
    type Run = &'static [(usize, usize, f32)];
    // "abc", 10 px per glyph, LTR: visual order is logical order.
    const LTR: Run = &[(0, 1, 10.0), (1, 2, 10.0), (2, 3, 10.0)];
    // The same three glyphs read right-to-left: the logically-first glyph
    // is emitted last. A cut driven by visual order would keep the wrong
    // end of the run.
    const RTL: Run = &[(2, 3, 10.0), (1, 2, 10.0), (0, 1, 10.0)];
    // "a🇺🇸": one cluster (bytes 1..9) shaping to two 10 px glyphs. Paying
    // for one of them must not commit the whole cluster's bytes.
    const CLUSTER: Run = &[(0, 1, 10.0), (1, 9, 10.0), (1, 9, 10.0)];
    // "á" decomposed: a zero-width mark glyph sharing the base's cluster
    // costs nothing, so it must not hold the cut back.
    const MARK: Run = &[(0, 3, 10.0), (0, 3, 0.0), (3, 4, 10.0)];

    const ANY: usize = usize::MAX;
    let mut order = Vec::new();
    for (run, avail, max_end, expected, why) in [
        (LTR, 0.0, ANY, 0, "no budget keeps nothing"),
        (LTR, 9.9, ANY, 0, "a glyph is all-or-nothing"),
        (LTR, 10.0, ANY, 1, "an exact fit is a fit"),
        (LTR, 25.0, ANY, 2, "the third glyph would overrun"),
        (LTR, 30.0, ANY, 3, "the whole run fits"),
        (LTR, 1000.0, ANY, 3, "surplus budget keeps the whole run"),
        (
            RTL,
            10.0,
            ANY,
            1,
            "RTL keeps the logical prefix, not the visual one",
        ),
        (RTL, 25.0, ANY, 2, "RTL cut tracks logical order"),
        (RTL, 30.0, ANY, 3, "the whole RTL run fits"),
        (
            CLUSTER,
            10.0,
            ANY,
            1,
            "one glyph of the cluster is unaffordable",
        ),
        (
            CLUSTER,
            25.0,
            ANY,
            1,
            "25 px pays for only one of the two cluster glyphs",
        ),
        (CLUSTER, 30.0, ANY, 9, "30 px pays for the whole cluster"),
        (
            MARK,
            10.0,
            ANY,
            3,
            "a zero-width mark rides along with its base",
        ),
        (MARK, 20.0, ANY, 4, "the following glyph is affordable too"),
        // `max_end` drives the back-off: feeding back the previous answer
        // must retire at least one more cluster, all the way to nothing.
        (LTR, 1000.0, 3, 2, "the bound retires the last glyph"),
        (LTR, 1000.0, 2, 1, "and the one before it"),
        (LTR, 1000.0, 1, 0, "and the last one standing"),
        (LTR, 1000.0, 0, 0, "an exhausted bound stays at nothing"),
        (RTL, 1000.0, 3, 2, "the bound reads logical order too"),
        (
            CLUSTER,
            1000.0,
            9,
            1,
            "backing off a cluster retires all of its glyphs",
        ),
    ] {
        let cut = cosmic::fitting_prefix(
            run.len(),
            |i| ClusterGlyph {
                start: run[i].0,
                end: run[i].1,
                advance: run[i].2,
            },
            &mut order,
            avail,
            max_end,
        );
        assert_eq!(cut, expected, "avail={avail} max_end={max_end}: {why}");
        // Every bounded cut falls strictly below its bound, so feeding the
        // previous answer back always makes progress — that is what makes
        // the back-off terminate. Zero is the floor it terminates *at*: the
        // production loop stops on an empty cut and never re-bounds by it.
        assert!(
            max_end == ANY || max_end == 0 || cut < max_end,
            "a bounded cut must fall strictly below its bound",
        );
    }
}

#[test]
fn ellipsis_never_measures_wider_than_its_budget() {
    // Two ways a cut overruns the budget it was measured against: paying
    // for only some of a cluster's glyphs while committing all of its bytes
    // (flag and ZWJ emoji), and reshaping a prefix whose last letter changes
    // form once it lands at a word end (Arabic medial → final). Both resolve
    // through fonts this crate does not bundle, so the bound — never an exact
    // width — is what holds on every machine.
    let base = shape(16.0);
    for text in [
        "flag \u{1f1fa}\u{1f1f8} emoji \u{1f600} run",
        "\u{1f469}\u{200d}\u{1f469}\u{200d}\u{1f467} family emoji",
        "\u{627}\u{644}\u{633}\u{644}\u{627}\u{645} \u{639}\u{644}\u{64a}\u{643}\u{645}",
        "\u{645}\u{631}\u{62d}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645}",
    ] {
        for family in [FontFamily::Sans, FontFamily::Mono] {
            for fit in [LineFit::Clip, LineFit::Ellipsis] {
                let mut c = CosmicMeasure::with_bundled_fonts();
                for width_px in 0..=160 {
                    let width = width_px as f32;
                    let m = measure_truncated(
                        &mut c,
                        text,
                        TestShape {
                            max_width_px: Some(width),
                            family,
                            ..base
                        },
                        fit,
                    );
                    // Widths are whole pixels and `size.w` is ceil'd, so a
                    // run that fits its budget cannot round past it.
                    assert!(
                        m.size.w <= width,
                        "{family:?} {fit:?} {text:?}: measured {} against budget {width}",
                        m.size.w,
                    );
                }
            }
        }
    }
}

#[test]
fn ellipsis_keeps_the_logical_prefix_in_both_reading_directions() {
    // The cut walks the cached unbounded shape's glyphs, which arrive in
    // *visual* order. In an RTL run the logically-first glyph sits at the
    // right edge and trailing edges descend, so a cut driven by `x + w`
    // stops at the first glyph and drops the whole run.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let unbounded = shape(16.0);
    let elide = |c: &mut CosmicMeasure, text: &str, width: f32| {
        measure_truncated(
            c,
            text,
            TestShape {
                max_width_px: Some(width),
                ..unbounded
            },
            LineFit::Ellipsis,
        )
        .size
        .w
    };

    let rtl = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}";
    let marker_only = c.measure("\u{2026}", unbounded).size.w;

    // Room for the marker plus one glyph: something of the run must survive.
    let one = elide(&mut c, rtl, 28.0);
    assert!(
        one > marker_only,
        "an RTL run with room to spare must keep text, got {one} vs bare marker {marker_only}",
    );

    // Room for two: the survivors must be the logical prefix. The logical
    // *suffix* is the wrong answer a visual-order cut reaches for, and
    // Hebrew glyph advances differ enough to tell the two apart.
    let two = elide(&mut c, rtl, 35.0);
    let prefix = c.measure("\u{5e9}\u{5dc}\u{2026}", unbounded).size.w;
    let suffix = c.measure("\u{5d5}\u{5dd}\u{2026}", unbounded).size.w;
    assert!(
        (two - prefix).abs() < 1.0,
        "RTL elision must keep the leading characters: {two} vs prefix {prefix}",
    );
    assert!(
        (two - suffix).abs() >= 1.0,
        "prefix and suffix widths must differ for this to prove anything: {two} vs {suffix}",
    );

    // LTR is the control: same code path, and widening the box must reveal
    // more of the run rather than less.
    let narrow = elide(&mut c, "abcd", 20.0);
    let wide = elide(&mut c, "abcd", 28.0);
    assert!(
        wide > narrow,
        "a wider box must keep more of an LTR run: {wide} vs {narrow}",
    );
}

#[test]
fn cosmic_ellipsis_short_text_not_truncated() {
    // A label that already fits the cap is shaped whole — no spurious
    // ellipsis, width matches the natural measurement.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let short = "ok";
    let natural = c.measure(short, shape(16.0));
    let elided = measure_truncated(
        &mut c,
        short,
        TestShape {
            max_width_px: Some(200.0),
            ..shape(16.0)
        },
        LineFit::Ellipsis,
    );
    assert!(
        (elided.size.w - natural.size.w).abs() <= 2.0,
        "short text must not truncate: elided {} vs natural {}",
        elided.size.w,
        natural.size.w,
    );
}

#[test]
fn cosmic_truncate_fits_measures_natural_width_regardless_of_halign() {
    // Regression: a single-line label that fits a wide cap must measure to
    // its natural glyph width, not inflate toward the box, even with a
    // non-`Auto` halign (the encoder positions the line; the shaped buffer
    // must not bake in width + per-line align). A `Center`-aligned label in
    // a 400 px cap previously measured ~half the box wide.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let label = "File";
    let cap = 400.0;
    let natural = c.measure(label, shape(16.0));
    for fit in [false, true] {
        let m = measure_truncated(
            &mut c,
            label,
            TestShape {
                max_width_px: Some(cap),
                halign: HAlign::Center,
                ..shape(16.0)
            },
            if fit {
                LineFit::Ellipsis
            } else {
                LineFit::Clip
            },
        );
        assert!(
            (m.size.w - natural.size.w).abs() <= 2.0,
            "centered fitting label must measure natural width ({}), got {} (with_ellipsis={fit})",
            natural.size.w,
            m.size.w,
        );
    }
}

#[test]
fn cosmic_singleline_clips_to_width_without_ellipsis() {
    // The default `SingleLine` mode (clip, no marker) cuts an over-wide
    // label to fit the cap on one line — like the ellipsis path but with no
    // trailing `…`, and reserving no room for one. Distinct cache slot from
    // both the wrapped and the ellipsized buffers at the same width.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "Screenshot 2026-05-28 at 01.21.25.png";
    let w = 120.0;
    let full = c.measure(long, shape(16.0));
    assert!(
        full.size.w > w,
        "precondition: natural line ({}) must overflow the cap ({w})",
        full.size.w,
    );
    let clipped = measure_truncated(
        &mut c,
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
        LineFit::Clip,
    );
    assert!(
        clipped.size.w <= w + 1.0,
        "clipped width {} must fit cap {w}",
        clipped.size.w,
    );
    assert!(
        clipped.size.h <= (16.0 * LINE_HEIGHT_MULT).ceil() + 0.5,
        "clipped run must be a single line, got h={}",
        clipped.size.h,
    );
    assert_eq!(
        clipped.intrinsic_min,
        Some(0.0),
        "a clipped run has zero min floor"
    );
    // Clip and ellipsis cut to the same cap but bake different strings (the
    // ellipsis path appends `…` and reserves its width), so they must key
    // distinct cache slots.
    let elided = measure_truncated(
        &mut c,
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
        LineFit::Ellipsis,
    );
    // Clip, ellipsis, and wrap each bake a distinct buffer at the same width.
    let wrapped = c.measure(
        long,
        TestShape {
            max_width_px: Some(w),
            ..shape(16.0)
        },
    );
    assert_ne!(
        clipped.key, elided.key,
        "clip and ellipsis must key distinctly"
    );
    assert_ne!(
        clipped.key, wrapped.key,
        "clip and wrap must key distinctly"
    );
    assert_eq!(
        clipped.key.text_hash, full.key.text_hash,
        "bounded keys reuse the source text hash",
    );
    assert_eq!(clipped.key.fit_q, LineFit::Clip as u8);
    assert_eq!(elided.key.fit_q, LineFit::Ellipsis as u8);
    assert_eq!(wrapped.key.fit_q, LineFit::Wrap as u8);
}

#[test]
fn mono_ellipsis_caps_width_with_zero_floor() {
    // Mono fallback: an elided long word caps at the available width and
    // reports zero min-content (shrinks to the ellipsis); the wrap
    // counterpart instead grows height and keeps the longest-word floor.
    let long = "abcdefghijklmnop"; // 16 ASCII bytes × 8 px = 128 px natural
    let w = 40.0;
    let elided = mono_shape(long, 16.0, 16.0, Some(w), LineFit::Ellipsis);
    assert_eq!(elided.size.w, w, "elided mono caps at the width");
    assert_eq!(elided.size.h, 16.0, "elided mono is one line");
    assert_eq!(
        elided.intrinsic_min,
        Some(0.0),
        "elided mono has zero floor"
    );
    let wrapped = mono_shape(long, 16.0, 16.0, Some(w), LineFit::Wrap);
    assert!(wrapped.size.h > 16.0, "wrap grows height across lines");
    assert!(
        wrapped.wrap_floor() > 0.0,
        "wrap keeps a longest-word floor"
    );
}

/// Truncation reads its probe glyphs from the cached unbounded buffer.
/// Measure the same input on a fresh measurer and one containing unrelated
/// cached shapes; both the derived key and exact measurement must agree.
#[test]
fn truncation_from_cached_unbounded_is_order_independent() {
    let long = "the quick brown fox jumps over the lazy dog";
    let (fs, w) = (14.0, 80.0);

    // Fresh measurer: only the target measurement.
    let mut fresh = CosmicMeasure::with_bundled_fonts();
    let r_fresh = measure_truncated(
        &mut fresh,
        long,
        TestShape {
            max_width_px: Some(w),
            halign: HAlign::Left,
            ..shape(fs)
        },
        LineFit::Ellipsis,
    );

    // Reused measurer: populate unrelated unbounded, truncated, and ellipsis
    // cache entries first, then measure the identical target.
    let mut reused = CosmicMeasure::with_bundled_fonts();
    measure_truncated(
        &mut reused,
        "a considerably longer string that grows the probe buffer capacity",
        TestShape {
            max_width_px: Some(220.0),
            family: FontFamily::Mono,
            halign: HAlign::Left,
            ..shape(20.0)
        },
        LineFit::Ellipsis,
    );
    measure_truncated(
        &mut reused,
        "short",
        TestShape {
            max_width_px: Some(30.0),
            halign: HAlign::Left,
            ..shape(10.0)
        },
        LineFit::Clip,
    );
    let r_reused = measure_truncated(
        &mut reused,
        long,
        TestShape {
            max_width_px: Some(w),
            halign: HAlign::Left,
            ..shape(fs)
        },
        LineFit::Ellipsis,
    );

    assert_eq!(
        r_fresh.size, r_reused.size,
        "unrelated cached buffers changed the measured size",
    );
    assert_eq!(
        r_fresh.key, r_reused.key,
        "same inputs must map to the same cache key regardless of prior shaping",
    );

    // Truncation actually fired: the ellipsized line is narrower than the
    // full unbounded shape (and fits within the width budget).
    let unbounded = fresh.measure(
        long,
        TestShape {
            halign: HAlign::Left,
            ..shape(fs)
        },
    );
    assert!(
        r_fresh.size.w < unbounded.size.w,
        "expected truncation: ellipsized {} should be < unbounded {}",
        r_fresh.size.w,
        unbounded.size.w,
    );
    assert!(
        r_fresh.size.w <= w + 1.0,
        "ellipsized width {} should fit within budget {w}",
        r_fresh.size.w,
    );
}

/// A continuous font-size zoom over ellipsized text mints a distinct
/// quantized size every frame, so the ellipsis reservation is recomputed
/// throughout. Drive a long sweep of sizes through one budget and assert
/// every one still lands inside it.
#[test]
fn ellipsis_stays_within_budget_under_size_churn() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "the quick brown fox jumps over the lazy dog";
    let width = 60.0;
    for i in 0..261 {
        // Distinct quantized size each iteration (0.1px steps × 64 ≥ 1).
        let fs = 8.0 + i as f32 * 0.1;
        let r = measure_truncated(
            &mut c,
            long,
            TestShape {
                max_width_px: Some(width),
                halign: HAlign::Left,
                ..shape(fs)
            },
            LineFit::Ellipsis,
        );
        assert!(
            r.size.w <= width,
            "size {fs} measured {} against budget {width}",
            r.size.w,
        );
    }
}

/// A truncating fit paints exactly one visual line, including when the
/// source has a hard newline in it.
///
/// The cut is taken from the *first layout run* of the cached unbounded
/// probe, so everything past the newline is dropped before the prefix is
/// ever reshaped. Worth pinning separately from the width cases: a
/// truncating fit that measured two lines would break every caller that
/// sizes a row from it.
#[test]
fn a_truncating_fit_paints_one_line_even_across_a_newline() {
    let mut c = CosmicMeasure::with_bundled_fonts();
    let text = "first paragraph here\nsecond paragraph";
    let params = TestShape {
        max_width_px: Some(90.0),
        ..shape(16.0)
    };
    let one_line = 16.0_f32;

    for fit in [LineFit::Clip, LineFit::Ellipsis] {
        let r = measure_truncated(&mut c, text, params, fit);
        assert_eq!(
            r.size.h, one_line,
            "{fit:?} must measure one line, got h={}",
            r.size.h,
        );
        assert!(
            r.size.w <= 90.0,
            "{fit:?} must fit the committed width, got w={}",
            r.size.w,
        );
        // Every glyph kept belongs to the first line: none carries a
        // source offset from past the newline, and none sits below the
        // first line's band.
        let newline = text.find('\n').unwrap();
        for g in glyph_positions(&c, r.key) {
            assert!(
                g.start < newline,
                "{fit:?} kept a glyph from the second paragraph (byte {})",
                g.start,
            );
            assert_eq!(g.line_top, 0.0, "{fit:?} kept a glyph off line 0");
        }
    }

    // The contrast: wrapping the same text keeps both paragraphs.
    let wrapped = c.measure(text, params);
    assert!(
        wrapped.size.h > one_line * 2.0,
        "premise: wrapped keeps every line, got h={}",
        wrapped.size.h,
    );
}
