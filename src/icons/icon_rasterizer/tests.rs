use crate::common::span::Span;
use crate::icons::icon_raster_key::IconRasterKey;
use crate::icons::icon_rasterizer::{IconRasterizer, MAX_PARSED_TREES};
use crate::icons::icon_registry::IconSetId;
use crate::icons::icon_set::IconRef;
use crate::icons::icon_table::{IconDefinition, IconId, IconTable};
use crate::icons::internals::BROKEN;
use crate::primitives::paint::content_type::ContentType;
use crate::primitives::paint::raster_image::RasterImage;
use glam::{IVec2, U16Vec2, UVec2, Vec2};
use std::borrow::Cow;

/// A solid black square filling its 8x8 viewBox: coverage is exactly 255 everywhere.
const SOLID: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#000"/></svg>"##;
/// Left half opaque red, right half empty: a hand-checkable split and the colour path's fixture.
const HALF: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="4" height="8" fill="#ff0000"/><rect x="4" width="4" height="8" fill="#0000ff" fill-opacity="0.5"/></svg>"##;

/// The two fixtures as a set (ids follow `from_svgs`' name order).
fn fixtures() -> IconTable {
    IconTable::from_svgs([("half", HALF), ("solid", SOLID)]).unwrap()
}

const HALF_ID: IconId = IconId(0);
const SOLID_ID: IconId = IconId(1);

/// Rasterize and require an image.
fn raster<'a>(r: &'a mut IconRasterizer, table: &IconTable, key: IconRasterKey) -> RasterImage<'a> {
    r.rasterize(table, key).expect("fixture icon rasterizes")
}

fn key(icon: IconId, w: u16, h: u16) -> IconRasterKey {
    in_set(0, icon, w, h)
}

fn in_set(set: u16, icon: IconId, w: u16, h: u16) -> IconRasterKey {
    IconRasterKey::for_test(IconRef::fixture(set, icon.0), U16Vec2::new(w, h))
}

/// The parse cache is capped and evicts the longest-unrasterized.
#[test]
fn parse_cache_caps_at_the_ceiling_and_drops_the_coldest() {
    let table = IconTable::from_svgs((0..=MAX_PARSED_TREES).map(|i| {
        let name: &'static str = Box::leak(format!("i{i:03}").into_boxed_str());
        (name, SOLID)
    }))
    .unwrap();
    let icon = |n: usize| key(IconId(n as u16), 4, 4).icon;
    let mut r = IconRasterizer::default();
    for i in 0..MAX_PARSED_TREES {
        raster(&mut r, &table, key(IconId(i as u16), 4, 4));
    }
    assert_eq!(r.parsed_count(), MAX_PARSED_TREES, "filled to the ceiling");

    // Re-drawing icon 0 makes icon 1 the coldest; re-drawing a resident icon evicts nothing.
    raster(&mut r, &table, key(IconId(0), 4, 4));
    assert_eq!(r.parsed_count(), MAX_PARSED_TREES, "a hit evicts nothing");

    // The next *distinct* icon costs exactly one resident: icon 1.
    raster(&mut r, &table, key(IconId(MAX_PARSED_TREES as u16), 4, 4));
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
    let image = raster(&mut r, &table, key(SOLID_ID, 5, 5));
    assert_eq!(image.content, ContentType::Mask);
    // The box asked for, not the viewBox; the raster carries no bearing.
    assert_eq!(image.size, UVec2::new(5, 5));
    assert_eq!(image.bearing, IVec2::ZERO);
    assert_eq!(image.data.len(), 25);
    assert!(
        image.data.iter().all(|&c| c == 255),
        "a rect covering its whole viewBox is fully opaque everywhere, got {:?}",
        image.data,
    );

    let image = raster(&mut r, &table, key(SOLID_ID, 40, 40));
    assert_eq!(image.size, UVec2::new(40, 40));
    assert_eq!(image.data.len(), 1600);
    assert_eq!(r.parsed_count(), 1, "one parse serves every size");
}

#[test]
fn colour_icon_rasterizes_to_straight_srgb_rgba() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    let image = raster(&mut r, &table, key(HALF_ID, 8, 2));
    assert_eq!(image.content, ContentType::Color);
    assert_eq!(image.size, UVec2::new(8, 2));
    let out = image.data;
    assert_eq!(out.len(), 8 * 2 * 4);
    // Right half is blue at 50%, stored straight (255, not premultiplied 128): the rasterizer demultiplies before the atlas.
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

/// A hand-built set can hold an unparseable source; the rasterizer must fail once, not per frame.
#[test]
fn unparseable_icon_fails_once_and_is_not_retried() {
    static BROKEN_ICONS: [IconDefinition; 1] = [IconDefinition {
        name: Cow::Borrowed("broken"),
        view_box: Vec2::splat(8.0),
        svg: Span::new(0, BROKEN.len() as u32),
        tintable: true,
        filtered: false,
    }];
    let table = IconTable::baked(&BROKEN_ICONS, BROKEN.as_bytes());

    let mut r = IconRasterizer::default();
    assert!(r.rasterize(&table, key(IconId(0), 8, 8)).is_none());
    assert!(r.rasterize(&table, key(IconId(0), 9, 9)).is_none());
    assert_eq!(
        r.parsed_count(),
        1,
        "the failure is cached, so a broken icon costs one parse, not one per frame",
    );
}

/// Unloading a set drops its parses and nothing else's.
#[test]
fn forgetting_a_set_drops_its_parses_and_leaves_its_neighbours() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    for set in [0u16, 1] {
        for icon in [HALF_ID, SOLID_ID] {
            raster(&mut r, &table, in_set(set, icon, 8, 8));
        }
    }
    assert_eq!(r.parsed_count(), 4, "two icons in each of two sets");

    r.forget_sets(&[IconSetId::new(0, 0)]);
    assert_eq!(r.parsed_count(), 2, "only set 0's parses go");

    r.forget_sets(&[IconSetId::new(1, 1)]);
    assert_eq!(
        r.parsed_count(),
        2,
        "a different generation is a different set"
    );
    r.forget_sets(&[IconSetId::new(1, 0)]);
    assert_eq!(r.parsed_count(), 0);
}

/// Several sets released on one frame are forgotten in one walk, since `retain` costs the map's whole raw table.
#[test]
fn forgetting_a_batch_drops_exactly_its_members() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    for set in [0u16, 1, 2] {
        raster(&mut r, &table, in_set(set, SOLID_ID, 8, 8));
    }
    assert_eq!(r.parsed_count(), 3);

    r.forget_sets(&[IconSetId::new(0, 0), IconSetId::new(2, 0)]);
    assert_eq!(r.parsed_count(), 1, "both named sets go, in one pass");
    r.forget_sets(&[in_set(1, SOLID_ID, 8, 8).icon.set]);
    assert_eq!(r.parsed_count(), 0);
}

/// Non-square boxes render the artwork stretched to fill them, not letterboxed.
#[test]
fn non_square_box_stretches_rather_than_letterboxing() {
    let (mut r, table) = (IconRasterizer::default(), fixtures());
    let image = raster(&mut r, &table, key(SOLID_ID, 16, 4));
    assert_eq!(image.size, UVec2::new(16, 4));
    assert_eq!(image.data.len(), 64);
    assert!(
        image.data.iter().all(|&c| c == 255),
        "no transparent margin"
    );
}
