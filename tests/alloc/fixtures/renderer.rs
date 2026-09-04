//! Fixtures that push shape *count* and shape *variety* through a whole
//! frame, where the other widget fixtures push structure.
//!
//! **These are the only fixtures that render.** `Audit::run` drives a
//! `UiHarness`, and `Ui::frame` stops at damage — encode and compose live
//! behind a `Frontend`, which needs a device. So these four go through
//! [`OffscreenTarget`] instead, and what they cover is everything a shape
//! costs from `add_shape` to submit: the record store's per-frame copies
//! (a polyline's points, a mesh's vertex and index bytes), the tree's
//! shape arena, the cascade's paint rows, the encoder's per-shape
//! command, the composer's scratch, and the icon registry's re-load.
//!
//! **The budget is the driver, and that is the price of the coverage.**
//! Every wgpu submission allocates a `CommandEncoder` Arc, a
//! `CommandBuffer` Arc, the queue's in-flight `Vec` push, and per-pass
//! scratch from `wgpu_hal`. Measured, all four fixtures land within five
//! blocks of each other — the shape kind contributes nothing, which is
//! the result, and the number is the floor for a frame that actually
//! draws. So these read a *ceiling* where they used to read a strict
//! zero: a regression of one allocation per frame now hides under the
//! driver's own frame-to-frame spread, and only one that scales with the
//! shape count moves the number. `fixtures/widgets.rs` and
//! `fixtures/churn.rs` still hold the strict-zero half of the suite, and
//! `gates::offscreen_frame_stays_at_driver_floor` pins this floor on a
//! still tree so a drift in the driver reads there first.

use crate::harness::{Audit, OffscreenTarget, SURFACE};
use palantir::internals::headless_test_gpu;
use palantir::{
    Configure, Frame, Grid, IconId, IconSet, IconTable, Mesh, Panel, PolylineColors, RgbaF32,
    Shape, Sizing, Track, TranslateScale, Ui,
};
use std::rc::Rc;

/// Frames each fixture warms and then measures. Sixteen is the margin the
/// gates use for the same host; sixty-four is long enough for a
/// once-every-N-frames allocation to land inside the window.
const WARMUP: usize = 16;
const FRAMES: usize = 64;

/// Distinct positions the nudge cycles through. More than one so damage
/// is full on every frame, few enough that the tree never walks off the
/// surface and starts being culled instead of drawn.
const NUDGE_POSITIONS: u32 = 4;

/// Per-frame ceiling every fixture here shares, against measured worst
/// frames of 67 to 72 and means of 64 to 67.
///
/// One constant rather than four because the four measurements sit within
/// five blocks of each other: what they count is the submission, and the
/// shape kind riding on it costs nothing. Four numbers would read as four
/// findings and be one.
const FRONTEND_BLOCKS_PER_FRAME_MAX: u64 = 90;

/// Offscreen frames of `scene`, nudged one pixel sideways each frame so
/// every shape in it is re-encoded.
///
/// **The nudge is what makes this an audit of the renderer at all.** The
/// offscreen host keeps its damage baseline across calls, since the
/// target key is the texture's size and format rather than its identity —
/// so a still tree damages nothing after its first frames, and encode and
/// compose would walk an empty region while the shapes below never reach
/// the paths these fixtures exist to pin.
///
/// A moving transform is the cheapest change that damages the whole
/// subtree, but it is not free of the record side: the panel row folds
/// the transform into its node hash, so the measure cache misses and
/// layout re-runs with it. These numbers are therefore whole-frame
/// numbers, not frontend-only ones.
///
/// `#[track_caller]` so a blown budget still names the fixture rather
/// than this line.
#[track_caller]
fn frontend_audit(label: &str, mut scene: impl FnMut(&mut Ui)) {
    let gpu = headless_test_gpu();
    let mut target = OffscreenTarget::new(&gpu, label, SURFACE);
    let mut step = 0u32;
    Audit::new()
        .warmup(WARMUP)
        .frames(FRAMES)
        .budget(FRONTEND_BLOCKS_PER_FRAME_MAX)
        .run_frames(|| {
            step = (step + 1) % NUDGE_POSITIONS;
            target.frame(&gpu, 1.0, |ui| {
                Panel::zstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .transform(TranslateScale::from_translation(glam::Vec2::new(
                        step as f32,
                        0.0,
                    )))
                    .show(ui, |ui| scene(ui));
            });
        });
}

/// 16×16 grid of `Frame`s — 256 quads, re-encoded every frame. Stresses
/// `RenderCmdBuffer` and `RenderBuffer.quads` capacity reuse much harder
/// than `grid_8x8` (64 quads). A capacity-doubling regression in the
/// encoder shape vec or the composer quad vec shows up here.
#[test]
fn many_rects_compose_alloc_free() {
    frontend_audit("palantir.alloc.many_rects", |ui| {
        Grid::new()
            .auto_id()
            .cols([Track::FILL; 16])
            .rows([Track::FILL; 16])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..16u16 {
                    for c in 0..16u16 {
                        Frame::new()
                            .id_salt((r, c))
                            .background(palantir::Background {
                                fill: RgbaF32::WHITE.into(),
                                ..Default::default()
                            })
                            .grid_cell((r, c))
                            .show(ui);
                    }
                }
            });
    });
}

/// Static polyline pushed every frame. Slice borrows are copied into the
/// window's record store at `add_shape` time, so the closure can hold the
/// `Vec` and hand `&points[..]` to the shape variant. Pins the composer's
/// polyline point / index / direction scratch reuse, which nothing else
/// in the suite reaches.
#[test]
fn polyline_static_alloc_free() {
    let points: Vec<glam::Vec2> = (0..32)
        .map(|i| glam::Vec2::new(i as f32 * 20.0, 100.0 + (i as f32).sin() * 30.0))
        .collect();
    frontend_audit("palantir.alloc.polyline", move |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::polyline(
                    &points,
                    PolylineColors::Single(RgbaF32::WHITE),
                    2.0,
                ));
            });
    });
}

/// Static `Mesh` pushed every frame via `Ui::add_shape`. Vertex / index
/// bytes are copied into the tree's mesh arena at `add_shape` time, so
/// the mesh built once outside the closure is reused as-is. Pins that the
/// mesh-encoding command path doesn't allocate at steady state.
#[test]
fn mesh_static_alloc_free() {
    let mesh = {
        let mut m = Mesh::with_capacity(3, 3);
        let a = m.vertex(glam::Vec2::new(0.0, 0.0), RgbaF32::WHITE);
        let b = m.vertex(glam::Vec2::new(100.0, 0.0), RgbaF32::WHITE);
        let c = m.vertex(glam::Vec2::new(50.0, 100.0), RgbaF32::WHITE);
        m.triangle(a, b, c);
        m
    };
    frontend_audit("palantir.alloc.mesh", move |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::mesh(&mesh));
            });
    });
}

/// 200 icons per frame. Every one goes record → encode → compose, so the
/// per-frame cost is a push onto `RenderBuffer.icons` and nothing else:
/// the raster and its atlas slot are resolved once, on the frame that
/// first drew the icon, and re-found by a map probe thereafter. The
/// nudge does not change that — an icon's raster key is its physical
/// box, which a translate leaves alone.
///
/// The set is re-loaded inside the scene, which is the shape an
/// immediate-mode caller writes — so this also pins that re-loading a set
/// every frame is a refcount bump. `IconRegistry::register` finds the live
/// `IconSet` over that allocation and hands back a clone of it; a
/// regression that took a second slot would show up here before it showed
/// up as unbounded growth.
///
/// The set is *parked* across frames, which is the contract: an `IconSet`
/// owns its parses and its atlas rasters, so a scene that dropped the one
/// it loaded would unload them at every submit and rasterize afresh at
/// every frame. Keeping it is what the `#[must_use]` on `load_icons` is
/// telling the caller to do.
#[test]
fn many_icons_compose_alloc_free() {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" rx="3" fill="#fff"/></svg>"##;
    let atlas = Rc::new(IconTable::from_svgs([("chip", SVG)]));
    let chip = IconId(0);
    let mut held: Option<IconSet> = None;

    frontend_audit("palantir.alloc.many_icons", move |ui| {
        // `insert` drops last frame's clone *after* this frame's exists,
        // so the shared owner never reaches zero and nothing is released.
        let icons = held.insert(ui.load_icons(Rc::clone(&atlas)));
        Grid::new()
            .auto_id()
            .cols([Track::FILL; 20])
            .rows([Track::FILL; 10])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..10u16 {
                    for c in 0..20u16 {
                        Panel::zstack()
                            .id_salt((r, c))
                            .grid_cell((r, c))
                            .show(ui, |ui| {
                                ui.add_shape(icons.shape(chip));
                            });
                    }
                }
            });
    });
}
