use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_handle::Placement;
use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::{CutoutPlacements, CutoutPlacing};
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{
    BakeTable, Census, CornerTables, CutoutFrame, CutoutKey, CutoutPlan, ShadowBox, ShadowEntry,
    spread_radius,
};
use crate::primitives::geometry::corners::{self, Corners};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::urect::URect;
use crate::primitives::packed::half_simd::F16x4;
use crate::renderer::render_buffer::fill_form::FillForm;
use crate::renderer::render_buffer::quad_draw_row::{QuadDrawRow, QuadInstance, QuadKey};
use crate::renderer::render_buffer::shadow_instance::{ShadowInstance, ShadowKind};
use crate::renderer::render_buffer::shape_header::ShapeHeader;
use crate::renderer::repaint_scissors::RepaintScissors;
use bytemuck::{Pod, Zeroable};
use glam::{U16Vec2, UVec2, Vec2};
use soa_rs::Soa;

/// A drop shadow over a `size` quad, every corner `radius`, blurred by `sigma`, no spread.
fn drop_shadow(size: Size, radius: f32, sigma: f32) -> ShadowInstance {
    ShadowInstance {
        header: ShapeHeader {
            corners: Corners::all(radius),
            ..ShapeHeader::of_box(Rect::new(0.0, 0.0, size.w, size.h))
        },
        kind: ShadowKind::DROP,
        geometry: F16x4::from_lanes([0.0, 0.0, sigma, 0.0]),
        ..ShadowInstance::default()
    }
}

/// [`drop_shadow`], cast inward.
fn inset_shadow(size: Size, radius: f32, sigma: f32) -> ShadowInstance {
    ShadowInstance {
        kind: ShadowKind::INSET,
        ..drop_shadow(size, radius, sigma)
    }
}

/// The quad stream of `shadows`, in order.
fn stream(shadows: &[ShadowInstance]) -> Soa<QuadDrawRow> {
    shadows
        .iter()
        .map(|&shadow| QuadDrawRow {
            key: QuadKey::Shadow,
            instance: QuadInstance::from(shadow),
        })
        .collect()
}

/// A box fill's row, which no plan reads.
fn fill() -> QuadDrawRow {
    QuadDrawRow {
        key: QuadKey::Fill(FillForm::BOX_SOLID),
        instance: QuadInstance::default(),
    }
}

/// A viewport larger than any fixture here, so it hides nothing.
const WIDE: UVec2 = UVec2::new(8192, 8192);

/// The largest texture edge of the device a plan here places for.
const DEVICE_EDGE: u32 = 16384;

/// The code of a table of `side` texels at `(x, y)` of mask page `page`.
fn code_at(page: u8, x: u16, y: u16, side: u32) -> u32 {
    CutoutPlan::code(
        Placement {
            page,
            origin: U16Vec2::new(x, y),
            size: U16Vec2::splat(side as u16),
        },
        side,
    )
}

/// A pool whose mask list holds four pages of 256², none past its soft budget, so it opens a
/// page only once nothing on the others is evictable.
fn small_pool() -> AtlasPool {
    AtlasPool::uniform(256, 256, 0)
}

/// One window's plan, baking, over a backend's placements and pool of its own.
#[derive(Debug)]
struct Planner {
    plan: CutoutPlan,
    placements: CutoutPlacements,
    pool: AtlasPool,
}

impl Planner {
    fn build(
        &mut self,
        quads: &[ShadowInstance],
        repaint: &RepaintScissors,
        viewport: UVec2,
    ) -> Census {
        self.build_rows(&stream(quads), repaint, viewport)
    }

    fn build_rows(
        &mut self,
        quads: &Soa<QuadDrawRow>,
        repaint: &RepaintScissors,
        viewport: UVec2,
    ) -> Census {
        let frame = CutoutFrame {
            quads,
            repaint,
            viewport,
        };
        let mut placing = CutoutPlacing {
            placements: &mut self.placements,
            pool: &mut self.pool,
        };
        self.plan.build(frame, &mut placing)
    }

    fn tables(&self) -> &[BakeTable] {
        self.plan.tables()
    }

    fn corners(&self) -> &[CornerTables] {
        self.plan.corners()
    }

    fn entries(&self) -> &[ShadowEntry] {
        self.plan.entries()
    }
}

/// `quads` planned as a full repaint of a `viewport`, with baking on.
fn plan_in(quads: &[ShadowInstance], viewport: UVec2) -> Planner {
    plan_rows(&stream(quads), viewport)
}

/// The rows `quads` planned as a full repaint of a `viewport`, with baking on.
fn plan_rows(quads: &Soa<QuadDrawRow>, viewport: UVec2) -> Planner {
    plan_rows_over(quads, viewport, AtlasPool::of_edge(DEVICE_EDGE))
}

/// [`plan_rows`] over `pool`.
fn plan_rows_over(quads: &Soa<QuadDrawRow>, viewport: UVec2, pool: AtlasPool) -> Planner {
    let mut plan = Planner {
        plan: CutoutPlan::new(),
        placements: CutoutPlacements::default(),
        pool,
    };
    let census = plan.build_rows(quads, &RepaintScissors::Full, viewport);
    assert_eq!(census, Census::Cur, "a full repaint keeps no pixel");
    plan
}

fn plan(quads: &[ShadowInstance]) -> Planner {
    plan_in(quads, WIDE)
}

/// `plan` rebuilt as a full repaint, which is never stale.
fn full(plan: &mut Planner, quads: &[ShadowInstance], viewport: UVec2) {
    let census = plan.build(quads, &RepaintScissors::Full, viewport);
    assert_eq!(census, Census::Cur, "a full repaint keeps no pixel");
}

/// `plan` rebuilt over the shared `placing` as a full repaint of [`WIDE`], as one of several
/// windows on one backend.
fn full_over(plan: &mut CutoutPlan, placing: &mut CutoutPlacing<'_>, quads: &[ShadowInstance]) {
    let frame = CutoutFrame {
        quads: &stream(quads),
        repaint: &RepaintScissors::Full,
        viewport: WIDE,
    };
    assert_eq!(plan.build(frame, placing), Census::Cur);
}

/// `n` drop shadows 100 px square, radius 8, σ = 2, 120 px apart from `x`.
fn row(n: usize, x: f32) -> Vec<ShadowInstance> {
    (0..n)
        .map(|k| {
            let mut quad = drop_shadow(Size::new(100.0, 100.0), 8.0, 2.0);
            quad.header.rect.min.x = x + 120.0 * k as f32;
            quad
        })
        .collect()
}

/// The radius rules follow the shader. Spread 4 moves a radius at least 4 by 4
/// (8 → 12, 4 → 8, 10 → 14) and pulls a smaller one toward the plain one by
/// `1 + (r / s − 1)³`, so 0 stays 0; a negative spread subtracts, floored at
/// 0. The fit scales by the least side over its radii: a 20×10 box with the
/// left side's radii `12 + 14 = 26` against 10 gives `f = 10 / 26`.
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
        corners::fit([12.0, 0.0, 8.0, 14.0], Size::new(20.0, 10.0)),
        [12.0 * f, 0.0, 8.0 * f, 14.0 * f],
    );
    assert_eq!(
        corners::fit([1.0; 4], Size::new(20.0, 10.0)),
        [1.0; 4],
        "room to spare"
    );
}

/// One `shadow_box` call, as `vs_shadow` makes it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ShadowBoxIn {
    radius: [f32; 4],
    geometry: [f32; 4],
    half: [f32; 2],
    kind: u32,
    pad: u32,
}

/// What `shadow_box` returned.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ShadowBoxOut {
    radius: [f32; 4],
    center: [f32; 2],
    half: [f32; 2],
}

const SHADOW_BOX_PROBE: &str = "
struct ShadowBoxIn { radius: vec4<f32>, geometry: vec4<f32>, half: vec2<f32>, kind: u32, pad: u32 }
struct ShadowBoxOut { radius: vec4<f32>, center: vec2<f32>, half: vec2<f32> }
@group(0) @binding(14) var<storage, read> probe_in: array<ShadowBoxIn>;
@group(0) @binding(15) var<storage, read_write> probe_out: array<ShadowBoxOut>;
@compute @workgroup_size(1)
fn probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let q = probe_in[id.x];
    let b = shadow_box(q.kind, q.half, q.radius, q.geometry);
    probe_out[id.x] = ShadowBoxOut(b.radius, b.center, b.half);
}
";

/// The cutout plan sizes its tables from the box the shader blurs, so the shader's
/// `shadow_box` and [`ShadowBox`] agree on every arm of the spread rule: a radius at least
/// the spread, one below it, a sharp corner, a zero and a negative spread, radii the fit
/// scales, and a box the spread collapses. Within 4 ULP, as WGSL allows `/` 2.5 ULP and
/// a fused multiply-add.
#[test]
fn shadow_box_follows_the_shader() {
    let shadow = |kind: ShadowKind, size: Size, radii: [f32; 4], axis: [f32; 4]| ShadowInstance {
        header: ShapeHeader {
            corners: Corners::new(radii[0], radii[1], radii[2], radii[3]),
            ..ShapeHeader::of_box(Rect::new(0.0, 0.0, size.w, size.h))
        },
        kind,
        geometry: F16x4::from_lanes(axis),
        ..ShadowInstance::default()
    };
    let (drop, inset) = (ShadowKind::DROP, ShadowKind::INSET);
    let quads = [
        shadow(
            drop,
            Size::new(100.0, 60.0),
            [8.0, 0.0, 4.0, 10.0],
            [0.0, 0.0, 2.0, 4.0],
        ),
        shadow(
            drop,
            Size::new(100.0, 60.0),
            [3.0, 5.0, 12.0, 0.0],
            [0.0, 0.0, 2.0, 6.0],
        ),
        shadow(
            drop,
            Size::new(100.0, 60.0),
            [8.0, 0.0, 4.0, 10.0],
            [0.0, 0.0, 2.0, -3.0],
        ),
        shadow(
            drop,
            Size::new(100.0, 60.0),
            [8.0, 0.0, 4.0, 10.0],
            [0.0, 0.0, 2.0, 0.0],
        ),
        shadow(
            drop,
            Size::new(80.0, 50.0),
            [30.0, 30.0, 20.0, 40.0],
            [0.0, 0.0, 1.5, 2.0],
        ),
        shadow(
            drop,
            Size::new(20.0, 20.0),
            [4.0; 4],
            [0.0, 0.0, 1.0, -12.0],
        ),
        shadow(
            inset,
            Size::new(100.0, 60.0),
            [8.0, 0.0, 4.0, 10.0],
            [5.0, -3.0, 2.0, 4.0],
        ),
        shadow(
            inset,
            Size::new(100.0, 60.0),
            [8.0, 0.0, 4.0, 10.0],
            [0.0, 2.5, 3.0, -2.0],
        ),
        shadow(
            inset,
            Size::new(60.0, 60.0),
            [8.0; 4],
            [1.0, 1.0, 2.0, 40.0],
        ),
    ];
    let inputs: Vec<ShadowBoxIn> = quads
        .iter()
        .map(|quad| ShadowBoxIn {
            radius: quad.header.corners.to_array(),
            geometry: quad.geometry.lanes(),
            half: [quad.header.rect.size.w * 0.5, quad.header.rect.size.h * 0.5],
            kind: quad.kind.bits(),
            pad: 0,
        })
        .collect();
    let gpu: Vec<ShadowBoxOut> = QuadPipeline::SHADER.probe(SHADOW_BOX_PROBE, &inputs);
    let close = |a: f32, b: f32| (a - b).abs() <= 4.0 * f32::EPSILON * a.abs().max(b.abs());
    for (at, (quad, gpu)) in quads.iter().zip(&gpu).enumerate() {
        let cpu = ShadowBox::of(quad);
        let cpu_lanes = cpu
            .radius
            .into_iter()
            .chain(cpu.center.to_array())
            .chain(cpu.half.to_array());
        let gpu_lanes = gpu.radius.into_iter().chain(gpu.center).chain(gpu.half);
        for (lane, (c, g)) in cpu_lanes.zip(gpu_lanes).enumerate() {
            assert!(
                close(c, g),
                "quad {at} lane {lane}: {cpu:?} against {gpu:?}"
            );
        }
    }
}

/// A 400×300 drop shadow blurred by 16, radius 8: its box is the source inset
/// by `4σ = 64`, so all four corners share the key `(8, 16)`. `reach = 64.5`;
/// the side is `floor((8 + 129) · 10 / 16) + 2 = 87` texels, at cell `(0, 0)`.
/// An inset shadow with the same key and a second drop shadow reuse the table,
/// so the frame bakes one, whole: the first shadow's top-left corner reads
/// `(72 − p + 64.5) · 10 / 16` from 0 to 85.3, plus one texel past that and one
/// for rounding. Shadows draw through the wide tables entry (σ = 16 is past
/// [`CutoutPlan::SERIES_MIN_SIGMA`]), the quad between them through the
/// general one.
#[test]
fn equal_corners_share_one_table() {
    let mut quads = stream(&[
        drop_shadow(Size::new(400.0, 300.0), 8.0, 16.0),
        inset_shadow(Size::new(200.0, 200.0), 8.0, 16.0),
        drop_shadow(Size::new(500.0, 300.0), 8.0, 16.0),
    ]);
    quads.insert(1, fill());
    let plan = plan_rows(&quads, WIDE);
    let code = code_at(0, 0, 0, 87);
    assert_eq!(
        plan.tables(),
        [BakeTable {
            table: code,
            r: 8.0,
            sigma: 16.0,
            lo: U16Vec2::new(0, 0),
            hi: U16Vec2::new(87, 87),
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
            ShadowEntry::TablesWide,
            ShadowEntry::General,
            ShadowEntry::TablesWide,
            ShadowEntry::TablesWide,
        ],
    );
}

/// A tabled shadow's entry follows its blur: below
/// [`CutoutPlan::SERIES_MIN_SIGMA`] edges take `filter_cdf`'s difference form,
/// from it up the series. 8 shadows rounded 8 pay for their table either side:
/// at σ = 3.75 each shows `4 · 39²` px of corner regions, 73 008 nodes to
/// shade against `106² · 24 = 269 664` to bake; at σ = 4, `4 · 41²` px, 80 688
/// against `104² · 24 = 259 584`. Both keys bake; only the threshold tells
/// them apart.
#[test]
fn the_tables_entry_follows_the_blur() {
    let mut quads = Vec::new();
    for sigma in [3.75, 4.0] {
        quads.extend((0..8).map(|_| drop_shadow(Size::new(400.0, 300.0), 8.0, sigma)));
    }
    let plan = plan(&quads);
    assert_eq!(plan.tables().len(), 2);
    let mut want = vec![ShadowEntry::Tables; 8];
    want.extend([ShadowEntry::TablesWide; 8]);
    assert_eq!(plan.entries(), want);
}

/// A corner keeps the shaded cutout when its blur is below the cutout form,
/// it has no radius to cut, its table would pass the side budget, or baking is
/// off. A frame with no shadow plans nothing. A shadow with no radius still
/// draws through the tables entry, the rest through the general one.
#[test]
fn corners_without_a_table_keep_the_shaded_cutout() {
    let none = CornerTables([CutoutPlan::NONE; 4]);
    for (label, quad, entry) in [
        (
            "blur below the cutout form",
            drop_shadow(Size::new(400.0, 300.0), 8.0, 0.2),
            ShadowEntry::General,
        ),
        (
            "no radius",
            drop_shadow(Size::new(400.0, 300.0), 0.0, 16.0),
            ShadowEntry::TablesWide,
        ),
        // σ = 0.25 against a radius of 40: (40 + 3) · 40 + 2 = 1722 texels.
        (
            "past the side budget",
            inset_shadow(Size::new(400.0, 300.0), 40.0, 0.25),
            ShadowEntry::General,
        ),
    ] {
        let plan = plan(&[quad]);
        assert!(plan.tables().is_empty(), "{label}");
        assert_eq!(plan.corners(), [none], "{label}");
        assert_eq!(plan.entries(), [entry], "{label}");
    }
    let mut off = CutoutPlan::shading();
    let census = off.build(
        CutoutFrame {
            quads: &stream(&[drop_shadow(Size::new(400.0, 300.0), 8.0, 16.0)]),
            repaint: &RepaintScissors::Full,
            viewport: WIDE,
        },
        &mut CutoutPlacing {
            placements: &mut CutoutPlacements::default(),
            pool: &mut AtlasPool::of_edge(DEVICE_EDGE),
        },
    );
    assert_eq!(census, Census::Cur);
    assert!(off.tables().is_empty());
    assert_eq!(off.corners(), [none], "baking off");
    assert_eq!(off.entries(), [ShadowEntry::General], "baking off");
    let no_shadow = plan_rows(&[fill()].into_iter().collect(), WIDE);
    assert!(no_shadow.corners().is_empty(), "no shadow");
    assert!(no_shadow.entries().is_empty(), "no shadow");
}

/// Tables take the mask pages until the pool holds no more, and what is left keeps the shaded
/// cutout. At σ = 16 a radius from 7 to 8.5 makes an 87-texel table, `floor((r + 129) · 10 /
/// 16) + 2`, so 25 keys 1/16 apart share one side and pack by key. A 256² page opens shelves 88
/// high, rounded to four texels: two shelves, at rows 0 and 88, the third left 80 rows; each
/// holds two tables, at columns 0 and 87, the third past 256. Four tables a page, four pages
/// pinned whole, so keys 0 to 15 land in key order, page by page, and the 9 past them, 16 to 24,
/// keep the shaded cutout. Tables list in key order.
#[test]
fn tables_take_the_pages_until_the_pool_is_full() {
    // Radii 1/16 apart, which `f16` holds exactly between 4 and 8.
    let quads: Vec<ShadowInstance> = (0..25)
        .map(|k| drop_shadow(Size::new(400.0, 300.0), 7.0 + k as f32 / 16.0, 16.0))
        .collect();
    let plan = plan_rows_over(&stream(&quads), WIDE, small_pool());
    let slots = [(0, 0), (87, 0), (0, 88), (87, 88)];
    let placed: Vec<u32> = (0..16)
        .map(|k: usize| {
            let (x, y) = slots[k % 4];
            code_at((k / 4) as u8, x, y, 87)
        })
        .collect();
    assert_eq!(
        plan.tables()
            .iter()
            .map(|table| table.table)
            .collect::<Vec<_>>(),
        placed
    );
    for (k, corners) in plan.corners().iter().enumerate() {
        let want = placed.get(k).copied().unwrap_or(CutoutPlan::NONE);
        assert_eq!(*corners, CornerTables([want; 4]), "key {k}");
    }
}

/// A table keeps its place while its key is in use, across frames and across windows sharing
/// the pool. At σ = 16 a side is `floor((r + 129) · 10 / 16) + 2`: radius 8 makes an 87-texel
/// table, 14 a 91-texel one and 11 an 89-texel one. Planned alone, radius 8 takes the origin of
/// the first page; a later frame that adds radius 14 leaves it there, and 14 opens the shelf
/// below, 92 rows from row 88. A second window
/// with radius 11 lands beside 14 on that shelf, which takes it within a quarter of its height,
/// and the first window's next frame finds its tables where they were. Once the first window
/// closes, it gives back each key it held, and its keys keep their places for the next window to
/// use.
#[test]
fn a_table_keeps_its_place_across_frames_and_windows() {
    let shadow = |r: f32| drop_shadow(Size::new(400.0, 300.0), r, 16.0);
    let mut placements = CutoutPlacements::default();
    let mut pool = AtlasPool::of_edge(DEVICE_EDGE);
    let mut placing = CutoutPlacing {
        placements: &mut placements,
        pool: &mut pool,
    };
    let mut first = CutoutPlan::new();
    let code = |plan: &CutoutPlan, at: usize| plan.corners()[at].0[0];
    full_over(&mut first, &mut placing, &[shadow(8.0)]);
    let small = code(&first, 0);
    assert_eq!(small, code_at(0, 0, 0, 87));

    full_over(&mut first, &mut placing, &[shadow(8.0), shadow(14.0)]);
    assert_eq!(code(&first, 0), small, "the taller key moves no table");
    let tall = code(&first, 1);
    assert_eq!(tall, code_at(0, 0, 88, 91));

    let mut second = CutoutPlan::new();
    full_over(&mut second, &mut placing, &[shadow(11.0)]);
    let other = code(&second, 0);
    assert_eq!(other, code_at(0, 91, 88, 89), "a place of its own");
    full_over(&mut first, &mut placing, &[shadow(8.0), shadow(14.0)]);
    assert_eq!([code(&first, 0), code(&first, 1)], [small, tall]);

    let [r8, r14] = [8.0, 14.0].map(|r| CutoutKey::new(r, 16.0));
    assert_eq!([placing.users(r8), placing.users(r14)], [1, 1]);
    first.release_all(&mut placing);
    assert_eq!(
        [placing.users(r8), placing.users(r14)],
        [0, 0],
        "a closing window gives back each key it held"
    );
    full_over(&mut second, &mut placing, &[shadow(11.0), shadow(8.0)]);
    assert_eq!(placing.users(r8), 1, "the second window's use alone");
    assert_eq!(
        [code(&second, 0), code(&second, 1)],
        [other, small],
        "a closed window's key keeps its place"
    );
}

/// A table too tall for what the pool has left does not stop a shorter one after it. At σ = 16 a
/// side is `floor((r + 129) · 10 / 16) + 2` texels: radius 130 makes 163 and 130.25 to 131 make
/// 164, all on shelves 164 high, so a 256² page takes one, its shelf too narrow for a second, and
/// 92 rows left below. The four 164s go first and take the four pages; the 163 finds none and is
/// left out; radius 8 makes 87 texels, on a shelf 88 high, and lands on the first page's open
/// rows at 164. The quads are 1000
/// px square, so no radius is fitted down to the box `4σ` inside them.
#[test]
fn a_shorter_table_takes_the_room_a_taller_one_missed() {
    let mut quads: Vec<ShadowInstance> = (0..5)
        .map(|k| drop_shadow(Size::new(1000.0, 1000.0), 130.0 + k as f32 / 4.0, 16.0))
        .collect();
    quads.push(drop_shadow(Size::new(1000.0, 1000.0), 8.0, 16.0));
    let plan = plan_rows_over(&stream(&quads), WIDE, small_pool());
    let tall: Vec<u32> = plan.corners()[..5].iter().map(|c| c.0[0]).collect();
    assert_eq!(
        tall,
        [
            CutoutPlan::NONE,
            code_at(0, 0, 0, 164),
            code_at(1, 0, 0, 164),
            code_at(2, 0, 0, 164),
            code_at(3, 0, 0, 164),
        ],
        "one tall table a page, the last placed past the pool"
    );
    assert_eq!(
        plan.corners()[5].0[0],
        code_at(0, 0, 164, 87),
        "the short table on the open rows"
    );
}

/// A table is baked only where cheaper than shading the corners sharing it.
/// At σ = 2, r = 8 a 100 px drop shadow's box is inset by `4σ = 8`, so the
/// top-left corner's region runs from `8 − 8.5 = −0.5` to `8 + 8 + 8.5 = 24.5`
/// per axis; the quad starts at 0, so 24.5 px show: 600.25 px² at 12 nodes,
/// 7 203 per corner. The table is `25 · 5 + 2 = 127` texels square at 24
/// nodes, 387 096. So 53 corners shade cheaper and 54 do not: 13 shadows keep
/// the shaded cutout, 14 get the table. A region the viewport does not show
/// costs nothing, so an off-screen shadow adds nothing.
///
/// The entry follows the shown corners: shaded ones use the general entry, and
/// a shadow whose untabled corners are not shown uses the tables one.
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
    // 13 shadows on screen and 1 past its right edge: still not worth a table.
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
    // A 60 px viewport: the right corners' regions start at
    // `100 − 8 − 8 − 8.5 = 75.5`, so none is shown and the left shaded corners decide.
    let plan = plan_in(&row(1, 0.0), UVec2::new(60, 100));
    assert_eq!(plan.entries(), [ShadowEntry::General]);
    // 14 shadows at x = −80, each showing its last 20 px: left corners'
    // regions end at `−80 + 24.5`, off screen, and two shown corners each do
    // not pay for a table.
    let quads: Vec<ShadowInstance> = row(14, 0.0)
        .into_iter()
        .map(|quad| ShadowInstance {
            header: ShapeHeader {
                rect: Rect::new(-80.0, 0.0, 100.0, 100.0),
                ..quad.header
            },
            ..quad
        })
        .collect();
    let plan = plan_in(&quads, UVec2::new(2000, 200));
    assert!(plan.tables().is_empty());
    assert!(plan.entries().iter().all(|e| *e == ShadowEntry::General));
    // A 50 px viewport over 28 shadows rounded 8 on top, 4 below: 56 shown top
    // corners pay for the `(8, 2)` table; the bottom regions start at
    // `92 − 4 − 8.5 = 79.5`, so `(4, 2)` gets none and every shadow still reads tables.
    let quads: Vec<ShadowInstance> = row(28, 0.0)
        .into_iter()
        .map(|quad| ShadowInstance {
            header: ShapeHeader {
                corners: Corners::new(8.0, 8.0, 4.0, 4.0),
                ..quad.header
            },
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
/// census of the frames before it. 14 shadows pay for the `(8, 2)` table (see
/// `a_table_is_baked_where_it_is_cheaper_than_shading`). A repaint of the 4 px
/// square at the first shadow's corner draws only that shadow, and alone would
/// shade a sliver; the census still holds the other 13, so it reads the table
/// and bakes only what the sliver reads: the top-left arc is centered at 16 per
/// axis, so it reads `(16 − p + 8.5) · 5` from 102.5 to 122.5, texels 101 to
/// 124 with one past and one each side for rounding, where the whole frame
/// reads 0 to 124 of 127. A repaint far from every shadow bakes nothing.
#[test]
fn a_partial_repaint_plans_what_a_full_one_does() {
    let viewport = UVec2::new(2000, 200);
    let quads = row(14, 0.0);
    let mut plan = plan_in(&quads, viewport);
    let code = plan.tables()[0].table;
    assert_eq!(plan.corners()[0], CornerTables([code; 4]));
    assert_eq!(
        (plan.tables()[0].lo, plan.tables()[0].hi),
        (U16Vec2::new(0, 0), U16Vec2::new(125, 125))
    );
    let sliver = RepaintScissors::partial(&[URect::new(0, 0, 4, 4)]);
    let census = plan.build(&quads[..1], &sliver, viewport);
    assert_eq!(census, Census::Cur);
    assert_eq!(plan.corners(), [CornerTables([code; 4])]);
    assert_eq!(plan.entries(), [ShadowEntry::Tables]);
    assert_eq!(plan.tables().len(), 1, "the sliver reads the table");
    assert_eq!(
        (plan.tables()[0].lo, plan.tables()[0].hi),
        (U16Vec2::new(101, 101), U16Vec2::new(125, 125)),
        "the sliver's texels",
    );
    let elsewhere = RepaintScissors::partial(&[URect::new(1900, 150, 4, 4)]);
    let census = plan.build(&[], &elsewhere, viewport);
    assert_eq!(census, Census::Cur);
    assert!(plan.tables().is_empty(), "no repainted pixel reads it");
    // The census survives the frame that drew no shadow.
    let census = plan.build(&quads[..1], &sliver, viewport);
    assert_eq!(census, Census::Cur);
    assert_eq!(plan.corners(), [CornerTables([code; 4])]);
}

/// A repaint bakes the texels its pixels read past a corner's region, where
/// bilinear filtering still reaches the table. An inset shadow 200 px square,
/// radius 7.25, σ = 16, its hole moved 100 right to x 100 to 300: the top-left
/// arc is centered at `(107.25, 7.25)`, `reach = 64.5`, the region runs to
/// `q = 72.5`, x = 35.5. The table has `floor((7.25 + 129) · 0.625) + 2 = 87`
/// texels, read up to `t = 86`, i.e. `q = 86 / 0.625 − 64.5 = 73.1`, x = 34.15.
/// The two left corners show `136.25 · 71.75` px each, 234.6k nodes to shade
/// against 181.7k to bake: the key pays. A repaint of x 0 to 35 misses the
/// region, yet its pixel centered at 34.5 reads `t = (72.75 + 64.5) · 0.625 =
/// 85.78`, texels 85 and 86. So it bakes x from `floor((72.25 + 64.5) · 0.625)
/// − 1 = 84` to the side, and y from 0 to `floor((7.25 + 64.5) · 0.625) + 3 = 47`.
#[test]
fn a_repaint_past_a_corners_region_bakes_what_it_reads() {
    let mut quad = inset_shadow(Size::new(200.0, 200.0), 7.25, 16.0);
    quad.geometry = F16x4::from_lanes([100.0, 0.0, 16.0, 0.0]);
    let mut plan = plan(&[quad]);
    let code = code_at(0, 0, 0, 87);
    assert_eq!(plan.corners(), [CornerTables([code; 4])]);
    let band = RepaintScissors::partial(&[URect::new(0, 0, 35, 72)]);
    assert_eq!(plan.build(&[quad], &band, WIDE), Census::Cur);
    assert_eq!(
        plan.tables(),
        [BakeTable {
            table: code,
            r: 7.25,
            sigma: 16.0,
            lo: U16Vec2::new(84, 0),
            hi: U16Vec2::new(87, 47),
        }],
    );
}

/// A partial repaint is stale when a key gains or loses its table under a
/// shadow it does not redraw whole, and only then. 13 shadows shade the
/// `(8, 2)` key; a 14th repainted beside them makes it pay, leaving the 13
/// showing the shaded form: stale until a full repaint. Removing it takes the
/// table from the 13: stale again. 14 more shadows of another radius, all
/// repainted, gain a table no shadow left alone reads: current. A repaint
/// removing the 14th and covering every other shadow but the first, which it
/// only reaches, leaves that one's left side showing a table it no longer
/// reads: stale; covering the first too is current.
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
    let others: Vec<ShadowInstance> = (0..14)
        .map(|k| {
            let mut quad = drop_shadow(Size::new(100.0, 100.0), 12.0, 2.0);
            quad.header.rect.min = Vec2::new(120.0 * k as f32, 200.0);
            quad
        })
        .collect();
    let below = RepaintScissors::partial(&[URect::new(0, 190, 1800, 120)]);
    assert_eq!(plan.build(&others, &below, viewport), Census::Cur);
    assert_eq!(plan.tables().len(), 1, "the new key pays");
    assert!(
        plan.entries().iter().all(|e| *e == ShadowEntry::Tables),
        "{:?}",
        plan.entries()
    );
    // Two disjoint scissors meeting across the first shadow cover it as one does.
    for (what, rects, census) in [
        (
            "from 50",
            &[URect::new(50, 0, 1650, 120)][..],
            Census::Stale,
        ),
        ("from 0", &[URect::new(0, 0, 1700, 120)], Census::Cur),
        (
            "two meeting",
            &[URect::new(0, 0, 25, 120), URect::new(25, 0, 1675, 120)],
            Census::Cur,
        ),
    ] {
        let mut plan = plan_in(&row(14, 0.0), viewport);
        let repaint = RepaintScissors::partial(rects);
        assert_eq!(
            plan.build(&row(13, 0.0), &repaint, viewport),
            census,
            "{what}"
        );
    }
}

/// The plan hangs on the frame's shadows as a set: the same quads in another
/// order pack every key into the same cell. At σ = 16 the radii 8, 8.0625 and
/// 8.125 all make 87-texel tables, a tie the packer breaks by key, not arrival.
#[test]
fn the_plan_does_not_hang_on_quad_order() {
    let quads: Vec<ShadowInstance> = [8.0, 8.0625, 8.125]
        .into_iter()
        .map(|r| drop_shadow(Size::new(400.0, 300.0), r, 16.0))
        .collect();
    let forward = plan(&quads);
    let mut reversed: Vec<ShadowInstance> = quads;
    reversed.reverse();
    let backward = plan(&reversed);
    assert_eq!(forward.tables().len(), 3);
    assert_eq!(forward.tables(), backward.tables());
    let mut flipped = backward.corners().to_vec();
    flipped.reverse();
    assert_eq!(forward.corners(), flipped);
}
