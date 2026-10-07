use super::*;
use crate::layout::measured::Measured;

#[test]
fn mono_measure_cases() {
    // Mono lays every `char` out `font_size * 0.5` wide on a `line_height` band, so expected sizes are arithmetic; heights pin the wrap. No empty case; see `an_empty_run_is_answered_at_the_boundary_and_shapes_nothing`.
    let base = shape(16.0);
    let tall = base.leading(24.0);
    for (label, text, params, expected) in [
        ("unbroken_legacy_short", "Hi", base, Size::new(16.0, 16.0)),
        (
            "unbroken_legacy_long",
            "hello!!",
            base,
            Size::new(56.0, 16.0),
        ),
        (
            "wraps_below_unbroken",
            "12345678",
            base.width(32.0),
            Size::new(32.0, 32.0),
        ),
        (
            "fits_inside_bound",
            "Hi",
            base.width(32.0),
            Size::new(16.0, 16.0),
        ),
        ("line_height_param_short", "Hi", tall, Size::new(16.0, 24.0)),
        // Seven chars in fourteen bytes: the width is the chars', 7 × 8.
        (
            "multibyte_counts_chars",
            "ééééééé",
            base,
            Size::new(56.0, 16.0),
        ),
        (
            "multibyte_wraps_by_chars",
            "éééééééé",
            base.width(32.0),
            Size::new(32.0, 32.0),
        ),
        (
            "line_height_param_wrapped",
            "12345678",
            tall.width(32.0),
            Size::new(32.0, 48.0),
        ),
    ] {
        assert_eq!(
            mono_extent(text, params, LineFit::Wrap),
            expected,
            "case: {label}"
        );
    }
}

/// The mono root reports what only an unbounded shape has, as the cosmic root does: one visual line and a UAX #14 wrap floor. `single_line` gates `TextSystem::measure`'s fitting-truncate skip; losing it would reshape every fitting label.
#[test]
fn the_mono_root_reports_one_line_and_a_segment_floor() {
    let params = shape(16.0);
    let root = mono_root("hello world", params);
    assert!(root.single_line, "an unbounded mono run is one line");
    assert_eq!(root.extent.size, Size::new(88.0, 16.0), "11 bytes × 8 px");
    // "hello " and "world" are the two unbreakable segments; the space hangs off the first, so both measure five 8 px cells.
    assert_eq!(root.wrap_floor(), 40.0);

    assert_eq!(mono_root("abcdefg ", params).wrap_floor(), 56.0);
    assert_eq!(mono_root("héllo wörld", params).wrap_floor(), 40.0);
}

#[test]
fn bundled_faces_resolve_and_their_metrics_differ() {
    // Each `FontFamily` / `FontWeight` pair must reach its intended face, asserted on the resolved family and advances, not the cache key (which can discriminate while every request falls back to one face).
    let mut c = CosmicMeasure::default();

    assert_eq!(
        c.resolved_family("M", shape(16.0).family(FontFamily::SANS).font)
            .as_deref(),
        Some("Inter"),
        "Sans must shape with the bundled Inter face",
    );
    assert_eq!(
        c.resolved_family("M", shape(16.0).family(FontFamily::MONO).font)
            .as_deref(),
        Some("JetBrains Mono"),
        "Mono must shape with the bundled JetBrains Mono face",
    );

    let width = |c: &mut CosmicMeasure, family, weight| {
        c.measure("MMMM", shape(16.0).family(family).weight(weight))
            .size
            .w
    };
    let sans = width(&mut c, FontFamily::SANS, FontWeight::REGULAR);
    let sans_bold = width(&mut c, FontFamily::SANS, FontWeight::BOLD);
    let mono = width(&mut c, FontFamily::MONO, FontWeight::REGULAR);
    let mono_bold = width(&mut c, FontFamily::MONO, FontWeight::BOLD);

    // Widths round up to whole pixels. JetBrains Mono's cell is 600/1000 em, 9.6 px at 16 px: four is 38.4 → 39. Inter is proportional; 58.0 is four 'M' advances.
    assert_eq!(
        (sans, mono),
        (58.0, 39.0),
        "bundled-face advances for 'MMMM'"
    );
    assert_eq!(
        sans_bold, 60.0,
        "Inter Bold is wider than Regular's 58 — an equal width means Bold \
         silently fell back to Regular",
    );
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
        stable_from_w: [f32; 3],
    }

    // "aa bbbb" is 7 cells of 8 px: 56 wide on one line. A never-binding policy holds anywhere; a truncating fit holds from 56 up and only at its offer where it cuts; a wrapping one binds to every width.
    const AT: f32 = Measured::AT_OFFER_ONLY;
    let mut text = TextSystem::mono();
    let widget_id = WidgetId::from_hash("wrap policy");
    let cases = [
        Case {
            wrap: TextWrap::SingleLine,
            measured: Size::new(56.0, 16.0),
            content: Size::new(56.0, 16.0),
            min_content: Size::new(56.0, 16.0),
            max_content: Size::new(56.0, 16.0),
            stable_from_w: [0.0, 0.0, 0.0],
        },
        Case {
            wrap: TextWrap::Scroll,
            measured: Size::new(56.0, 16.0),
            content: Size::new(0.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(0.0, 16.0),
            stable_from_w: [0.0, 0.0, 0.0],
        },
        Case {
            wrap: TextWrap::Truncate,
            measured: Size::new(24.0, 16.0),
            content: Size::new(24.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
            stable_from_w: [AT, 56.0, 56.0],
        },
        Case {
            wrap: TextWrap::Ellipsis,
            measured: Size::new(24.0, 16.0),
            content: Size::new(24.0, 16.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
            stable_from_w: [AT, 56.0, 56.0],
        },
        Case {
            wrap: TextWrap::Wrap,
            measured: Size::new(24.0, 48.0),
            content: Size::new(24.0, 48.0),
            min_content: Size::new(0.0, 16.0),
            max_content: Size::new(56.0, 16.0),
            stable_from_w: [AT, AT, AT],
        },
        Case {
            wrap: TextWrap::WrapWithOverflow,
            measured: Size::new(32.0, 32.0),
            content: Size::new(32.0, 32.0),
            min_content: Size::new(32.0, 16.0),
            max_content: Size::new(56.0, 16.0),
            stable_from_w: [AT, AT, AT],
        },
    ];

    let params = shape(16.0);
    for (ordinal, case) in cases.into_iter().enumerate() {
        let request = params.unbounded_request("aa bbbb");
        let slot = slot_at(widget_id, ordinal as u16);
        let unbounded = text.root(slot, request, case.wrap);
        let resolved = text.measure(slot, request, case.wrap, HAlign::Auto, Some(24.0));
        let resolved_stable_from_w = [Some(24.0), Some(64.0), None].map(|width| {
            text.measure(slot, request, case.wrap, HAlign::Auto, width)
                .stable_from_w
        });
        assert_eq!(resolved_stable_from_w, case.stable_from_w, "{case:?}");
        let resolved = resolved.shaped;
        assert_eq!(resolved.extent.size, case.measured, "{case:?}");
        assert_eq!(
            case.wrap.content_size(resolved.extent.size),
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
}

#[test]
fn an_empty_run_is_answered_at_the_boundary_and_shapes_nothing() {
    // **The one empty-text boundary.** A `TextShapeRequest` cannot hold a run with no bytes, so no layer below guards; the crate edge answers it itself.
    let params = ui_shape(16.0);
    assert!(
        TextShapeRequest::unbounded("", params.font).is_none(),
        "an empty run has no request to make of the shaper",
    );

    // A usable face still names a key; what no bytes mint is the buffer, so the encoder drops the run.
    assert!(
        TextRun {
            text: "",
            font: params.font,
            wrap: TextWrap::Wrap,
            align: Align::h(HAlign::Auto),
            max_width: None,
        }
        .unbounded_key()
        .is_some(),
        "a usable face names a key whether or not there are bytes to shape",
    );

    // Both metrics answer alike through the probe edge: zero extent, no buffer key, no dispatch.
    for shaper in [TextShaper::new(), TextShaper::test_mono()] {
        let calls = shaper.measure_calls();
        let measured = shaper.measure("", params);
        assert_eq!(measured.extent.size, Size::ZERO);
        assert!(measured.key.is_none(), "empty text mints no buffer");
        assert_eq!(shaper.measure_calls(), calls, "no dispatch for no bytes");
        assert_eq!(shaper.cosmic_cache_len(), 0, "and no cached buffer");

        // What an empty block still answers: one position, at its origin, on a band of the requested height.
        shaper.probe_layout("", params, |probe| {
            assert_eq!(probe.size(), Size::ZERO);
            let caret = probe.caret_at(0);
            assert_eq!(caret.x, 0.0);
            assert_eq!(caret.y_top, 0.0);
            // `TextShapeKey` quantizes leading to 1/64 px, so the empty block's band is round(19.2 × 64) / 64.
            assert_eq!(caret.line_height, 19.203125);
            assert_eq!(probe.byte_at(37.0, 5.0), 0, "every point is byte 0");
        });
    }
}

#[test]
fn cosmic_intrinsic_min_tracks_the_widest_unbreakable_segment() {
    // `intrinsic_min` is the wrap floor: the widest segment no UAX #14 break can split (matching cosmic-text's shape words), so it tracks punctuation and script boundaries, not just whitespace.
    let mut c = CosmicMeasure::default();
    c.load_font(HEBREW.into())
        .expect("the Hebrew test face loads");
    let shape = ui_shape(16.0);

    for (text, widest) in [
        // Right to left: segments are read in logical order; read visually, resets came a glyph late and merged two words.
        (
            "\u{5d0}\u{5d1}\u{5d2}\u{5d3}\u{5d4} \u{5d5}\u{5d6}",
            "\u{5d0}\u{5d1}\u{5d2}\u{5d3}\u{5d4}",
        ),
        (
            "ab \u{5d0}\u{5d1}\u{5d2}\u{5d3}\u{5d4} cd",
            "\u{5d0}\u{5d1}\u{5d2}\u{5d3}\u{5d4}",
        ),
        ("hello world hi", "world"),
        ("aaa-bbb", "aaa-"),
        ("aaa, bbb", "aaa,"),
        // No whitespace: every ideograph is its own segment, so a CJK paragraph floors at one glyph, not one line.
        (
            "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30c6}\u{30ad}\u{30b9}\u{30c8}",
            "\u{65e5}",
        ),
    ] {
        let full = c.measure(text, shape);
        let segment = c.measure(widest, shape);
        assert_eq!(full.wrap_floor(), segment.size.w, "{text:?} vs {widest:?}");
        assert_eq!(
            segment.wrap_floor(),
            segment.size.w,
            "{widest:?} is one segment"
        );
    }

    // A no-break space opens no break: it neither splits nor hangs, and its advance counts. Floors equal the run width, integral on the whole-pixel grid the wrap width snaps to.
    for (text, width) in [("aaa\u{a0}bbb", 61.0), ("hello", 37.0)] {
        let measured = c.measure(text, shape);
        assert_eq!(measured.size.w, width, "{text:?}");
        assert_eq!(measured.wrap_floor(), width, "{text:?}: one segment");
    }
    assert_eq!(
        c.measure("supercalifragilistic", shape).wrap_floor(),
        138.0,
        "a long word floors at its whole-pixel width",
    );

    let full = c.measure("hello world hi", shape);
    let bounded = c.measure("hello world hi", shape.width(60.0));
    // One line is 19.203125, ceiled to 20. At 60 px the run breaks into "hello", "world", "hi": 3 × 19.203125 = 57.609375, ceiled to 58, as wide as its widest line, "world".
    assert_eq!(full.size, Size::new(101.0, 20.0));
    assert_eq!(bounded.size, Size::new(43.0, 58.0), "60 px forces a wrap");
    assert_eq!(
        bounded.intrinsic_min, None,
        "bounded shapes must not pay the segment scan",
    );
}

/// The wrap floor is scanned only for the policy that reads it, and backfilled when a cheaper policy reached the shared buffer first: the unbounded key carries no policy, and storing "not scanned" as `0.0` would let a `WrapWithOverflow` run break a long word instead of overflowing.
#[test]
fn the_wrap_floor_is_scanned_on_demand_and_backfilled_for_a_later_policy() {
    let mut text = TextSystem::cosmic();
    let wid = WidgetId::from_hash("wrap-floor");
    let content = "a extraordinarily b";
    let request = shape(16.0).leading(19.2).unbounded_request(content);

    let plain = text.root(slot_at(wid, 0), request, TextWrap::Wrap);
    assert_eq!(
        plain.intrinsic_min, None,
        "Wrap must not pay for a floor it never reads",
    );

    let overflow = text.root(slot_at(wid, 1), request, TextWrap::WrapWithOverflow);
    let floor = overflow.wrap_floor();
    assert_eq!(overflow.extent.size.w, 137.0);
    assert_eq!(
        floor, 109.0,
        "the backfilled floor is the width of \"extraordinarily\", not a zero \
         left behind by the Wrap run",
    );

    let fresh = TextSystem::cosmic()
        .root(slot_at(wid, 0), request, TextWrap::WrapWithOverflow)
        .wrap_floor();
    assert_eq!(floor, fresh, "backfilled floor must equal a fresh scan");

    // A policy change on the same slot is picked up too: the row's key is unchanged, so only the floor's absence triggers the refill.
    let same_slot = text.root(slot_at(wid, 0), request, TextWrap::WrapWithOverflow);
    assert_eq!(same_slot.wrap_floor(), fresh, "same-slot policy change");

    // WrapWithOverflow floors its shaping width at the floor, so a committed width below it must be raised; a zero floor would lose that.
    assert_eq!(
        TextWrap::WrapWithOverflow.target_width(1.0, &overflow),
        floor,
    );
    assert_eq!(TextWrap::Wrap.target_width(1.0, &overflow), 1.0);
}

/// A probe and the paint must shape under the same key for every policy whose committed width isn't the width offered. `TextShaper::layout` resolves those beside the shaping call; before, a probe got a different buffer than paint and the caret sat in the wrong place.
#[test]
fn a_probe_shapes_under_the_key_the_paint_committed() {
    let content = "a extraordinarily b";
    let params = shape(16.0).leading(19.2);
    let wid = WidgetId::from_hash("probe-key-parity");
    let probed_width = 1.0;

    for (ordinal, wrap) in [
        TextWrap::WrapWithOverflow,
        TextWrap::Ellipsis,
        TextWrap::Truncate,
        TextWrap::Wrap,
    ]
    .into_iter()
    .enumerate()
    {
        let (text, width) = match wrap {
            TextWrap::Ellipsis | TextWrap::Truncate => ("hi", 512.0),
            _ => (content, probed_width),
        };
        let mut system = TextSystem::cosmic();
        let request = params.unbounded_request(text);
        let painted = system
            .measure(
                slot_at(wid, ordinal as u16),
                request,
                wrap,
                HAlign::Auto,
                Some(width),
            )
            .shaped
            .key;

        let probed = system.shaper().layout(&TextRun {
            text,
            font: params.font,
            wrap,
            align: Align::h(HAlign::Auto),
            max_width: Some(width),
        });
        assert_eq!(
            probed.shaped_key(),
            painted,
            "{wrap:?}: probe and paint must share one shaped buffer",
        );

        // Prove the case is live: for the two resolving policies the committed key is not the raw bind of `width`, so agreement is the resolution working, not both sides binding the same number.
        let raw = TextShapeRequest::unbounded(text, params.font)
            .expect("the fixture has text")
            .with_bound(WrapBound::new(
                width,
                HAlign::Auto,
                wrap.line_fit().expect("all four policies bind"),
            ))
            .key();
        match wrap {
            TextWrap::Wrap => {
                assert_eq!(Some(raw), painted, "Wrap commits the width it was offered");
            }
            _ => assert_ne!(
                Some(raw),
                painted,
                "{wrap:?}: a raw bind must differ, or this case proves nothing",
            ),
        }
    }
}

/// A probe answers in the alignment the run asked for, not the one its cache key carries: an unbounded key stores `LineAlign::Auto` whatever the run said, which once put a right-aligned run's caret at the block's left edge.
#[test]
fn a_glyphless_line_takes_its_caret_from_the_run_not_the_key() {
    let text = "wide enough\n\ntail";
    let empty_line_byte = "wide enough\n".len();
    let shaper = TextShaper::new();
    let run = |halign| TextRun {
        text,
        font: shape(16.0).leading(19.2).font,
        wrap: TextWrap::SingleLine,
        align: Align::h(halign),
        max_width: None,
    };

    let left = shaper.layout(&run(HAlign::Left));
    let block_w = left.size().w;
    assert_eq!(
        left.caret_at(empty_line_byte).x,
        0.0,
        "a left-aligned empty line starts at the block's left edge",
    );
    drop(left);
    assert_eq!(block_w, 98.0, "\"wide enough\" is the widest line");

    let right = shaper.layout(&run(HAlign::Right));
    assert_eq!(
        right.caret_at(empty_line_byte).x,
        block_w,
        "a right-aligned empty line puts the caret where the first typed \
         glyph will land",
    );
    drop(right);

    let centre = shaper.layout(&run(HAlign::Center));
    assert_eq!(centre.caret_at(empty_line_byte).x, block_w * 0.5);
}
