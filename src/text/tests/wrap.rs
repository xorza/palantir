use super::*;

#[test]
fn mono_measure_cases() {
    // Mono lays every ASCII byte out `font_size * 0.5` wide on a
    // `line_height` band, so each expected size below is arithmetic.
    //
    // `single_line` is asserted alongside, and the height column is what
    // makes it checkable rather than restated: a case measuring one band
    // tall must report `true`, two bands `false`. That flag is what gates
    // `TextSystem::measure`'s fitting-truncate skip, so a shape that lost
    // it would silently start reshaping every fitting label.
    let base = shape(16.0);
    let tall = base.leading(24.0);
    for (label, text, params, expected, single_line) in [
        ("empty", "", base, Size::ZERO, true),
        (
            "unbroken_legacy_short",
            "Hi",
            base,
            Size::new(16.0, 16.0),
            true,
        ),
        (
            "unbroken_legacy_long",
            "hello!!",
            base,
            Size::new(56.0, 16.0),
            true,
        ),
        (
            "wraps_below_unbroken",
            "12345678",
            base.width(32.0),
            Size::new(32.0, 32.0),
            false,
        ),
        (
            "line_height_param_short",
            "Hi",
            tall,
            Size::new(16.0, 24.0),
            true,
        ),
        (
            "line_height_param_wrapped",
            "12345678",
            tall.width(32.0),
            Size::new(32.0, 48.0),
            false,
        ),
    ] {
        let r = mono_shape(text, params, LineFit::Wrap);
        assert_eq!(r.size, expected, "case: {label}");
        assert_eq!(r.single_line, single_line, "case: {label}");
        assert_eq!(
            r.single_line,
            r.size.h <= params.line_height_px,
            "case: {label}: single_line must agree with the measured height",
        );
    }
    // Empty also produces the INVALID sentinel.
    assert!(mono_shape("", base, LineFit::Wrap).key.is_invalid());
}

#[test]
fn bundled_faces_resolve_and_their_metrics_differ() {
    // Each `FontFamily` / `FontWeight` pair must reach its intended
    // physical face. Asserted on the resolved family name and on advances,
    // not on the cache key — a key can discriminate perfectly while every
    // request silently falls back to one face.
    let mut c = CosmicMeasure::with_bundled_fonts();

    assert_eq!(
        c.resolved_family("M", FontFamily::Sans).as_deref(),
        Some("Inter"),
        "Sans must shape with the bundled Inter face",
    );
    assert_eq!(
        c.resolved_family("M", FontFamily::Mono).as_deref(),
        Some("JetBrains Mono"),
        "Mono must shape with the bundled JetBrains Mono face",
    );

    let width = |c: &mut CosmicMeasure, family, weight| {
        c.measure("MMMM", shape(16.0).family(family).weight(weight))
            .size
            .w
    };
    let sans = width(&mut c, FontFamily::Sans, FontWeight::Regular);
    let sans_bold = width(&mut c, FontFamily::Sans, FontWeight::Bold);
    let mono = width(&mut c, FontFamily::Mono, FontWeight::Regular);
    let mono_bold = width(&mut c, FontFamily::Mono, FontWeight::Bold);

    assert!(sans > 0.0 && sans.is_finite());
    assert_ne!(
        sans, mono,
        "Inter (proportional) and JBMono (monospace) differ for 'MMMM'",
    );
    assert!(
        sans_bold > sans,
        "Inter Bold ({sans_bold}) must be wider than Regular ({sans}) — a \
         smaller-or-equal width means Bold silently fell back to Regular",
    );
    // The variable mono face instantiates `wght` without changing the cell
    // width, so weight-invariance here is the correct expectation.
    assert_eq!(
        mono, mono_bold,
        "monospace advance must be weight-invariant",
    );
}

#[test]
fn text_wrap_policy_resolves_shape_and_layout_sizes_together() {
    #[derive(Clone, Copy, Debug)]
    struct Case {
        wrap: TextWrap,
        measured: Size,
        content: Size,
        min_content: Size,
        max_content: Size,
    }

    let mut text = TextSystem::mono();
    let widget_id = WidgetId::from_hash("wrap policy");
    let cases = [
        Case {
            wrap: TextWrap::SingleLine,
            measured: Size::new(56.0, 16.0),
            content: Size::new(56.0, 16.0),
            min_content: Size::new(56.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
        Case {
            wrap: TextWrap::Scroll,
            measured: Size::new(56.0, 16.0),
            content: Size::new(0.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(0.0, 16.0),
        },
        Case {
            wrap: TextWrap::Truncate,
            measured: Size::new(24.0, 16.0),
            content: Size::new(24.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
        Case {
            wrap: TextWrap::Ellipsis,
            measured: Size::new(24.0, 16.0),
            content: Size::new(24.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
        Case {
            wrap: TextWrap::Wrap,
            measured: Size::new(24.0, 48.0),
            content: Size::new(24.0, 48.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
        Case {
            wrap: TextWrap::WrapWithOverflow,
            measured: Size::new(32.0, 32.0),
            content: Size::new(32.0, 32.0),
            min_content: Size::new(32.0, 16.0),
            max_content: Size::new(56.0, 16.0),
        },
    ];

    let params = shape(16.0);
    for (ordinal, case) in cases.into_iter().enumerate() {
        let request = params.unbounded_request("aa bbbb");
        let slot = slot_at(widget_id, ordinal as u16);
        let unbounded = text.root(slot, request, case.wrap);
        let resolved = text.measure(slot, request, case.wrap, HAlign::Auto, Some(24.0));
        assert_eq!(resolved.measured, case.measured, "{case:?}");
        assert_eq!(
            case.wrap.content_size(resolved.measured),
            case.content,
            "{case:?}"
        );
        assert_eq!(
            case.wrap.min_content(&unbounded),
            case.min_content,
            "{case:?}"
        );
        assert_eq!(
            case.wrap.max_content(&unbounded),
            case.max_content,
            "{case:?}"
        );
    }

    let empty_slot = slot_at(widget_id, cases.len() as u16);
    let empty_request = params.unbounded_request("");
    let empty = text.measure(
        empty_slot,
        empty_request,
        TextWrap::Ellipsis,
        HAlign::Auto,
        Some(24.0),
    );
    assert_eq!(empty.measured, Size::ZERO);
    assert_eq!(TextWrap::Ellipsis.content_size(empty.measured), Size::ZERO);
    let empty_root = text.root(empty_slot, empty_request, TextWrap::Ellipsis);
    assert_eq!(TextWrap::Ellipsis.min_content(&empty_root), Size::ZERO);
    assert_eq!(TextWrap::Ellipsis.max_content(&empty_root), Size::ZERO);
}

#[test]
fn cosmic_empty_text_returns_invalid_zero_size() {
    // Empty-text early-return on the cosmic path: ZERO size, INVALID
    // key, zero intrinsic_min. Pins the renderer's "drop INVALID
    // runs" contract for empty strings.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let r = c.measure("", ui_shape(16.0));
    assert_eq!(r.size, Size::ZERO);
    assert!(r.key.is_invalid());
    assert_eq!(
        r.intrinsic_min,
        Some(0.0),
        "empty text has a genuinely zero floor, not an unscanned one",
    );
    // Nothing was cached for the empty input. Asserted against the cache
    // itself rather than by looking the sentinel up: it names no entry,
    // so `shaped_run` treats being asked as a misuse rather than a miss.
    assert_eq!(c.cache_len(), 0, "empty text must mint no shaped buffer");

    // The mono fallback agrees, and empty text short-circuits ahead of
    // the dispatch tally — a run with nothing to shape is not a shape.
    let shaper = TextShaper::test_mono();
    let calls = shaper.measure_calls();
    let r = shaper.measure("", ui_shape(16.0));
    assert_eq!(r.measured, Size::ZERO);
    assert!(r.key.is_invalid(), "empty text mints no shaped buffer");
    assert_eq!(shaper.measure_calls(), calls);
}

#[test]
fn cosmic_intrinsic_min_tracks_the_widest_unbreakable_segment() {
    // `intrinsic_min` is the wrap floor: the width of the widest segment
    // no line break can split. Break opportunities are UAX #14's — the
    // same ones cosmic-text splits its shape words on — so the floor has
    // to track punctuation and script boundaries, not just whitespace.
    let mut c = CosmicMeasure::with_bundled_fonts();
    let shape = ui_shape(16.0);

    // (run, the widest segment its floor must land on)
    for (text, widest) in [
        // "world" outweighs "hello" in Inter — `w` is the wider glyph.
        ("hello world hi", "world"),
        // A hyphen opens a break after itself, so the floor is the
        // prefix *including* it, not the whole token.
        ("aaa-bbb", "aaa-"),
        // Trailing punctuation binds to the word it follows.
        ("aaa, bbb", "aaa,"),
        // No whitespace anywhere: every ideograph is its own segment, so
        // a CJK paragraph must floor at one glyph rather than one line.
        (
            "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30c6}\u{30ad}\u{30b9}\u{30c8}",
            "\u{65e5}",
        ),
    ] {
        let full = c.measure(text, shape);
        let segment = c.measure(widest, shape);
        assert!(
            full.wrap_floor() < full.size.w,
            "{text:?} must break somewhere: floor {} vs total width {}",
            full.wrap_floor(),
            full.size.w,
        );
        // Kerning across a break can shift the in-run segment width a
        // little against its standalone measurement, so allow ±15%.
        let rel_err = (full.wrap_floor() - segment.wrap_floor()).abs() / segment.wrap_floor();
        assert!(
            rel_err < 0.15,
            "{text:?} floor ({}) must be the width of {widest:?} ({}), rel_err = {rel_err}",
            full.wrap_floor(),
            segment.wrap_floor(),
        );
    }

    // A no-break space opens no opportunity, so it neither splits its run
    // nor hangs — its own advance counts toward the segment.
    let nbsp = c.measure("aaa\u{a0}bbb", shape);
    assert!(
        (nbsp.wrap_floor() - nbsp.size.w).abs() < 2.0,
        "no-break space must keep one segment: floor {} vs width {}",
        nbsp.wrap_floor(),
        nbsp.size.w,
    );

    // Single-segment input: intrinsic_min ≈ size.w. size.w is the last
    // glyph's (x + w) ceil'd; intrinsic_min sums glyph widths. The two
    // differ by sub-pixel kerning / ceil rounding — allow 2 px.
    let hello = c.measure("hello", shape);
    assert!(
        (hello.wrap_floor() - hello.size.w).abs() < 2.0,
        "single word: intrinsic_min ({}) ≈ size.w ({})",
        hello.wrap_floor(),
        hello.size.w,
    );

    // Width-bounded shapes skip the segment scan and report a zero floor —
    // every consumer derives it from the unbounded root instead.
    let full = c.measure("hello world hi", shape);
    let bounded = c.measure("hello world hi", shape.width(60.0));
    assert!(bounded.size.h > full.size.h, "60 px must force a wrap");
    assert_eq!(
        bounded.intrinsic_min, None,
        "bounded shapes must not pay the segment scan",
    );
}

/// The wrap floor is scanned only for the policy that reads it, and is
/// backfilled when a cheaper policy reached the shared buffer first.
///
/// The unbounded key carries no wrap policy, so one shaped buffer answers
/// every run with the same text and face. A `Wrap` run therefore populates
/// the entry a `WrapWithOverflow` run will hit — and if "not scanned" were
/// stored as `0.0`, that second run would read a zero floor and let a long
/// word break instead of overflowing. It is only wrong when two policies
/// share a string, which is exactly the case a single-policy test misses.
#[test]
fn the_wrap_floor_is_scanned_on_demand_and_backfilled_for_a_later_policy() {
    let mut text = TextSystem::cosmic();
    let wid = WidgetId::from_hash("wrap-floor");
    // One long unbreakable word, so the floor is well clear of both zero
    // and the full run width.
    let content = "a extraordinarily b";
    let request = shape(16.0).leading(19.2).unbounded_request(content);

    // The five policies that never read the floor leave it unscanned —
    // this is the saving, and it is what makes the backfill necessary.
    let plain = text.root(slot_at(wid, 0), request, TextWrap::Wrap);
    assert_eq!(
        plain.intrinsic_min, None,
        "Wrap must not pay for a floor it never reads",
    );

    // A second slot over the same string now hits the buffer the first run
    // shaped, so the floor cannot come from shaping — it has to be scanned
    // against the resident buffer.
    let overflow = text.root(slot_at(wid, 1), request, TextWrap::WrapWithOverflow);
    let floor = overflow.wrap_floor();
    assert!(
        floor > 0.0 && floor < overflow.size.w,
        "backfilled floor {floor} must be a real segment width inside the \
         run's {} px, not a zero left behind by the Wrap run",
        overflow.size.w,
    );

    // Same value as a shaper that scanned from the start, so the backfill
    // is the real scan and not an approximation.
    let fresh = TextSystem::cosmic()
        .root(slot_at(wid, 0), request, TextWrap::WrapWithOverflow)
        .wrap_floor();
    assert_eq!(floor, fresh, "backfilled floor must equal a fresh scan");

    // And the policy change is picked up on the *same* slot too: the row's
    // key is unchanged, so only the floor's absence can trigger the refill.
    let same_slot = text.root(slot_at(wid, 0), request, TextWrap::WrapWithOverflow);
    assert_eq!(same_slot.wrap_floor(), fresh, "same-slot policy change");

    // The floor is what WrapWithOverflow floors its shaping width at, so a
    // committed width below it must be raised — the behaviour a zero floor
    // would silently lose.
    assert_eq!(
        TextWrap::WrapWithOverflow.target_width(1.0, &overflow),
        floor,
    );
    assert_eq!(TextWrap::Wrap.target_width(1.0, &overflow), 1.0);
}
