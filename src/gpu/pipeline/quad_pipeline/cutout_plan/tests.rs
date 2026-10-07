use crate::gpu::pipeline::quad_pipeline::cutout_plan::{
    BakeTable, Census, CornerTables, CutoutPlan, ShadowEntry, fit_radii, spread_radius,
};
use crate::gpu::surface::viewport::RepaintScissors;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::renderer::quad::Quad;
use glam::{UVec2, Vec2};

/// A shadow of `kind` over a `w`×`h` quad, every corner `radius`, blurred by
/// `sigma` with no spread.
fn shadow(kind: FillKind, w: f32, h: f32, radius: f32, sigma: f32) -> Quad {
    Quad {
        rect: Rect::new(0.0, 0.0, w, h),
        corners: Corners::all(radius),
        fill_kind: kind,
        fill_axis: FillAxis::from_lanes(0.0, 0.0, sigma, 0.0),
        ..Quad::default()
    }
}

/// A viewport larger than any fixture here, so it hides nothing.
const WIDE: UVec2 = UVec2::new(8192, 8192);

/// `quads` planned as a full repaint of a `viewport`, with baking on.
fn plan_in(quads: &[Quad], viewport: UVec2) -> CutoutPlan {
    let mut plan = CutoutPlan::new(true);
    full(&mut plan, quads, viewport);
    plan
}

fn plan(quads: &[Quad]) -> CutoutPlan {
    plan_in(quads, WIDE)
}

/// `plan` rebuilt as a full repaint, which is never stale.
fn full(plan: &mut CutoutPlan, quads: &[Quad], viewport: UVec2) {
    let census = plan.build(quads, &RepaintScissors::Full, viewport);
    assert_eq!(census, Census::Current, "a full repaint keeps no pixel");
}

/// `n` drop shadows 100 px square, radius 8 and σ = 2, side by side 120 px
/// apart from `x`.
fn row(n: usize, x: f32) -> Vec<Quad> {
    (0..n)
        .map(|k| {
            let mut quad = shadow(FillKind::SHADOW_DROP, 100.0, 100.0, 8.0, 2.0);
            quad.rect.min.x = x + 120.0 * k as f32;
            quad
        })
        .collect()
}

/// The two radius rules follow the shader's arithmetic. Spread 4 moves a
/// radius at least 4 by 4 (8 → 12, 4 → 8, 10 → 14) and pulls one below it
/// toward the plain one by `1 + (r / s − 1)³`, so 0 stays 0; a negative
/// spread subtracts, floored at 0. The fit scales by the least side over its
/// radii: the box is 20×10, the left side's radii `12 + 14 = 26` against
/// 10, so `f = 10 / 26`.
#[test]
fn radius_rules_follow_the_shader() {
    assert_eq!(
        spread_radius([8.0, 0.0, 4.0, 10.0], 4.0),
        [12.0, 0.0, 8.0, 14.0]
    );
    assert_eq!(
        spread_radius([8.0, 0.0, 4.0, 10.0], -3.0),
        [5.0, 0.0, 1.0, 7.0]
    );
    let f = 10.0 / 26.0;
    assert_eq!(
        fit_radii([12.0, 0.0, 8.0, 14.0], Vec2::new(10.0, 5.0)),
        [12.0 * f, 0.0, 8.0 * f, 14.0 * f],
    );
    assert_eq!(
        fit_radii([1.0; 4], Vec2::new(10.0, 5.0)),
        [1.0; 4],
        "room to spare"
    );
}

/// A 400×300 drop shadow blurred by 16, radius 8: its box is the source
/// inset by `4σ = 64`, which has room for the radius, so all four corners
/// share the key `(8, 16)`. `reach = 64.5`, and the side is
/// `floor((8 + 129) · 10 / 16) + 2 = 87` texels, at cell `(0, 0)`. An
/// inset shadow with the same key reuses the table, and so does a second
/// drop shadow, so the frame bakes one. Every shadow reads its corners from
/// it, so each draws through the tables entry, and the quad between them,
/// which is no shadow, through the general one.
#[test]
fn equal_corners_share_one_table() {
    let quads = [
        shadow(FillKind::SHADOW_DROP, 400.0, 300.0, 8.0, 16.0),
        Quad::default(),
        shadow(FillKind::SHADOW_INSET, 200.0, 200.0, 8.0, 16.0),
        shadow(FillKind::SHADOW_DROP, 500.0, 300.0, 8.0, 16.0),
    ];
    let plan = plan(&quads);
    let code = CutoutPlan::code([0, 0], 87);
    assert_eq!(
        plan.tables(),
        [BakeTable {
            table: code,
            r: 8.0,
            sigma: 16.0,
        }],
    );
    assert_eq!(
        plan.corners(),
        [
            CornerTables([code; 4]),
            CornerTables([CutoutPlan::NONE; 4]),
            CornerTables([code; 4]),
            CornerTables([code; 4]),
        ],
    );
    assert_eq!(
        plan.entries(),
        [
            ShadowEntry::Tables,
            ShadowEntry::General,
            ShadowEntry::Tables,
            ShadowEntry::Tables,
        ],
    );
}

/// A corner keeps the shaded cutout when its blur is below the cutout form,
/// when it has no radius to cut, when its table would pass the side budget,
/// and when baking is off. A frame with no shadow plans nothing at all.
///
/// Only a corner with a radius needs its table, so a shadow with no radius
/// still draws through the tables entry, and the rest through the general
/// one.
#[test]
fn corners_without_a_table_keep_the_shaded_cutout() {
    let none = CornerTables([CutoutPlan::NONE; 4]);
    for (label, quad, entry) in [
        (
            "blur below the cutout form",
            shadow(FillKind::SHADOW_DROP, 400.0, 300.0, 8.0, 0.2),
            ShadowEntry::General,
        ),
        (
            "no radius",
            shadow(FillKind::SHADOW_DROP, 400.0, 300.0, 0.0, 16.0),
            ShadowEntry::Tables,
        ),
        // σ = 0.25 against a radius of 40: (40 + 3) · 40 + 2 = 1722 texels.
        (
            "past the side budget",
            shadow(FillKind::SHADOW_INSET, 400.0, 300.0, 40.0, 0.25),
            ShadowEntry::General,
        ),
    ] {
        let plan = plan(&[quad]);
        assert!(plan.tables().is_empty(), "{label}");
        assert_eq!(plan.corners(), [none], "{label}");
        assert_eq!(plan.entries(), [entry], "{label}");
    }
    let mut off = CutoutPlan::new(false);
    let census = off.build(
        &[shadow(FillKind::SHADOW_DROP, 400.0, 300.0, 8.0, 16.0)],
        &RepaintScissors::Full,
        WIDE,
    );
    assert_eq!(census, Census::Current);
    assert!(off.tables().is_empty());
    assert_eq!(off.corners(), [none], "baking off");
    assert_eq!(off.entries(), [ShadowEntry::General], "baking off");
    let no_shadow = plan(&[Quad::default()]);
    assert!(no_shadow.corners().is_empty(), "no shadow");
    assert!(no_shadow.entries().is_empty(), "no shadow");
}

/// Tables pack tallest first along a shelf of cells, start a new shelf when
/// a row is full, and stop when the atlas is: what is left keeps the shaded
/// cutout. At σ = 16 a radius from 8 to 14.25 makes 87 to 91 texels, 6
/// cells of 16 either way, so a 64-cell row holds 10 tables. 100 keys fill
/// 10 shelves of 10, and the last in packing order is left out: the
/// smallest side, 87 texels, holds the radii below `137.6 − 129 = 8.6`, and
/// among those ties the packer goes by key, so the largest, 8.5625, is
/// last. The tables list in key order, so the cells are compared as a set.
#[test]
fn tables_pack_into_shelves_until_the_atlas_is_full() {
    // Radii 1/16 apart, which `f16` holds exactly between 8 and 16.
    let quads: Vec<Quad> = (0..101)
        .map(|k| {
            shadow(
                FillKind::SHADOW_DROP,
                400.0,
                300.0,
                8.0 + k as f32 / 16.0,
                16.0,
            )
        })
        .collect();
    let plan = plan(&quads);
    assert_eq!(plan.tables().len(), 100);
    let mut cells: Vec<[u32; 2]> = plan
        .tables()
        .iter()
        .map(|table| [table.table & 63, (table.table >> 6) & 63])
        .collect();
    cells.sort_unstable();
    let mut want: Vec<[u32; 2]> = (0..10)
        .flat_map(|y| (0..10).map(move |x| [x * 6, y * 6]))
        .collect();
    want.sort_unstable();
    assert_eq!(cells, want);
    let unplaced = plan
        .corners()
        .iter()
        .filter(|corners| **corners == CornerTables([CutoutPlan::NONE; 4]))
        .count();
    assert_eq!(unplaced, 1, "the one key past the atlas");
    assert_eq!(
        plan.corners()[9],
        CornerTables([CutoutPlan::NONE; 4]),
        "radius 8.5625, packed last"
    );
}

/// A table is baked only where it is cheaper than shading the corners that
/// share it. At σ = 2 and r = 8 a 100 px drop shadow's box is inset by
/// `4σ = 8`, so the top-left corner's region runs from `8 − 8.5 = −0.5` to
/// `8 + 8 + 8.5 = 24.5` on each axis; the quad starts at 0, so 24.5 px of
/// it are shown, 600.25 px² at 12 nodes, 7 203 per corner. Its table is
/// `25 · 5 + 2 = 127` texels square at 24 nodes, 387 096. So 53 corners
/// shade cheaper and 54 do not: 13 shadows of 4 corners keep the shaded
/// cutout, 14 get the table. A region the viewport does not show costs
/// nothing to shade, so a shadow off screen adds nothing.
///
/// The entry follows the shown corners: shaded ones draw through the
/// general entry, and a shadow whose untabled corners the viewport does not
/// show through the tables one, whether it shows none of them or some.
#[test]
fn a_table_is_baked_where_it_is_cheaper_than_shading() {
    for (n, baked, entry) in [
        (13, false, ShadowEntry::General),
        (14, true, ShadowEntry::Tables),
    ] {
        let plan = plan(&row(n, 0.0));
        assert_eq!(plan.tables().len(), usize::from(baked), "{n} shadows");
        assert!(
            plan.entries().iter().all(|e| *e == entry),
            "{n} shadows: {:?}",
            plan.entries(),
        );
    }
    // 13 shadows on screen and 1 past its right edge: still not worth a
    // table.
    let mut quads = row(13, 0.0);
    quads.extend(row(1, 5000.0));
    let plan = plan_in(&quads, UVec2::new(2000, 200));
    assert!(plan.tables().is_empty());
    assert!(
        plan.corners()
            .iter()
            .all(|corners| *corners == CornerTables([CutoutPlan::NONE; 4]))
    );
    let mut want = vec![ShadowEntry::General; 13];
    want.push(ShadowEntry::Tables);
    assert_eq!(plan.entries(), want, "the one off screen shows nothing");
    // A viewport 60 px wide: the right corners' regions start at
    // `100 − 8 − 8 − 8.5 = 75.5`, so none is shown and the shaded left
    // corners decide.
    let plan = plan_in(&row(1, 0.0), UVec2::new(60, 100));
    assert_eq!(plan.entries(), [ShadowEntry::General]);
    // 14 shadows at x = −80, each showing only its last 20 px: the left
    // corners' regions end at `−80 + 24.5`, off screen, and two shown
    // corners each do not pay for a table either.
    let quads: Vec<Quad> = row(14, 0.0)
        .into_iter()
        .map(|quad| Quad {
            rect: Rect::new(-80.0, 0.0, 100.0, 100.0),
            ..quad
        })
        .collect();
    let plan = plan_in(&quads, UVec2::new(2000, 200));
    assert!(plan.tables().is_empty());
    assert!(plan.entries().iter().all(|e| *e == ShadowEntry::General));
    // A viewport 50 px tall over 28 shadows rounded 8 on top and 4 below:
    // 56 shown top corners pay for the `(8, 2)` table, and the bottom ones,
    // whose regions start at `92 − 4 − 8.5 = 79.5`, are not shown, so their
    // `(4, 2)` key gets none and every shadow still reads tables.
    let quads: Vec<Quad> = row(28, 0.0)
        .into_iter()
        .map(|quad| Quad {
            corners: Corners::new(8.0, 8.0, 4.0, 4.0),
            ..quad
        })
        .collect();
    let plan = plan_in(&quads, UVec2::new(4000, 50));
    assert_eq!(plan.tables().len(), 1);
    let code = plan.tables()[0].table;
    assert_eq!(
        plan.corners()[0],
        CornerTables([code, code, CutoutPlan::NONE, CutoutPlan::NONE]),
    );
    assert!(plan.entries().iter().all(|e| *e == ShadowEntry::Tables));
}

/// A partial repaint plans what a full one of the same frame does, from the
/// census of the frames before it. 14 shadows pay for the `(8, 2)` table
/// (see `a_table_is_baked_where_it_is_cheaper_than_shading`). A repaint of
/// the 4 px square at the first shadow's corner draws only that shadow, and
/// would shade a sliver that pays for nothing on its own; the census still
/// holds the other 13, so it reads the table the full repaint does, and
/// bakes it, since a repainted pixel reads it. A repaint far from every
/// shadow draws none and bakes nothing.
#[test]
fn a_partial_repaint_plans_what_a_full_one_does() {
    let viewport = UVec2::new(2000, 200);
    let quads = row(14, 0.0);
    let mut plan = plan_in(&quads, viewport);
    let code = plan.tables()[0].table;
    assert_eq!(plan.corners()[0], CornerTables([code; 4]));
    let sliver = RepaintScissors::partial(&[URect::new(0, 0, 4, 4)]);
    let census = plan.build(&quads[..1], &sliver, viewport);
    assert_eq!(census, Census::Current);
    assert_eq!(plan.corners(), [CornerTables([code; 4])]);
    assert_eq!(plan.entries(), [ShadowEntry::Tables]);
    assert_eq!(plan.tables().len(), 1, "the sliver reads the table");
    let elsewhere = RepaintScissors::partial(&[URect::new(1900, 150, 4, 4)]);
    let census = plan.build(&[], &elsewhere, viewport);
    assert_eq!(census, Census::Current);
    assert!(plan.tables().is_empty(), "no repainted pixel reads it");
    // The census survives the frame that drew no shadow.
    let census = plan.build(&quads[..1], &sliver, viewport);
    assert_eq!(census, Census::Current);
    assert_eq!(plan.corners(), [CornerTables([code; 4])]);
}

/// A partial repaint is stale when a key gains or loses its table under a
/// shadow it does not redraw whole, and only then. 13 shadows shade the
/// `(8, 2)` key; a 14th repainted beside them makes it pay, and the 13 left
/// alone show the shaded form: stale, until a full repaint. Removing it
/// again takes the table away from the 13: stale once more. 14 more shadows
/// of another radius, all repainted, gain a table no shadow left alone
/// reads: current. And a repaint that removes the 14th and covers every
/// other shadow but the first, which it only reaches, leaves that one's
/// left side showing the table it no longer reads: stale, where covering
/// the first as well is current.
#[test]
fn a_partial_repaint_that_moves_a_kept_table_is_stale() {
    let viewport = UVec2::new(4000, 400);
    let mut plan = plan_in(&row(13, 0.0), viewport);
    assert!(plan.tables().is_empty());
    let fourteenth = row(1, 13.0 * 120.0);
    let around = RepaintScissors::partial(&[URect::new(1550, 0, 140, 120)]);
    assert_eq!(plan.build(&fourteenth, &around, viewport), Census::Stale);
    full(&mut plan, &row(14, 0.0), viewport);
    assert_eq!(plan.tables().len(), 1);
    assert_eq!(plan.build(&[], &around, viewport), Census::Stale, "removed");
    full(&mut plan, &row(13, 0.0), viewport);
    assert!(plan.tables().is_empty());
    let others: Vec<Quad> = (0..14)
        .map(|k| {
            let mut quad = shadow(FillKind::SHADOW_DROP, 100.0, 100.0, 12.0, 2.0);
            quad.rect.min = Vec2::new(120.0 * k as f32, 200.0);
            quad
        })
        .collect();
    let below = RepaintScissors::partial(&[URect::new(0, 190, 1800, 120)]);
    assert_eq!(plan.build(&others, &below, viewport), Census::Current);
    assert_eq!(plan.tables().len(), 1, "the new key pays");
    assert!(
        plan.entries().iter().all(|e| *e == ShadowEntry::Tables),
        "{:?}",
        plan.entries()
    );
    for (from, census) in [(50, Census::Stale), (0, Census::Current)] {
        let mut plan = plan_in(&row(14, 0.0), viewport);
        let repaint = RepaintScissors::partial(&[URect::new(from, 0, 1700 - from, 120)]);
        assert_eq!(
            plan.build(&row(13, 0.0), &repaint, viewport),
            census,
            "from {from}"
        );
    }
}

/// The plan hangs on the frame's shadows as a set: the same quads in
/// another order pack every key into the same cell. At σ = 16 the radii
/// 8, 8.0625 and 8.125 all make 87-texel tables, a tie the packer breaks
/// by key rather than by arrival.
#[test]
fn the_plan_does_not_hang_on_quad_order() {
    let quads: Vec<Quad> = [8.0, 8.0625, 8.125]
        .into_iter()
        .map(|r| shadow(FillKind::SHADOW_DROP, 400.0, 300.0, r, 16.0))
        .collect();
    let forward = plan(&quads);
    let mut reversed: Vec<Quad> = quads.clone();
    reversed.reverse();
    let backward = plan(&reversed);
    assert_eq!(forward.tables().len(), 3);
    assert_eq!(forward.tables(), backward.tables());
    let mut flipped = backward.corners().to_vec();
    flipped.reverse();
    assert_eq!(forward.corners(), flipped);
}
