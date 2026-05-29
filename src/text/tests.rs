use super::*;

/// Line-height equal to font size keeps the mono-fallback line
/// height numerically equal to `font_size`, matching the legacy
/// placeholder layout the existing tests pin.
fn lh(font_size: f32) -> f32 {
    font_size
}

#[test]
fn mono_measure_cases() {
    type Case = (&'static str, &'static str, f32, f32, Option<f32>, Size);
    let cases: &[Case] = &[
        ("empty", "", 16.0, lh(16.0), None, Size::ZERO),
        (
            "unbroken_legacy_short",
            "Hi",
            16.0,
            lh(16.0),
            None,
            Size::new(16.0, 16.0),
        ),
        (
            "unbroken_legacy_long",
            "hello!!",
            16.0,
            lh(16.0),
            None,
            Size::new(56.0, 16.0),
        ),
        (
            "wraps_below_unbroken",
            "12345678",
            16.0,
            lh(16.0),
            Some(32.0),
            Size::new(32.0, 32.0),
        ),
        (
            "line_height_param_short",
            "Hi",
            16.0,
            24.0,
            None,
            Size::new(16.0, 24.0),
        ),
        (
            "line_height_param_wrapped",
            "12345678",
            16.0,
            24.0,
            Some(32.0),
            Size::new(32.0, 48.0),
        ),
    ];
    for (label, text, fs, lh_v, max_w, expected) in cases {
        let r = mono_measure(text, *fs, *lh_v, *max_w, LineFit::Wrap);
        assert_eq!(r.size, *expected, "case: {label}");
    }
    // Empty also produces the INVALID sentinel.
    assert!(
        mono_measure("", 16.0, lh(16.0), None, LineFit::Wrap)
            .key
            .is_invalid()
    );
}

/// `cursor_xy(...).x`. Mono fallback: each ASCII byte is
/// `font_size * 0.5` wide. Caret x is independent of `line_height`
/// (advance only depends on font_size + glyph). Empty string and
/// zero offset short-circuit to zero.
#[test]
fn cursor_xy_x_cases() {
    let cases: &[(&str, &str, usize, f32, f32, f32)] = &[
        ("zero_offset", "hello", 0, 16.0, lh(16.0), 0.0),
        ("empty_string", "", 0, 16.0, lh(16.0), 0.0),
        ("mono_one_char", "abc", 1, 16.0, lh(16.0), 8.0),
        ("mono_two_chars", "abc", 2, 16.0, lh(16.0), 16.0),
        ("mono_three_chars", "abc", 3, 16.0, lh(16.0), 24.0),
        ("lh_independent_short", "abc", 2, 16.0, 16.0, 16.0),
        ("lh_independent_tall", "abc", 2, 16.0, 24.0, 16.0),
    ];
    for (label, text, offset, fs, lh_v, expected) in cases {
        let m = TextShaper::default();
        assert_eq!(
            m.cursor_xy(
                text,
                *offset,
                *fs,
                *lh_v,
                None,
                FontFamily::Sans,
                HAlign::Auto,
            )
            .x,
            *expected,
            "case: {label}"
        );
    }
}

#[test]
fn cosmic_text_cache_key_distinguishes_line_height() {
    // Pin: at the same font-size, different leadings produce
    // different TextCacheKeys. The renderer caches shaped buffers
    // by key — without this discrimination, a 16/19.2 buffer would
    // be returned for a request that wanted 16/24, mismatching the
    // measured rect against the rasterized glyphs.
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let a = c
        .measure(
            "hi",
            16.0,
            16.0 * LINE_HEIGHT_MULT,
            None,
            FontFamily::Sans,
            HAlign::Auto,
        )
        .key;
    let b = c
        .measure("hi", 16.0, 24.0, None, FontFamily::Sans, HAlign::Auto)
        .key;
    assert_ne!(a, b, "different leading must produce different key");
    assert_ne!(a.lh_q, b.lh_q, "lh_q is the discriminating field");
    // Same call repeated → identical key (cache hit, deterministic).
    let a2 = c
        .measure(
            "hi",
            16.0,
            16.0 * LINE_HEIGHT_MULT,
            None,
            FontFamily::Sans,
            HAlign::Auto,
        )
        .key;
    assert_eq!(a, a2);
}

#[test]
fn cosmic_text_family_distinguishes_key_and_metrics() {
    // Pin: Sans (Inter) and Mono (JetBrains Mono) at the same text
    // and size produce (a) different cache keys and (b) different
    // measured widths. Without this, a regression in `attrs_for`
    // could silently fall both families through to Inter and every
    // other test would still pass.
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let sans = c.measure("MMMM", 16.0, lh(16.0), None, FontFamily::Sans, HAlign::Auto);
    let mono = c.measure("MMMM", 16.0, lh(16.0), None, FontFamily::Mono, HAlign::Auto);
    assert_ne!(sans.key, mono.key, "family must enter the cache key");
    assert_ne!(
        sans.key.family_q, mono.key.family_q,
        "family_q is the discriminating field",
    );
    assert_ne!(
        sans.size.w, mono.size.w,
        "Inter and JBMono produce different glyph widths for 'MMMM'",
    );
}

#[test]
fn shape_unbounded_caches_per_authoring_hash_only() {
    // The reuse cache is keyed by `(WidgetId, NodeHash)` — different
    // line heights with the *same* hash would collide (same widget
    // id, same hash → cache hit returning the wrong measurement).
    // Authoring-side hash includes line_height_px (pinned in
    // node_hash tests), so callers that change leading must produce
    // a different hash — pin that the measure cache respects the
    // hash distinction.
    let m = TextShaper::default();
    let wid = WidgetId::from_hash("a");
    let h1 = NodeHash(1);
    let h2 = NodeHash(2);
    let r1 = m.shape_unbounded(wid, 0, h1, "hi", 16.0, 16.0, FontFamily::Sans);
    let r2 = m.shape_unbounded(wid, 0, h2, "hi", 16.0, 24.0, FontFamily::Sans);
    assert_ne!(
        r1.size.h, r2.size.h,
        "different leading via different hash → distinct cache entries",
    );
    // Re-querying with the original hash returns the original (16
    // px height), proving the entry wasn't overwritten.
    let r1_again = m.shape_unbounded(wid, 0, h1, "hi", 16.0, 16.0, FontFamily::Sans);
    assert_eq!(r1.size.h, r1_again.size.h);
}

#[test]
#[should_panic(expected = "shape_wrap requires a prior shape_unbounded")]
fn shape_wrap_panics_without_prime() {
    // Contract change: `shape_wrap` no longer falls back to a
    // dispatch-without-cache when the unbounded entry is missing.
    // Pin the panic so a future caller that wraps without priming
    // first fails loudly instead of silently losing the cache.
    let m = TextShaper::default();
    let wid = WidgetId::from_hash("a");
    m.shape_wrap(
        wid,
        0,
        "hi",
        16.0,
        16.0,
        100.0,
        100,
        FontFamily::Sans,
        HAlign::Auto,
        LineFit::Wrap,
    );
}

#[test]
fn text_cache_key_invalid_constant_zero_filled() {
    // `_pad` byte was added to satisfy bytemuck's no-padding rule;
    // pin that the INVALID sentinel still round-trips through
    // `is_invalid`. Failure here would mean a malformed default.
    assert!(TextCacheKey::INVALID.is_invalid());
    // And a non-INVALID key registers as such even with all
    // hashable fields zero except text_hash.
    let real = TextCacheKey::new(1, 0, 0, 0, 0, 0);
    assert!(!real.is_invalid());
}

#[test]
fn cursor_xy_cosmic_path_is_monotonic_and_bounded() {
    // With real shaping, caret-x at each byte boundary must be
    // non-decreasing and approach the full-string width at the
    // final offset. Exact pixel values depend on font metrics; we
    // only pin the monotonicity invariant consumers rely on.
    let m = TextShaper::with_bundled_fonts();
    let s = "hello";
    let widths: Vec<f32> = (0..=s.len())
        .map(|i| {
            m.cursor_xy(
                s,
                i,
                16.0,
                16.0 * LINE_HEIGHT_MULT,
                None,
                FontFamily::Sans,
                HAlign::Auto,
            )
            .x
        })
        .collect();
    assert_eq!(widths[0], 0.0, "caret-x at offset 0 is zero");
    for w in widths.windows(2) {
        assert!(
            w[1] >= w[0] - 0.01,
            "caret-x must be non-decreasing, got {w:?}",
        );
    }
    assert!(
        widths[s.len()] > widths[0],
        "non-empty string has positive width",
    );
}

#[test]
fn byte_at_xy_mono_fallback() {
    // Mono shaper: glyph_w = font_size * 0.5 = 8 px at 16 px font.
    // `byte_at_xy` ignores y on the mono path. Picks the boundary
    // whose prefix-x is closest to `target_x`.
    let m = TextShaper::default();
    let cases: &[(&str, f32, usize)] = &[
        ("origin", 0.0, 0),
        ("first_boundary", 8.0, 1),
        ("mid_glyph_rounds_to_nearer_boundary", 11.0, 1),
        ("mid_glyph_rounds_to_nearer_boundary_other", 13.0, 2),
        ("past_end_clamps", 100.0, 5),
    ];
    for (label, x, expected) in cases {
        let got = m.byte_at_xy(
            "hello",
            *x,
            0.0,
            16.0,
            16.0,
            None,
            FontFamily::Sans,
            HAlign::Auto,
        );
        assert_eq!(got, *expected, "case: {label}");
    }
}

#[test]
fn byte_at_xy_cosmic_path_monotonic_and_bounded() {
    // Real shaping: caret at the cursor_xy of byte i must hit-test
    // back to a byte close to i; widths sweep monotonically.
    let m = TextShaper::with_bundled_fonts();
    let s = "hello";
    let fs = 16.0;
    let lh_v = fs * LINE_HEIGHT_MULT;
    let probes: Vec<usize> = (0..=s.len())
        .map(|i| {
            let x = m
                .cursor_xy(s, i, fs, lh_v, None, FontFamily::Sans, HAlign::Auto)
                .x;
            m.byte_at_xy(s, x, 0.0, fs, lh_v, None, FontFamily::Sans, HAlign::Auto)
        })
        .collect();
    // Monotone non-decreasing — hit-test never goes backwards as x grows.
    for w in probes.windows(2) {
        assert!(w[1] >= w[0], "byte_at_xy not monotone: {probes:?}");
    }
    // Past-end x clamps to text.len().
    let past = m.byte_at_xy(
        s,
        10_000.0,
        0.0,
        fs,
        lh_v,
        None,
        FontFamily::Sans,
        HAlign::Auto,
    );
    assert_eq!(past, s.len(), "x past end must clamp to text.len()");
}

#[test]
fn selection_rects_empty_range_is_noop() {
    let m = TextShaper::with_bundled_fonts();
    let mut out: SelectionRects = SelectionRects::new();
    out.push(Rect::new(1.0, 2.0, 3.0, 4.0)); // pre-populate
    m.selection_rects(
        "hello",
        5..5,
        16.0,
        16.0 * LINE_HEIGHT_MULT,
        None,
        FontFamily::Sans,
        HAlign::Auto,
        &mut out,
    );
    assert!(
        out.is_empty(),
        "empty range must clear out and emit nothing"
    );
}

#[test]
fn selection_rects_single_line_emits_one_rect() {
    let m = TextShaper::with_bundled_fonts();
    let mut out: SelectionRects = SelectionRects::new();
    m.selection_rects(
        "hello",
        1..4,
        16.0,
        16.0 * LINE_HEIGHT_MULT,
        None,
        FontFamily::Sans,
        HAlign::Auto,
        &mut out,
    );
    assert_eq!(out.len(), 1, "single-line range → one rect");
    let r = out[0];
    assert!(r.size.w > 0.0, "rect has positive width");
    assert!(r.size.h > 0.0, "rect has positive height");
    // Origin pinned at y = 0 — first (and only) visual line.
    assert!(
        r.min.y.abs() < 0.5,
        "single line starts at y≈0, got {}",
        r.min.y
    );
}

#[test]
fn selection_rects_multiline_emits_one_rect_per_line() {
    // Three lines, range crossing every line break. Cosmic emits one
    // highlight rect per visual line; we pin >=3 rects (cosmic may
    // emit additional segments if it splits per-run, but never < the
    // line count).
    let m = TextShaper::with_bundled_fonts();
    let mut out: SelectionRects = SelectionRects::new();
    let text = "abc\ndef\nghi";
    m.selection_rects(
        text,
        0..text.len(),
        16.0,
        16.0 * LINE_HEIGHT_MULT,
        None,
        FontFamily::Sans,
        HAlign::Auto,
        &mut out,
    );
    assert!(
        out.len() >= 3,
        "≥3 rects for 3-line selection, got {}",
        out.len()
    );
    // y_top strictly increases between successive lines.
    let mut last_y = f32::MIN;
    let mut distinct_ys = 0;
    for r in out.iter() {
        if r.min.y > last_y + 0.5 {
            distinct_ys += 1;
            last_y = r.min.y;
        }
    }
    assert!(
        distinct_ys >= 3,
        "rects must span ≥3 distinct y rows, got {distinct_ys}"
    );
}

#[test]
fn cursor_byte_round_trip_multiline() {
    // `cursor_from_byte` and `cursor_to_byte` must invert each other
    // across line breaks. Offsets sampled at every byte position of a
    // 3-line string with varying line lengths.
    let text = "ab\ncde\nfg";
    for off in 0..=text.len() {
        let cur = cursor_from_byte(text, off);
        let back = cursor_to_byte(text, cur);
        assert_eq!(
            back, off,
            "round-trip failed at offset {off}, cursor={cur:?}"
        );
    }
    // Line counts: offsets 0..=2 → line 0; 3..=6 → line 1; 7..=9 → line 2.
    assert_eq!(cursor_from_byte(text, 0).line, 0);
    assert_eq!(cursor_from_byte(text, 2).line, 0);
    assert_eq!(cursor_from_byte(text, 3).line, 1);
    assert_eq!(cursor_from_byte(text, 6).line, 1);
    assert_eq!(cursor_from_byte(text, 7).line, 2);
    assert_eq!(cursor_from_byte(text, 9).line, 2);
}

#[test]
fn cursor_xy_multiline_y_top_advances_per_line() {
    // Two-line buffer: caret on line 1 must have y_top > caret on line 0,
    // and the delta must be ≈ line_height. Pins multi-line caret routing
    // through cosmic's layout_runs.
    let m = TextShaper::with_bundled_fonts();
    let fs = 16.0;
    let lh_v = fs * LINE_HEIGHT_MULT;
    let p0 = m.cursor_xy(
        "abc\ndef",
        0,
        fs,
        lh_v,
        None,
        FontFamily::Sans,
        HAlign::Auto,
    );
    let p1 = m.cursor_xy(
        "abc\ndef",
        4,
        fs,
        lh_v,
        None,
        FontFamily::Sans,
        HAlign::Auto,
    );
    assert!(p0.y_top.abs() < 0.5, "line 0 y_top ≈ 0, got {}", p0.y_top);
    assert!(
        (p1.y_top - lh_v).abs() < 2.0,
        "line 1 y_top ≈ line_height ({lh_v}), got {}",
        p1.y_top,
    );
}

#[test]
fn cosmic_empty_text_returns_invalid_zero_size() {
    // Empty-text early-return on the cosmic path: ZERO size, INVALID
    // key, zero intrinsic_min. Pins the renderer's "drop INVALID
    // runs" contract for empty strings.
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let r = c.measure(
        "",
        16.0,
        16.0 * LINE_HEIGHT_MULT,
        None,
        FontFamily::Sans,
        HAlign::Auto,
    );
    assert_eq!(r.size, Size::ZERO);
    assert!(r.key.is_invalid());
    assert_eq!(r.intrinsic_min, 0.0);
    // `buffer_for(INVALID)` must return None — even after measuring,
    // no buffer was cached for the empty input.
    assert!(c.buffer_for(r.key).is_none());
}

#[test]
fn cosmic_nonpositive_font_size_returns_invalid() {
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    for fs in [0.0_f32, -1.0, -16.0] {
        let r = c.measure("hi", fs, 16.0, None, FontFamily::Sans, HAlign::Auto);
        assert_eq!(r.size, Size::ZERO, "fs={fs}");
        assert!(r.key.is_invalid(), "fs={fs}");
    }
}

#[test]
fn cosmic_intrinsic_min_tracks_longest_word() {
    // `intrinsic_min` = width of the widest unbreakable run. For a
    // multi-word string, it must (a) be strictly positive, (b) be
    // strictly less than the unbroken total (multiple words present),
    // and (c) match the standalone measurement of the longest word
    // within shaping tolerance — that's the wrap floor downstream
    // layout pins as the "can't break below this" guarantee.
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let full = c.measure(
        "hello world hi",
        16.0,
        16.0 * LINE_HEIGHT_MULT,
        None,
        FontFamily::Sans,
        HAlign::Auto,
    );
    let hello = c.measure(
        "hello",
        16.0,
        16.0 * LINE_HEIGHT_MULT,
        None,
        FontFamily::Sans,
        HAlign::Auto,
    );
    let world = c.measure(
        "world",
        16.0,
        16.0 * LINE_HEIGHT_MULT,
        None,
        FontFamily::Sans,
        HAlign::Auto,
    );
    let hi = c.measure(
        "hi",
        16.0,
        16.0 * LINE_HEIGHT_MULT,
        None,
        FontFamily::Sans,
        HAlign::Auto,
    );
    let longest_w = hello.size.w.max(world.size.w).max(hi.size.w);
    assert!(
        full.intrinsic_min > hi.size.w,
        "must exceed the shortest word"
    );
    assert!(
        full.intrinsic_min < full.size.w,
        "multi-word intrinsic_min ({}) must be < total width ({})",
        full.intrinsic_min,
        full.size.w,
    );
    // Within shaping tolerance — kerning around space glyphs can
    // shift the in-run word width a couple of px vs the standalone
    // measurement, so allow ±10%.
    let rel_err = (full.intrinsic_min - longest_w).abs() / longest_w;
    assert!(
        rel_err < 0.15,
        "intrinsic_min ({}) must ≈ longest-word width ({}), rel_err = {}",
        full.intrinsic_min,
        longest_w,
        rel_err,
    );
    // Single-word input: intrinsic_min ≈ size.w. size.w is the
    // last glyph's (x + w) ceil'd; intrinsic_min sums glyph widths.
    // The two differ by sub-pixel kerning / ceil rounding — allow 2 px.
    assert!(
        (hello.intrinsic_min - hello.size.w).abs() < 2.0,
        "single-word: intrinsic_min ({}) ≈ size.w ({})",
        hello.intrinsic_min,
        hello.size.w,
    );
}

#[test]
fn cache_key_collapses_halign_when_unbounded() {
    // Optimization in `cosmic::key_for`: halign only affects shaped
    // glyph positions when there's a wrap target, so without one the
    // key folds non-Auto halign down to Auto. Two unbounded measures
    // with different halign must therefore produce identical keys —
    // single-line callers don't pay an N-way cache split.
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let auto = c
        .measure("hi", 16.0, 16.0, None, FontFamily::Sans, HAlign::Auto)
        .key;
    let right = c
        .measure("hi", 16.0, 16.0, None, FontFamily::Sans, HAlign::Right)
        .key;
    let center = c
        .measure("hi", 16.0, 16.0, None, FontFamily::Sans, HAlign::Center)
        .key;
    assert_eq!(auto, right, "unbounded: halign collapses to Auto in key");
    assert_eq!(auto, center, "unbounded: halign collapses to Auto in key");
    // With a wrap target the keys must diverge — per-line align now
    // affects glyph positions in the shaped buffer.
    let auto_w = c
        .measure(
            "hi",
            16.0,
            16.0,
            Some(200.0),
            FontFamily::Sans,
            HAlign::Auto,
        )
        .key;
    let right_w = c
        .measure(
            "hi",
            16.0,
            16.0,
            Some(200.0),
            FontFamily::Sans,
            HAlign::Right,
        )
        .key;
    assert_ne!(auto_w, right_w, "wrap-bounded: halign must enter the key");
}

#[test]
fn shape_wrap_busts_on_halign_change_same_target() {
    // Wrap reuse cache is keyed by `(target_q, halign)`. Same target,
    // different halign → different cached buffer (cosmic's per-line
    // align changes glyph positions). Pin that the reuse cache
    // discriminates on halign, not just target_q.
    let m = TextShaper::with_bundled_fonts();
    let wid = WidgetId::from_hash("w");
    let hash = NodeHash(7);
    m.shape_unbounded(wid, 0, hash, "hi", 16.0, 16.0, FontFamily::Sans);
    let baseline = m.measure_calls();
    // First wrap shape — dispatches once.
    m.shape_wrap(
        wid,
        0,
        "hi",
        16.0,
        16.0,
        200.0,
        200 * 64,
        FontFamily::Sans,
        HAlign::Left,
        LineFit::Wrap,
    );
    let after_left = m.measure_calls();
    assert_eq!(after_left, baseline + 1, "first wrap shape must dispatch");
    // Repeat same call — cache hit, no dispatch.
    m.shape_wrap(
        wid,
        0,
        "hi",
        16.0,
        16.0,
        200.0,
        200 * 64,
        FontFamily::Sans,
        HAlign::Left,
        LineFit::Wrap,
    );
    assert_eq!(
        m.measure_calls(),
        after_left,
        "identical wrap call must hit cache"
    );
    // Same target, different halign — must dispatch again.
    m.shape_wrap(
        wid,
        0,
        "hi",
        16.0,
        16.0,
        200.0,
        200 * 64,
        FontFamily::Sans,
        HAlign::Right,
        LineFit::Wrap,
    );
    assert_eq!(
        m.measure_calls(),
        after_left + 1,
        "halign change at same target must bust wrap reuse",
    );
}

#[test]
fn sweep_removed_evicts_reuse_entries() {
    // `Ui::post_record` calls this with the per-frame removed set so
    // dropped widgets don't leak text-shape cache rows. Pin: entries
    // for removed ids vanish; entries for surviving ids stay.
    let m = TextShaper::default();
    let a = WidgetId::from_hash("a");
    let b = WidgetId::from_hash("b");
    m.shape_unbounded(a, 0, NodeHash(1), "hi", 16.0, 16.0, FontFamily::Sans);
    m.shape_unbounded(b, 0, NodeHash(2), "yo", 16.0, 16.0, FontFamily::Sans);
    assert!(m.has_reuse_entry(a, 0));
    assert!(m.has_reuse_entry(b, 0));
    let removed: FxHashSet<WidgetId> = FxHashSet::from_iter([a]);
    m.sweep_removed(&removed);
    assert!(!m.has_reuse_entry(a, 0), "removed widget's entry evicted");
    assert!(m.has_reuse_entry(b, 0), "surviving widget's entry kept");
    // Empty removed set is a no-op (early return path).
    m.sweep_removed(&FxHashSet::default());
    assert!(m.has_reuse_entry(b, 0));
}

/// Right-aligned multi-line buffer: caret at byte 4 ("abc\n|") lands
/// on the empty second line. Cosmic's per-line halign offset only
/// shifts existing glyphs, so an empty line has `line_w = 0` and
/// the naive `unwrap_or(run.line_w)` reports `x = 0` (left edge).
/// Post-fix the empty-line branch routes through `empty_line_x`,
/// putting the caret at the right edge of the wrap target.
#[test]
fn cursor_xy_on_empty_line_respects_right_align() {
    let m = TextShaper::with_bundled_fonts();
    let text = "abc\n";
    let wrap = 200.0;
    let font = 16.0;
    let line_h = font * LINE_HEIGHT_MULT;
    // `cursor_xy` calls `with_buffer` which in turn drives
    // `measure` end-to-end (unbounded + wrap-shape), so no
    // pre-prime is needed — the shaper builds whatever cache
    // entry it needs on first hit.
    let pos = m.cursor_xy(
        text,
        text.len(),
        font,
        line_h,
        Some(wrap),
        FontFamily::Sans,
        HAlign::Right,
    );
    assert!(
        (pos.x - wrap).abs() < 0.5,
        "right-aligned caret on empty trailing line must sit at \
         the wrap target ({wrap}); got x = {}",
        pos.x,
    );
    // And the left-aligned counterpart still anchors at zero —
    // sanity-pins the helper isn't accidentally always returning
    // the right edge.
    let pos_left = m.cursor_xy(
        text,
        text.len(),
        font,
        line_h,
        Some(wrap),
        FontFamily::Sans,
        HAlign::Left,
    );
    assert!(
        pos_left.x.abs() < 0.5,
        "left-aligned caret on empty trailing line stays at 0; \
         got x = {}",
        pos_left.x,
    );
}

#[test]
fn cosmic_ellipsis_elides_long_line_to_width() {
    // A label wider than the committed width truncates to one line that
    // fits, with a trailing ellipsis. Pins the "labels never overflow
    // their box" contract the Button relies on.
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "Screenshot 2026-05-28 at 01.21.25.png";
    let w = 120.0;
    let elided = c.measure_truncated(
        long,
        16.0,
        lh(16.0),
        w,
        FontFamily::Sans,
        HAlign::Auto,
        true,
    );
    // Precondition: the natural single line genuinely overflows `w`.
    let full = c.measure(long, 16.0, lh(16.0), None, FontFamily::Sans, HAlign::Auto);
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
        elided.intrinsic_min, 0.0,
        "an elided run has zero min floor"
    );
    // The elided buffer must not collide with the *wrapped* buffer at the
    // same width — they hold different strings, so distinct cache keys.
    let wrapped = c.measure(
        long,
        16.0,
        lh(16.0),
        Some(w),
        FontFamily::Sans,
        HAlign::Auto,
    );
    assert_ne!(
        elided.key, wrapped.key,
        "elision and wrap must key distinct cache slots at the same width",
    );
}

#[test]
fn cosmic_ellipsis_short_text_not_truncated() {
    // A label that already fits the cap is shaped whole — no spurious
    // ellipsis, width matches the natural measurement.
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let short = "ok";
    let natural = c.measure(short, 16.0, lh(16.0), None, FontFamily::Sans, HAlign::Auto);
    let elided = c.measure_truncated(
        short,
        16.0,
        lh(16.0),
        200.0,
        FontFamily::Sans,
        HAlign::Auto,
        true,
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
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let label = "File";
    let cap = 400.0;
    let natural = c.measure(label, 16.0, lh(16.0), None, FontFamily::Sans, HAlign::Auto);
    for fit in [false, true] {
        let m = c.measure_truncated(
            label,
            16.0,
            lh(16.0),
            cap,
            FontFamily::Sans,
            HAlign::Center,
            fit,
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
    use crate::text::cosmic::CosmicMeasure;
    let mut c = CosmicMeasure::with_bundled_fonts();
    let long = "Screenshot 2026-05-28 at 01.21.25.png";
    let w = 120.0;
    let full = c.measure(long, 16.0, lh(16.0), None, FontFamily::Sans, HAlign::Auto);
    assert!(
        full.size.w > w,
        "precondition: natural line ({}) must overflow the cap ({w})",
        full.size.w,
    );
    let clipped = c.measure_truncated(
        long,
        16.0,
        lh(16.0),
        w,
        FontFamily::Sans,
        HAlign::Auto,
        false,
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
        clipped.intrinsic_min, 0.0,
        "a clipped run has zero min floor"
    );
    // Clip and ellipsis cut to the same cap but bake different strings (the
    // ellipsis path appends `…` and reserves its width), so they must key
    // distinct cache slots.
    let elided = c.measure_truncated(
        long,
        16.0,
        lh(16.0),
        w,
        FontFamily::Sans,
        HAlign::Auto,
        true,
    );
    // Clip, ellipsis, and wrap each bake a distinct buffer at the same width.
    let wrapped = c.measure(
        long,
        16.0,
        lh(16.0),
        Some(w),
        FontFamily::Sans,
        HAlign::Auto,
    );
    assert_ne!(
        clipped.key, elided.key,
        "clip and ellipsis must key distinctly"
    );
    assert_ne!(
        clipped.key, wrapped.key,
        "clip and wrap must key distinctly"
    );
}

#[test]
fn mono_ellipsis_caps_width_with_zero_floor() {
    // Mono fallback: an elided long word caps at the available width and
    // reports zero min-content (shrinks to the ellipsis); the wrap
    // counterpart instead grows height and keeps the longest-word floor.
    let long = "abcdefghijklmnop"; // 16 ASCII bytes × 8 px = 128 px natural
    let w = 40.0;
    let elided = mono_measure(long, 16.0, lh(16.0), Some(w), LineFit::Ellipsis);
    assert_eq!(elided.size.w, w, "elided mono caps at the width");
    assert_eq!(elided.size.h, lh(16.0), "elided mono is one line");
    assert_eq!(elided.intrinsic_min, 0.0, "elided mono has zero floor");
    let wrapped = mono_measure(long, 16.0, lh(16.0), Some(w), LineFit::Wrap);
    assert!(wrapped.size.h > lh(16.0), "wrap grows height across lines");
    assert!(
        wrapped.intrinsic_min > 0.0,
        "wrap keeps a longest-word floor"
    );
}

/// `end_frame_evict` must (1) never drop a pinned key regardless of how
/// old it is, and (2) among the unpinned remainder keep exactly the
/// `keep_unpinned` most-recently-used by `last_used`. We shape ten
/// distinct widths one-per-frame so each entry gets a strictly
/// increasing `last_used`, then evict with the *oldest* key pinned and a
/// budget of 2 — proving recency loses to pinning and that the cap is
/// honoured.
#[test]
fn end_frame_evict_pins_survive_and_unpinned_lru_capped() {
    use crate::text::cosmic::CosmicMeasure;
    use rustc_hash::FxHashSet;

    let mut c = CosmicMeasure::with_bundled_fonts();
    let empty = FxHashSet::default();
    let mut keys = Vec::new();
    for i in 0..10u32 {
        // Distinct width per frame ⇒ distinct cache key ⇒ a fresh insert
        // stamped with that frame's generation.
        let r = c.measure(
            "hello world",
            14.0,
            lh(18.0),
            Some(40.0 + i as f32 * 5.0),
            FontFamily::Sans,
            HAlign::Left,
        );
        keys.push(r.key);
        // Advance the generation without evicting (budget far exceeds the
        // live count) so the next insert lands in a later frame.
        c.end_frame_evict(&empty, 1000);
    }
    assert_eq!(c.cache_len(), 10, "ten distinct widths, ten buffers");

    // Pin the OLDEST key; keep only 2 unpinned by recency.
    let pins: FxHashSet<TextCacheKey> = [keys[0]].into_iter().collect();
    c.end_frame_evict(&pins, 2);

    assert_eq!(c.cache_len(), 3, "1 pinned + 2 most-recent unpinned");
    assert!(
        c.buffer_for(keys[0]).is_some(),
        "pinned key survives despite being least-recently-used",
    );
    assert!(c.buffer_for(keys[9]).is_some(), "newest unpinned kept");
    assert!(
        c.buffer_for(keys[8]).is_some(),
        "second-newest unpinned kept"
    );
    for evicted in [1usize, 2, 5, 7] {
        assert!(
            c.buffer_for(keys[evicted]).is_none(),
            "older unpinned key {evicted} evicted",
        );
    }
}

/// Below budget the cache is left completely untouched — the no-regression
/// guarantee for bounded multi-size rotation (`frame/resizing_cpu`), whose
/// working set never crosses the budget and so must never reshape.
#[test]
fn end_frame_evict_is_noop_under_budget() {
    use crate::text::cosmic::CosmicMeasure;
    use rustc_hash::FxHashSet;

    let mut c = CosmicMeasure::with_bundled_fonts();
    let empty = FxHashSet::default();
    let mut keys = Vec::new();
    for i in 0..4u32 {
        let r = c.measure(
            "rotation",
            14.0,
            lh(18.0),
            Some(100.0 + i as f32 * 20.0),
            FontFamily::Sans,
            HAlign::Left,
        );
        keys.push(r.key);
        c.end_frame_evict(&empty, 1000);
    }
    // Four widths, nothing pinned, generous budget ⇒ no eviction even
    // though the most-recent (pinned=∅) entries are "newer" than the rest.
    c.end_frame_evict(&empty, 64);
    assert_eq!(c.cache_len(), 4, "under-budget eviction is a no-op");
    for k in &keys {
        assert!(c.buffer_for(*k).is_some(), "every rotation width retained");
    }
}
