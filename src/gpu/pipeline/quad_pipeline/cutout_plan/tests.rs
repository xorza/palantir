use crate::gpu::pipeline::quad_pipeline::cutout_plan::{
    BakeTable, CornerTables, CutoutPlan, fit_radii, spread_radius,
};
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::renderer::quad::Quad;
use glam::Vec2;

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

fn plan(quads: &[Quad]) -> CutoutPlan {
    let mut plan = CutoutPlan::default();
    plan.build(quads, true, |region| region.size.w * region.size.h);
    plan
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
/// drop shadow, so the frame bakes one.
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
}

/// A corner keeps the shaded cutout when its blur is below the cutout form,
/// when it has no radius to cut, when its table would pass the side budget,
/// and when baking is off. A frame with no shadow plans nothing at all.
#[test]
fn corners_without_a_table_keep_the_shaded_cutout() {
    let none = CornerTables([CutoutPlan::NONE; 4]);
    for (label, quad) in [
        (
            "blur below the cutout form",
            shadow(FillKind::SHADOW_DROP, 400.0, 300.0, 8.0, 0.2),
        ),
        (
            "no radius",
            shadow(FillKind::SHADOW_DROP, 400.0, 300.0, 0.0, 16.0),
        ),
        // σ = 0.25 against a radius of 40: (40 + 3) · 40 + 2 = 1722 texels.
        (
            "past the side budget",
            shadow(FillKind::SHADOW_INSET, 400.0, 300.0, 40.0, 0.25),
        ),
    ] {
        let plan = plan(&[quad]);
        assert!(plan.tables().is_empty(), "{label}");
        assert_eq!(plan.corners(), [none], "{label}");
    }
    let mut off = CutoutPlan::default();
    off.build(
        &[shadow(FillKind::SHADOW_DROP, 400.0, 300.0, 8.0, 16.0)],
        false,
        |region| region.size.w * region.size.h,
    );
    assert!(off.tables().is_empty());
    assert_eq!(off.corners(), [none], "baking off");
    assert!(plan(&[Quad::default()]).corners().is_empty(), "no shadow");
}

/// Tables pack tallest first along a shelf of cells, start a new shelf when
/// a row is full, and stop when the atlas is: what is left keeps the shaded
/// cutout. At σ = 16 a radius from 8 to 14.25 makes 87 to 91 texels, 6
/// cells of 16 either way, so a 64-cell row holds 10 tables. 100 keys fill
/// 10 shelves of 10, and key 101 is left out.
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
    let cells: Vec<[u32; 2]> = plan
        .tables()
        .iter()
        .map(|table| [table.table & 63, (table.table >> 6) & 63])
        .collect();
    let want: Vec<[u32; 2]> = (0..10)
        .flat_map(|y| (0..10).map(move |x| [x * 6, y * 6]))
        .collect();
    assert_eq!(cells, want);
    let unplaced = plan
        .corners()
        .iter()
        .filter(|corners| **corners == CornerTables([CutoutPlan::NONE; 4]))
        .count();
    assert_eq!(unplaced, 1, "the one key past the atlas");
}

/// A table is baked only where it is cheaper than shading the corners that
/// share it. At σ = 2 and r = 8 a 100 px drop shadow's box is inset by
/// `4σ = 8`, so the top-left corner's region runs from `8 − 8.5 = −0.5` to
/// `8 + 8 + 8.5 = 24.5` on each axis; the quad starts at 0, so 24.5 px of
/// it are drawn, 600.25 px² at 12 nodes, 7 203 per corner. Its table is
/// `25 · 5 + 2 = 127` texels square at 24 nodes, 387 096. So 53 corners
/// shade cheaper and 54 do not: 13 shadows of 4 corners keep the shaded
/// cutout, 14 get the table. A region the frame does not draw costs
/// nothing to shade, so a shadow outside the damage adds nothing.
#[test]
fn a_table_is_baked_where_it_is_cheaper_than_shading() {
    let full = |region: Rect| region.size.w * region.size.h;
    let shadows = |n: usize, x: f32| -> Vec<Quad> {
        (0..n)
            .map(|_| {
                let mut quad = shadow(FillKind::SHADOW_DROP, 100.0, 100.0, 8.0, 2.0);
                quad.rect.min.x = x;
                quad
            })
            .collect()
    };
    for (n, baked) in [(13, false), (14, true)] {
        let mut plan = CutoutPlan::default();
        plan.build(&shadows(n, 0.0), true, full);
        assert_eq!(plan.tables().len(), usize::from(baked), "{n} shadows");
    }
    // 13 shadows on screen and 1 off it: still not worth a table.
    let mut quads = shadows(13, 0.0);
    quads.extend(shadows(1, 5000.0));
    let mut plan = CutoutPlan::default();
    plan.build(&quads, true, |region| {
        if region.min.x < 1000.0 {
            full(region)
        } else {
            0.0
        }
    });
    assert!(plan.tables().is_empty());
    assert!(
        plan.corners()
            .iter()
            .all(|corners| *corners == CornerTables([CutoutPlan::NONE; 4]))
    );
}
