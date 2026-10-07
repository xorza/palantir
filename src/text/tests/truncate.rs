use super::*;

#[test]
fn fitting_truncate_returns_the_unbounded_root_without_reshaping() {
    let mut text = TextSystem::cosmic();
    let wid = WidgetId::from_hash("fitting truncate");
    let fitting = shape(16.0).width(200.0).halign(HAlign::Center);

    // `capped_w` is the run's Inter width once cut to the 20 px bound. Ellipsis is narrower than Truncate because its "…" must fit inside the same bound.
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

        let truncated = text.shape_run(run_slot, "wider than twenty", fitting.width(20.0), wrap);
        assert_ne!(
            truncated.buffer_key(),
            truncated.buffer_key().unbounded_version()
        );
        assert_eq!(truncated.buffer_key().fit(), fit);
        assert_eq!(truncated.size.w, capped_w, "{wrap:?} caps inside 20 px");
    }

    // A multi-line source collapses to its first line under Clip/Ellipsis, so the unbounded root can't stand in even when its widest line fits.
    let multiline = text.shape_run(slot_at(wid, 2), "a\nb", fitting, TextWrap::Ellipsis);
    let bounded_key = fitting.request("a\nb", LineFit::Ellipsis).key();
    assert_eq!(
        multiline.buffer_key(),
        bounded_key,
        "multi-line text must resolve through the truncating path",
    );
    // One line at the 16 px leading `fitting` carries; the collapse makes a two-line source measure one line tall.
    assert_eq!(multiline.size.h, one_line_h(fitting));
}

/// Both truncating fits cut an over-wide label to one line within the committed width, differing only in the marker (`Ellipsis` reserves `…`, `Clip` cuts flush). Pins "labels never overflow their box", which Button relies on.
#[test]
fn a_truncating_fit_cuts_an_overflowing_label_to_one_fitting_line() {
    let mut c = CosmicMeasure::default();
    let long = "Screenshot 2026-05-28 at 01.21.25.png";
    let params = shape(16.0).width(120.0);
    let w = params.max_width.unwrap();

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
        assert_eq!(cut.buffer_key().fit(), fit);
        assert_eq!(
            cut.buffer_key().text_hash,
            full.buffer_key().text_hash,
            "{fit:?}: bounded keys reuse the source text hash",
        );

        let zero = measure_truncated(&mut c, long, params.width(0.0), fit);
        assert_eq!(
            zero.size.w, 0.0,
            "{fit:?} at zero width must collapse to zero",
        );
        keys.push(cut.buffer_key());
    }

    // Clip, ellipsis and wrap bake three different strings at one width, so each must key a distinct cache slot.
    let wrapped = c.measure(long, params);
    assert_eq!(wrapped.buffer_key().fit(), LineFit::Wrap);
    assert_ne!(keys[0], keys[1], "clip and ellipsis must key distinctly");
    assert_ne!(
        keys[0],
        wrapped.buffer_key(),
        "clip and wrap must key distinctly"
    );
    assert_ne!(
        keys[1],
        wrapped.buffer_key(),
        "ellipsis and wrap must key distinctly"
    );
}

#[test]
fn fitting_prefix_cuts_on_logical_cluster_boundaries() {
    // Hand-built glyph runs, so the cut is checked against arithmetic, not installed fonts; entries are (start, end, advance) in visual order.
    type Run = &'static [(usize, usize, f32)];
    const LTR: Run = &[(0, 1, 10.0), (1, 2, 10.0), (2, 3, 10.0)];
    const RTL: Run = &[(2, 3, 10.0), (1, 2, 10.0), (0, 1, 10.0)];
    const CLUSTER: Run = &[(0, 1, 10.0), (1, 9, 10.0), (1, 9, 10.0)];
    const MARK: Run = &[(0, 3, 10.0), (0, 3, 0.0), (3, 4, 10.0)];
    // "ab<cd>e" with a middle RTL segment: visual starts 0,1,3,2,4, inverted only across the pair; other cases pass a first-pair check by luck.
    const BIDI: Run = &[
        (0, 1, 10.0),
        (1, 2, 10.0),
        (3, 4, 10.0),
        (2, 3, 10.0),
        (4, 5, 10.0),
    ];

    const ANY: usize = usize::MAX;
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
        // `max_end` drives the back-off: feeding back the previous answer must retire at least one more cluster, down to nothing.
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
        let mut glyphs: Vec<ClusterGlyph> = run
            .iter()
            .map(|&(start, end, advance)| ClusterGlyph {
                start,
                end,
                advance,
            })
            .collect();
        let cut = ClusterGlyph::fitting_prefix(&mut glyphs, avail, max_end);
        assert_eq!(cut, expected, "avail={avail} max_end={max_end}: {why}");
        // Every bounded cut falls strictly below its bound, so feeding the answer back always progresses and the back-off terminates. Zero is the floor: production stops on an empty cut.
        assert!(
            max_end == ANY || max_end == 0 || cut < max_end,
            "a bounded cut must fall strictly below its bound",
        );
    }
}

#[test]
fn ellipsis_never_measures_wider_than_its_budget() {
    // A cut can overrun its budget by paying for only some of a cluster's glyphs while committing all its bytes (flag, ZWJ emoji), or by reshaping a prefix whose last letter changes form at a word end (Arabic). Both use fonts this crate doesn't bundle, so only the bound holds on every machine.
    //
    // One measurer across all combinations: the cache is keyed by everything affecting shaping, and `truncation_from_cached_unbounded_is_order_independent` pins that prior contents can't change a result. Sharing also spares 15 system-font scans.
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
    // The cut walks glyphs in visual order; in an RTL run a cut driven by `x + w` stops at the first glyph and drops the run. Hebrew, because no bundled face covers it (every glyph would be the same tofu box); the test face is loaded, not scanned.
    let mut c = CosmicMeasure::new(FontScope::Bundled);
    c.load_font(HEBREW.into())
        .expect("the Hebrew test face loads");
    let unbounded = shape(16.0);
    let elide = |c: &mut CosmicMeasure, text: &str, width: f32| {
        measure_truncated(c, text, unbounded.width(width), LineFit::Ellipsis)
            .size
            .w
    };

    // Three shin (widest) then three vav (narrowest), neither positional, so a cut prefix reshapes to its measured advances. Budgets are measured off the face.
    let rtl = "\u{5e9}\u{5e9}\u{5e9}\u{5d5}\u{5d5}\u{5d5}";
    let width_of = |c: &mut CosmicMeasure, text: &str| c.measure(text, unbounded).size.w;
    let marker_only = width_of(&mut c, "\u{2026}");
    let one_prefix = width_of(&mut c, "\u{5e9}\u{2026}");
    let two_prefix = width_of(&mut c, "\u{5e9}\u{5e9}\u{2026}");
    let two_suffix = width_of(&mut c, "\u{5d5}\u{5d5}\u{2026}");
    let whole = width_of(&mut c, rtl);

    assert!(
        two_prefix < whole,
        "two letters and the marker must be a cut, not the whole run: \
         {two_prefix} against {whole}",
    );
    assert!(
        (two_prefix - two_suffix).abs() >= 1.0,
        "prefix and suffix widths must differ for this to prove anything: \
         {two_prefix} vs {two_suffix}",
    );

    let one = elide(&mut c, rtl, one_prefix);
    assert_eq!(
        one, one_prefix,
        "an RTL run with room to spare must keep text, and a bare marker \
         measures {marker_only}",
    );

    let two = elide(&mut c, rtl, two_prefix);
    assert_eq!(
        two, two_prefix,
        "RTL elision must keep the leading letters, and {two_suffix} is what \
         the trailing ones measure",
    );

    let narrow = elide(&mut c, "abcd", 20.0);
    let wide = elide(&mut c, "abcd", 28.0);
    assert!(
        wide > narrow,
        "a wider box must keep more of an LTR run: {wide} vs {narrow}",
    );
}

/// A label that already fits its cap is shaped whole, with exactly the natural extent. The halign row is the regression: a `Center` label in a 400 px cap once measured ~half the box, because the buffer baked in the width and the encoder aligned again.
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
    // Mono fallback: an elided long word caps at the available width; the wrap counterpart grows height and keeps the longest-word floor, which only its unbounded root can report.
    let long = "abcdefghijklmnop"; // 16 ASCII bytes × 8 px = 128 px natural
    let params = shape(16.0).width(40.0);
    let w = params.max_width.unwrap();

    let elided = mono_extent(long, params, LineFit::Ellipsis);
    assert_eq!(elided.w, w, "elided mono caps at the width");
    assert_eq!(elided.h, 16.0, "elided mono is one line");
    assert_eq!(mono_extent("ab", params, LineFit::Ellipsis).w, 16.0);

    let wrapped = mono_extent(long, params, LineFit::Wrap);
    assert_eq!(wrapped.h, 64.0, "wrap grows height across lines");
    assert_eq!(
        mono_root(long, params).wrap_floor(),
        128.0,
        "one unbreakable word floors at its whole natural width",
    );
}

/// Truncation reads probe glyphs from the cached unbounded buffer: a fresh measurer and one holding unrelated shapes must agree on both derived key and exact measurement.
#[test]
fn truncation_from_cached_unbounded_is_order_independent() {
    let long = "the quick brown fox jumps over the lazy dog";
    let target = shape(14.0).width(80.0).halign(HAlign::Left);

    let mut fresh = CosmicMeasure::default();
    let r_fresh = truncate(&mut fresh, long, target, LineFit::Ellipsis);

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

/// A continuous font-size zoom over ellipsized text mints a new quantized size every frame; every size must still land inside one budget.
///
/// The "…" advance is memoized per face, and one slot was not enough: interleaved faces (header above detail, bold beside regular) missed on every truncation. Driven at a fresh width each round so each call reaches the memo.
#[test]
fn the_ellipsis_memo_survives_interleaved_faces() {
    const TEXT: &str = "a label far too long for the column it sits in";
    let mut c = CosmicMeasure::default();
    let faces = [
        shape(14.0).leading(18.0),
        shape(20.0).leading(24.0).weight(FontWeight::BOLD),
    ];

    for face in faces {
        truncate(&mut c, TEXT, face.width(120.0), LineFit::Ellipsis);
    }
    let warm = c.cache_counts();
    assert_eq!(
        warm.ellipsis_misses, 2,
        "premise: first touch of each face reshapes the marker once",
    );

    // Alternate with a fresh width each round (a drag over a two-style list): every round is a truncation miss and must still find its face.
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

/// A truncating fit paints exactly one visual line even with a hard newline: the cut comes from the first layout run of the unbounded probe.
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
        let newline = text.find('\n').unwrap();
        for g in glyph_positions(&c, r.buffer_key()) {
            assert!(
                g.start < newline,
                "{fit:?} kept a glyph from the second paragraph (byte {})",
                g.start,
            );
            assert_eq!(g.line_top, 0.0, "{fit:?} kept a glyph off line 0");
        }
    }

    let wrapped = c.measure(text, params);
    assert!(
        wrapped.size.h > one_line_h(params) * 2.0,
        "premise: wrapped keeps every line, got h={}",
        wrapped.size.h,
    );
}
