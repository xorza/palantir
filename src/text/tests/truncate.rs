use super::*;

#[test]
fn fitting_truncate_returns_the_unbounded_root_without_reshaping() {
    let mut text = TextSystem::cosmic();
    let wid = WidgetId::from_hash("fitting truncate");
    let fitting = shape(16.0).width(200.0).halign(HAlign::Center);

    // `capped_w` is the run's Inter width once the fit has cut it to the
    // 20 px bound. Ellipsis lands narrower than Truncate because the "…"
    // it appends has to fit inside the same bound.
    for (ordinal, wrap, capped_w) in [
        (0u16, TextWrap::Truncate, 17.0),
        (1, TextWrap::Ellipsis, 14.0),
    ] {
        let run_slot = slot_at(wid, ordinal);
        let fit = wrap.line_fit().unwrap();
        let natural = text.shape_run(run_slot, "ok", fitting.unbounded(), wrap);
        let calls = text.shaper().measure_calls();

        let fitted = text.shape_run(run_slot, "ok", fitting, wrap);
        assert_eq!(
            fitted.key, natural.key,
            "a fitting {wrap:?} must reuse the unbounded root's identity",
        );
        assert_eq!(fitted.size, natural.size);
        assert_eq!(fitted.intrinsic_min, natural.intrinsic_min);
        assert_eq!(
            text.shaper().measure_calls(),
            calls,
            "a fitting {wrap:?} must not dispatch a second shape",
        );
        let bounded_key = fitting.request("ok", fit).key;
        assert!(
            !text.shaper().has_cosmic_buffer(bounded_key),
            "a fitting {wrap:?} must not mint a bounded cache entry",
        );

        // Over-wide text still resolves through the truncating path.
        let truncated = text.shape_run(run_slot, "wider than twenty", fitting.width(20.0), wrap);
        assert_ne!(truncated.key, truncated.key.unbounded_version());
        assert_eq!(truncated.key.fit(), fit);
        assert_eq!(truncated.size.w, capped_w, "{wrap:?} caps inside 20 px");
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
    assert_eq!(multiline.size.h, one_line_h(fitting));
}

/// Both truncating fits cut an over-wide label to one line that fits the
/// committed width. They differ only in the marker: `Ellipsis` appends
/// `…` and reserves its advance, `Clip` cuts flush. Everything else — the
/// overflow precondition, the single-line height, the zero wrap floor,
/// and keying distinctly from each other and from the wrapped buffer at
/// the same width — is one contract stated once over both.
///
/// Pins "labels never overflow their box", which Button relies on.
#[test]
fn a_truncating_fit_cuts_an_overflowing_label_to_one_fitting_line() {
    let mut c = CosmicMeasure::default();
    let long = "Screenshot 2026-05-28 at 01.21.25.png";
    let params = shape(16.0).width(120.0);
    let w = params.max_width_px.unwrap();

    // Precondition: the natural single line genuinely overflows `w`.
    let full = c.measure(long, params.unbounded());
    assert!(
        full.size.w > w,
        "precondition: natural line ({}) must overflow the cap ({w})",
        full.size.w,
    );

    let mut keys = Vec::new();
    for fit in [LineFit::Clip, LineFit::Ellipsis] {
        let cut = measure_truncated(&mut c, long, params, fit);
        assert!(
            cut.size.w <= w,
            "{fit:?} width {} must fit cap {w}",
            cut.size.w,
        );
        assert_eq!(
            cut.size.h,
            one_line_h(params),
            "{fit:?} must measure exactly one line",
        );
        assert_eq!(
            cut.intrinsic_min, None,
            "{fit:?} is a bounded resolve, which has no wrapping floor to \
             report — the floor belongs to the unbounded root",
        );
        assert_eq!(cut.key.fit(), fit);
        assert_eq!(
            cut.key.text_hash, full.key.text_hash,
            "{fit:?}: bounded keys reuse the source text hash",
        );

        // An ellipsis that cannot fit its own marker collapses to nothing;
        // a clip has no marker to reserve and does the same at zero width.
        let zero = measure_truncated(&mut c, long, params.width(0.0), fit);
        assert_eq!(
            zero.size.w, 0.0,
            "{fit:?} at zero width must collapse to zero",
        );
        keys.push(cut.key);
    }

    // Clip, ellipsis, and wrap bake three different strings at the same
    // width, so all three must key distinct cache slots.
    let wrapped = c.measure(long, params);
    assert_eq!(wrapped.key.fit(), LineFit::Wrap);
    assert_ne!(keys[0], keys[1], "clip and ellipsis must key distinctly");
    assert_ne!(keys[0], wrapped.key, "clip and wrap must key distinctly");
    assert_ne!(
        keys[1], wrapped.key,
        "ellipsis and wrap must key distinctly"
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
    // "ab<cd>e" with an RTL segment in the middle: visual starts run
    // 0,1,3,2,4, so the run is logical at both ends and inverted only
    // across the embedded pair. Every other case here is either sorted
    // outright or fully reversed, which a first-pair check would call
    // correctly by luck — this one is what forces the ordering scan to
    // look at the whole run.
    const BIDI: Run = &[
        (0, 1, 10.0),
        (1, 2, 10.0),
        (3, 4, 10.0),
        (2, 3, 10.0),
        (4, 5, 10.0),
    ];

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
            BIDI,
            25.0,
            ANY,
            2,
            "the embedded segment has not been paid for",
        ),
        (
            BIDI,
            30.0,
            ANY,
            3,
            "the logically-third glyph sits third in visual order's tail",
        ),
        (BIDI, 50.0, ANY, 5, "the whole bidi run fits"),
        (
            BIDI,
            1000.0,
            5,
            4,
            "the bound retires the logically-last glyph",
        ),
        (
            CLUSTER,
            1000.0,
            9,
            1,
            "backing off a cluster retires all of its glyphs",
        ),
    ] {
        let cut = cluster_glyph::fitting_prefix(
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
    //
    // One measurer across every combination, not one per: the cache is
    // keyed by everything that affects shaping, and
    // `truncation_from_cached_unbounded_is_order_independent` pins that
    // prior contents cannot change a result. Sharing it also exercises
    // that, and spares 15 system-font scans.
    let base = shape(16.0);
    let mut c = CosmicMeasure::default();
    for text in [
        "flag \u{1f1fa}\u{1f1f8} emoji \u{1f600} run",
        "\u{1f469}\u{200d}\u{1f469}\u{200d}\u{1f467} family emoji",
        "\u{627}\u{644}\u{633}\u{644}\u{627}\u{645} \u{639}\u{644}\u{64a}\u{643}\u{645}",
        "\u{645}\u{631}\u{62d}\u{628}\u{627} \u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645}",
    ] {
        for family in [FontFamily::SANS, FontFamily::MONO] {
            for fit in [LineFit::Clip, LineFit::Ellipsis] {
                for width_px in 0..=160 {
                    let width = width_px as f32;
                    let m = measure_truncated(&mut c, text, base.family(family).width(width), fit);
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
    // Hebrew, which no bundled face covers: without the machine's fonts
    // every glyph is the same tofu box and the prefix/suffix widths this
    // case separates would be identical.
    let mut c = CosmicMeasure::new(FontScope::System);
    let unbounded = shape(16.0);
    let elide = |c: &mut CosmicMeasure, text: &str, width: f32| {
        measure_truncated(c, text, unbounded.width(width), LineFit::Ellipsis)
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

/// A label that already fits its cap is shaped whole — no spurious
/// ellipsis, and the extent is the natural one *exactly*. Exact, not
/// approximate: the fitting path reshapes the identical string through
/// the same measurer, so a pixel of drift means truncation fired when it
/// should not have and dropped a glyph.
///
/// The halign row is the regression that motivated it: a `Center`-aligned
/// label in a 400 px cap once measured ~half the box wide, because the
/// shaped buffer baked in the width and the per-line align that the
/// encoder applies again.
#[test]
fn a_fitting_label_measures_its_natural_width_whatever_the_cap_or_align() {
    let mut c = CosmicMeasure::default();
    for (label, text, cap, halign) in [
        ("short label", "ok", 200.0, HAlign::Auto),
        ("centered in a wide cap", "File", 400.0, HAlign::Center),
        ("right-aligned in a wide cap", "File", 400.0, HAlign::Right),
    ] {
        let params = shape(16.0).width(cap).halign(halign);
        let natural = c.measure(text, params.unbounded());
        for fit in [LineFit::Clip, LineFit::Ellipsis] {
            let fitted = measure_truncated(&mut c, text, params, fit);
            assert_eq!(
                fitted.size, natural.size,
                "{label} ({fit:?}) must measure its natural extent",
            );
        }
    }
}

#[test]
fn mono_ellipsis_caps_width_and_leaves_the_floor_to_the_root() {
    // Mono fallback: an elided long word caps at the available width; the
    // wrap counterpart instead grows height and keeps the longest-word
    // floor, which only its unbounded root can report — a bounded resolve
    // hands back an extent, so there is no floor on it to be wrong about.
    let long = "abcdefghijklmnop"; // 16 ASCII bytes × 8 px = 128 px natural
    let params = shape(16.0).width(40.0);
    let w = params.max_width_px.unwrap();

    let elided = mono_extent(long, params, LineFit::Ellipsis);
    assert_eq!(elided.w, w, "elided mono caps at the width");
    assert_eq!(elided.h, 16.0, "elided mono is one line");
    // A run that already fits measures its own glyphs, which is what the
    // cosmic cut answers for a prefix it never had to shorten.
    assert_eq!(mono_extent("ab", params, LineFit::Ellipsis).w, 16.0);

    // 40 px holds five 8 px cells, so 16 characters wrap to four 16 px lines.
    let wrapped = mono_extent(long, params, LineFit::Wrap);
    assert_eq!(wrapped.h, 64.0, "wrap grows height across lines");
    assert_eq!(
        mono_root(long, params).wrap_floor(),
        128.0,
        "one unbreakable word floors at its whole natural width",
    );
}

/// Truncation reads its probe glyphs from the cached unbounded buffer.
/// Measure the same input on a fresh measurer and one containing unrelated
/// cached shapes; both the derived key and exact measurement must agree.
#[test]
fn truncation_from_cached_unbounded_is_order_independent() {
    let long = "the quick brown fox jumps over the lazy dog";
    let target = shape(14.0).width(80.0).halign(HAlign::Left);

    // Fresh measurer: only the target measurement.
    let mut fresh = CosmicMeasure::default();
    let r_fresh = truncate(&mut fresh, long, target, LineFit::Ellipsis);

    // Reused measurer: populate unrelated unbounded, truncated, and ellipsis
    // cache entries first, then measure the identical target.
    let mut reused = CosmicMeasure::default();
    measure_truncated(
        &mut reused,
        "a considerably longer string that grows the probe buffer capacity",
        shape(20.0)
            .width(220.0)
            .family(FontFamily::MONO)
            .halign(HAlign::Left),
        LineFit::Ellipsis,
    );
    measure_truncated(
        &mut reused,
        "short",
        shape(10.0).width(30.0).halign(HAlign::Left),
        LineFit::Clip,
    );
    let r_reused = measure_truncated(&mut reused, long, target, LineFit::Ellipsis);

    assert_eq!(
        r_fresh.fitted.size, r_reused.size,
        "unrelated cached buffers changed the measured size",
    );
    assert_eq!(
        r_fresh.fitted.key, r_reused.key,
        "same inputs must map to the same cache key regardless of prior shaping",
    );

    // Truncation actually fired: the ellipsized line is narrower than the
    // full unbounded shape (and fits within the width budget).
    assert!(
        r_fresh.fitted.size.w < r_fresh.unbounded.size.w,
        "expected truncation: ellipsized {} should be < unbounded {}",
        r_fresh.fitted.size.w,
        r_fresh.unbounded.size.w,
    );
    assert!(
        r_fresh.fitted.size.w <= 80.0,
        "ellipsized width {} should fit within budget 80",
        r_fresh.fitted.size.w,
    );
}

/// A continuous font-size zoom over ellipsized text mints a distinct
/// quantized size every frame, so the ellipsis reservation is recomputed
/// throughout. Drive a long sweep of sizes through one budget and assert
/// every one still lands inside it.
/// The "…" advance is memoized per face, and one slot was not enough.
///
/// Record order interleaves faces constantly — a header above its detail
/// row, bold beside regular in one line, a tree sized per depth — and a
/// single slot holding only the *last* face missed on every one of those
/// truncations, giving back the whole ~29% the memo buys.
///
/// Driven at a fresh width each round so every call is a truncation
/// *miss* and actually reaches the memo; a repeated width would hit the
/// shaped-buffer cache and never ask.
#[test]
fn the_ellipsis_memo_survives_interleaved_faces() {
    const TEXT: &str = "a label far too long for the column it sits in";
    let mut c = CosmicMeasure::default();
    // Two faces a real frame would interleave: body text and a heavier,
    // larger heading.
    let faces = [
        shape(14.0).leading(18.0),
        shape(20.0).leading(24.0).weight(FontWeight::BOLD),
    ];

    // Warm both, so what follows measures reuse rather than first touch.
    for face in faces {
        truncate(&mut c, TEXT, face.width(120.0), LineFit::Ellipsis);
    }
    let warm = c.cache_counts();
    assert_eq!(
        warm.ellipsis_misses, 2,
        "premise: first touch of each face reshapes the marker once",
    );

    // Now alternate, a fresh width each round — a drag over a two-style
    // list. Every round is a truncation miss, and every one must still
    // find its face.
    for round in 0..8 {
        for face in faces {
            truncate(
                &mut c,
                TEXT,
                face.width(119.0 - round as f32),
                LineFit::Ellipsis,
            );
        }
    }
    let churn = c.cache_counts() - warm;
    assert!(
        churn.shapes >= 16,
        "premise: each round reshaped, so the memo was actually consulted          ({} shapes)",
        churn.shapes,
    );
    assert_eq!(
        churn.ellipsis_misses, 0,
        "an interleaved second face must not evict the first",
    );

    // And the slots are finite: more distinct faces than they hold does
    // fall back to reshaping, which is what bounds them.
    let many: Vec<_> = (0..8)
        .map(|i| shape(10.0 + i as f32).leading(24.0))
        .collect();
    for face in &many {
        truncate(&mut c, TEXT, face.width(100.0), LineFit::Ellipsis);
    }
    let before = c.cache_counts();
    for face in &many {
        truncate(&mut c, TEXT, face.width(99.0), LineFit::Ellipsis);
    }
    assert!(
        (c.cache_counts() - before).ellipsis_misses > 0,
        "eight faces cannot all fit four slots — the memo must be bounded",
    );
}

#[test]
fn ellipsis_stays_within_budget_under_size_churn() {
    let mut c = CosmicMeasure::default();
    let long = "the quick brown fox jumps over the lazy dog";
    let width = 60.0;
    for i in 0..261 {
        // Distinct quantized size each iteration (0.1px steps × 64 ≥ 1).
        let fs = 8.0 + i as f32 * 0.1;
        let r = measure_truncated(
            &mut c,
            long,
            shape(fs).width(width).halign(HAlign::Left),
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
    let mut c = CosmicMeasure::default();
    let text = "first paragraph here\nsecond paragraph";
    let params = shape(16.0).width(90.0);

    for fit in [LineFit::Clip, LineFit::Ellipsis] {
        let r = measure_truncated(&mut c, text, params, fit);
        assert_eq!(
            r.size.h,
            one_line_h(params),
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
        wrapped.size.h > one_line_h(params) * 2.0,
        "premise: wrapped keeps every line, got h={}",
        wrapped.size.h,
    );
}
