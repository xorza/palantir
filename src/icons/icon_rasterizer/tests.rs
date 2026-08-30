use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_rasterizer::{IconRasterizer, MAX_PARSED_TREES};
use crate::icons::icon_registry::IconSetId;
use crate::icons::icon_set::IconRef;
use crate::icons::icon_table::{IconDef, IconId, IconTable};
use crate::primitives::span::Span;
use crate::renderer::backend::raster_atlas::content_type::ContentType;
use glam::{U16Vec2, Vec2};

/// A solid black square filling its whole 8x8 viewBox: every pixel is
/// fully covered, so coverage is exactly 255 everywhere and the expected
/// output can be written down rather than eyeballed.
const SOLID: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#000"/></svg>"##;
/// Left half opaque red, right half empty — a hand-checkable split, and
/// the colour path's fixture.
const HALF: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="4" height="8" fill="#ff0000"/><rect x="4" width="4" height="8" fill="#0000ff" fill-opacity="0.5"/></svg>"##;
/// Broken markup — the parse must fail once and stay failed.
const BROKEN: &str = "<svg";

/// The three fixtures as a set. `leak_from_svgs` derives each one's
/// viewBox and tintability by parsing it, which is also what makes the
/// ids below the *name-sorted* order rather than the listed one.
fn fixtures() -> IconTable {
    IconTable::from_svgs([("half", HALF), ("solid", SOLID)])
}

// Name-sorted, not listed-order.
const HALF_ID: IconId = IconId(0);
const SOLID_ID: IconId = IconId(1);

fn key(icon: IconId, w: u16, h: u16) -> IconRasterKey {
    IconRasterKey::for_test(
        IconRef {
            set: IconSetId::new(0, 0),
            icon,
        },
        U16Vec2::new(w, h),
    )
}

/// The parse cache is capped, and what leaves is what has gone longest
/// without a rasterize — a session that draws its way through a large
/// set keeps a working set instead of every document it ever parsed.
#[test]
fn parse_cache_caps_at_the_ceiling_and_drops_the_coldest() {
    // `from_svgs` sorts by name and keys ids in that order, so the
    // zero-padded names make `IconId(i)` the `i`th icon.
    let table = IconTable::from_svgs((0..=MAX_PARSED_TREES).map(|i| {
        let name: &'static str = Box::leak(format!("i{i:03}").into_boxed_str());
        (name, SOLID)
    }));
    let icon = |n: usize| key(IconId(n as u16), 4, 4).icon;
    let mut r = IconRasterizer::default();
    let mut out = Vec::new();
    for i in 0..MAX_PARSED_TREES {
        r.rasterize(&table, key(IconId(i as u16), 4, 4), &mut out);
    }
    assert_eq!(r.parsed_count(), MAX_PARSED_TREES, "filled to the ceiling");

    // Re-drawing icon 0 makes icon 1 the coldest; re-drawing a resident
    // icon must not evict anything, since nothing new lands.
    r.rasterize(&table, key(IconId(0), 4, 4), &mut out);
    assert_eq!(r.parsed_count(), MAX_PARSED_TREES, "a hit evicts nothing");

    // The next *distinct* icon costs exactly one resident: icon 1.
    r.rasterize(&table, key(IconId(MAX_PARSED_TREES as u16), 4, 4), &mut out);
    assert_eq!(r.parsed_count(), MAX_PARSED_TREES);
    assert!(
        r.trees.contains_key(&icon(0)),
        "the icon touched most recently stays",
    );
    assert!(
        !r.trees.contains_key(&icon(1)),
        "the icon untouched longest is the one that left",
    );
    assert!(
        r.trees.contains_key(&icon(MAX_PARSED_TREES)),
        "and the newcomer is resident",
    );
}

#[test]
fn tintable_icon_rasterizes_to_full_coverage_at_the_exact_size() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    let mut out = Vec::new();
    assert_eq!(
        r.rasterize(&table, key(SOLID_ID, 5, 5), &mut out),
        Some(ContentType::Mask),
    );
    // One coverage byte per pixel of the box asked for — not of the
    // 8x8 viewBox, which is the whole point of rasterizing on demand.
    assert_eq!(out.len(), 25);
    assert!(
        out.iter().all(|&c| c == 255),
        "a rect covering its whole viewBox is fully opaque everywhere, got {out:?}",
    );

    // A second, larger size reuses the parse and produces that many px.
    out.clear();
    r.rasterize(&table, key(SOLID_ID, 40, 40), &mut out);
    assert_eq!(out.len(), 1600);
    assert_eq!(r.parsed_count(), 1, "one parse serves every size");
}

#[test]
fn colour_icon_rasterizes_to_straight_srgb_rgba() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    let mut out = Vec::new();
    assert_eq!(
        r.rasterize(&table, key(HALF_ID, 8, 2), &mut out),
        Some(ContentType::Color),
    );
    assert_eq!(out.len(), 8 * 2 * 4);
    // Left half: opaque red. Right half: blue at 50% — and stored
    // *straight*, so blue reads 255 rather than the 128 a premultiplied
    // buffer would hold. That distinction is the whole reason the
    // rasterizer demultiplies before handing pixels to the atlas.
    assert_eq!(&out[0..4], &[255, 0, 0, 255], "left half is opaque red");
    assert_eq!(&out[12..16], &[255, 0, 0, 255]);
    let right = &out[16..20];
    assert_eq!(right[0], 0, "no red on the right");
    assert_eq!(right[2], 255, "blue is straight, not premultiplied by 0.5");
    assert!(
        (127..=128).contains(&right[3]),
        "fill-opacity 0.5 is alpha 127 or 128, got {}",
        right[3],
    );
}

/// `leak_from_svgs` drops a source that will not parse, so reaching this
/// path takes a hand-built set — which a baked one effectively is. The
/// rasterizer still has to fail *once* rather than once per frame.
#[test]
fn unparseable_icon_fails_once_and_is_not_retried() {
    static BROKEN_ICONS: [IconDef; 1] = [IconDef {
        name: "broken",
        view_box: Vec2::splat(8.0),
        svg: Span::new(0, BROKEN.len() as u32),
        tintable: true,
        filtered: false,
    }];
    let table = IconTable::baked(&BROKEN_ICONS, BROKEN.as_bytes());

    let mut r = IconRasterizer::default();
    let mut out = Vec::new();
    assert_eq!(r.rasterize(&table, key(IconId(0), 8, 8), &mut out), None);
    assert_eq!(r.rasterize(&table, key(IconId(0), 9, 9), &mut out), None);
    assert_eq!(
        r.parsed_count(),
        1,
        "the failure is cached, so a broken icon costs one parse, not one per frame",
    );
}

/// Unloading a set drops its parses and nothing else's. This is the
/// expensive half of the unload: one parsed document per icon the
/// session drew, held for as long as the set was loaded.
#[test]
fn forgetting_a_set_drops_its_parses_and_leaves_its_neighbours() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    let mut out = Vec::new();
    for set in [0u16, 1] {
        for icon in [HALF_ID, SOLID_ID] {
            let mut k = key(icon, 8, 8);
            k.icon.set = IconSetId::new(set, 0);
            r.rasterize(&table, k, &mut out);
        }
    }
    assert_eq!(r.parsed_count(), 4, "two icons in each of two sets");

    r.forget_sets(&[IconSetId::new(0, 0)]);
    assert_eq!(r.parsed_count(), 2, "only set 0's parses go");

    // The generation is part of the identity: the slot's next occupant
    // is not the set that was forgotten.
    r.forget_sets(&[IconSetId::new(1, 1)]);
    assert_eq!(
        r.parsed_count(),
        2,
        "a different generation is a different set"
    );
    r.forget_sets(&[IconSetId::new(1, 0)]);
    assert_eq!(r.parsed_count(), 0);
}

/// Several sets released on one frame are forgotten in one walk, and
/// the batch is what makes that possible: `retain` costs the map's
/// whole raw table, so a per-set call paid that once per set.
#[test]
fn forgetting_a_batch_drops_exactly_its_members() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    let mut out = Vec::new();
    for set in [0u16, 1, 2] {
        let mut k = key(SOLID_ID, 8, 8);
        k.icon.set = IconSetId::new(set, 0);
        r.rasterize(&table, k, &mut out);
    }
    assert_eq!(r.parsed_count(), 3);

    r.forget_sets(&[IconSetId::new(0, 0), IconSetId::new(2, 0)]);
    assert_eq!(r.parsed_count(), 1, "both named sets go, in one pass");
    // And it is set 1 that survived, not whichever was cheapest to keep.
    let mut survivor = key(SOLID_ID, 8, 8);
    survivor.icon.set = IconSetId::new(1, 0);
    r.forget_sets(&[survivor.icon.set]);
    assert_eq!(r.parsed_count(), 0);
}

/// Non-square boxes must render the artwork stretched to fill them, not
/// letterboxed — the fit decision was already made upstream, and the
/// rasterizer's contract is "fill exactly this many pixels".
#[test]
fn non_square_box_stretches_rather_than_letterboxing() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    let mut out = Vec::new();
    r.rasterize(&table, key(SOLID_ID, 16, 4), &mut out);
    assert_eq!(out.len(), 64);
    assert!(out.iter().all(|&c| c == 255), "no transparent margin");
}
