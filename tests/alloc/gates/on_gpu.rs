//! The gates whose cost is partly the driver's rather than palantir's.
//!
//! Every wgpu submission allocates, and how much is the adapter's call:
//! the scale ramp below spends 396 blocks a frame on a GeForce RTX 4090,
//! 1059 on Mesa's lavapipe and 1759 on Apple's Metal. Validation
//! accounts for none of it — `WGPU_VALIDATION=0` moves the count by zero.
//!
//! So neither gate reads a strict zero. The still-tree gate needs no
//! number at all: it measures the adapter's floor on an empty scene in
//! the same run and pins the tree to it, so CI runs it on every adapter.
//! The ramp's ceiling was measured on one adapter and does not transfer —
//! widened to fit every driver it would let several hundred blocks a
//! frame through unseen — so CI skips that one by name.

use std::rc::Rc;

use glam::UVec2;
use palantir::internals::frame_fixture::{BENCH_DPR, FrameFixture};
use palantir::internals::{HeadlessTestGpuLease, TEXT_SCALE_STEP, isolated_headless_test_gpu};
use palantir::{
    Configure, Grid, IconId, IconSet, IconTable, Panel, Sizing, Track, TranslateScale, Ui,
};

use crate::gates::{MEASURE_FRAMES, WARMUP_FRAMES};
use crate::harness::{Audit, OffscreenTarget, Report};

/// Surface and tree for both GPU gates, deliberately smaller than the
/// CPU one: what the first of them pins is the driver's per-frame floor,
/// which scales with submissions rather than with node count. A bigger
/// tree would only make the same number slower to reach.
///
/// Shared so the ramp's number reads against the still-tree floor: the
/// ramp draws this tree plus a row of icons, and what it costs over the
/// floor is the miss path.
const RENDER_SURFACE: UVec2 = UVec2::new(1280, 800);
const RENDER_NODE_SCALE: usize = 6;

/// Offscreen frames of `scene` on a fresh target, warmed and measured
/// with no ceiling — what they cost is the answer, not the test.
fn device_frames(gpu: &HeadlessTestGpuLease, mut scene: impl FnMut(&mut Ui)) -> Report {
    let mut target = OffscreenTarget::new(gpu, "palantir.alloc_gate.floor.target", RENDER_SURFACE);
    Audit::new()
        .warmup(WARMUP_FRAMES)
        .frames(MEASURE_FRAMES)
        .budget(u64::MAX)
        .run_frames(|| {
            let _ = target.frame(gpu, BENCH_DPR, &mut scene);
        })
}

/// The adapter's per-frame floor: an empty scene's modal count. Every
/// submission allocates — a `CommandEncoder` Arc, a `CommandBuffer` Arc,
/// the queue's in-flight `Vec` push, per-pass scratch from `wgpu_hal` —
/// and the offscreen path submits its backbuffer copy on every frame.
fn empty_floor(gpu: &HeadlessTestGpuLease) -> u64 {
    let floor = device_frames(gpu, |_| {}).mode;
    // A floor that reads zero has stopped measuring, and only the number
    // says so.
    assert!(
        floor > 0,
        "an empty frame counted no allocation — the wgpu submission path \
         allocates, so this gate is no longer watching it",
    );
    floor
}

/// A still tree damages nothing, so its frames must cost the device what
/// an empty scene's do, on the same adapter: whatever palantir does on
/// the way to a skipped paint, it does without the heap.
///
/// Compared on the mode, not the worst frame. wgpu pools its command
/// encoders and tracking vectors, and how often a call hits that pool
/// depends on state palantir does not own, so a rare frame lands a few
/// blocks above the rest inside `create_command_encoder` and `submit`.
/// The mode leaves those frames out, so the comparison can be exact: one
/// more allocation on most frames fails it.
///
/// The leased test device carries no timestamp or pipeline-statistics
/// features, which matters: the queries an instrumented device runs
/// allocate per frame, and that is the very thing being counted here.
#[test]
fn still_tree_frame_costs_the_empty_floor() {
    let gpu = isolated_headless_test_gpu();
    let floor = empty_floor(&gpu);
    let mut state = FrameFixture::default();
    let tree = device_frames(&gpu, |ui| state.render(RENDER_NODE_SCALE, ui));
    assert_eq!(
        tree.mode, floor,
        "the still tree's frames over the empty floor"
    );
}

/// One `16×16` box, so what the rasterizer spends is the SVG pipeline
/// rather than the drawing in it.
const RAMP_ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" rx="3" fill="#fff"/></svg>"##;

/// Icons the ramp draws. Enough that a per-raster regression reads as a
/// multiple rather than as noise, few enough that the glyph side still
/// dominates the frame the way a real UI does.
const RAMP_ICONS: u16 = 5;

/// Frames the ramp measures, one raster rung each.
///
/// Short on purpose. The zoom carries content off the surface as it
/// climbs, so the glyph count per frame falls with it: measured over 384
/// frames the worst frame is the same one but the mean drops from 478 to
/// 334, which is 320 frames of measuring less and less. The costly frames
/// are the early ones, and this window is the ones that hold the whole
/// tree.
const RAMP_FRAMES: usize = 64;

/// Per-frame ceiling for the ramp, against a measured worst frame of
/// 400 and a mean of 350.
///
/// **Not zero, and it cannot be.** Every frame here misses every glyph
/// and every icon it draws: swash scales an outline per glyph, resvg
/// renders the parsed tree at a size it has not been rendered at, and
/// both atlases take an insert. Shaping is not in that list — the rung is
/// a raster scale, which `TextShapeKey` does not carry, so the shaped
/// buffers hit. The floor belongs to the dependencies rather than to
/// palantir. The glyph set is fixed — the host shapes with the four
/// bundled faces and nothing from the machine — so the headroom above the
/// measured worst is for the driver's frame-to-frame spread on this
/// adapter, which repeated runs put at 398 to 404.
///
/// What it pins is the *per-miss* cost. A regression that allocated once
/// more per glyph would lift this by the glyph count, and the audit
/// checks every frame on its own, so it fails on the frame that did it
/// with that frame's backtraces attached. Both rasterizers render into
/// retained scratch and hand back a borrow, so neither the glyph nor the
/// icon side contributes a block per raster; anything that made one of
/// them own its pixels again would show up here first.
///
/// **What it does not reach is atlas pressure.** Eviction needs the mask
/// side full, and a zoom that climbs far enough to fill it has already
/// carried most of the tree off the surface — so this ramp exercises
/// growth and the miss path, not the re-rasterize cascade
/// `RasterAtlas::evict_one` describes.
const RAMP_BLOCKS_PER_FRAME_MAX: u64 = 510;

/// A continuous zoom: the raster scale steps one [`TEXT_SCALE_STEP`] rung
/// a frame, so every glyph and every icon on screen resolves to a key
/// neither atlas holds.
///
/// The gap this closes. Every other audit in the suite paints at a fixed
/// scale, so its glyphs and icons are rasterized during warmup and hit
/// for the rest of the run — leaving `rasterize_and_insert`, the SVG
/// rasterizer, both atlases' insert paths and the encoded-run cache's
/// miss path outside every measured window. A moving scale also damages
/// the whole surface every frame, so this is the one audit that encodes
/// and composes a full tree rather than an empty damage region.
///
/// The warmup ramps too. Stopping the zoom to warm up would hand the
/// window a full set of hits and measure the steady state twice.
#[test]
fn scale_ramp_rasterizes_at_a_flat_cost_per_frame() {
    let gpu = isolated_headless_test_gpu();
    let mut target = OffscreenTarget::new(
        &gpu,
        "palantir.alloc_gate.scale_ramp.target",
        RENDER_SURFACE,
    );

    let atlas = Rc::new(IconTable::from_svgs([("chip", RAMP_ICON_SVG)]).unwrap());
    let chip = IconId(0);
    let mut held: Option<IconSet> = None;
    let mut state = FrameFixture::default();
    let mut zoom = 1.0f32;

    let floor = empty_floor(&gpu);
    let report = Audit::new()
        .warmup(WARMUP_FRAMES)
        .frames(RAMP_FRAMES)
        .budget(RAMP_BLOCKS_PER_FRAME_MAX)
        .run_frames(|| {
            zoom += TEXT_SCALE_STEP;
            target.frame(&gpu, BENCH_DPR, |ui| {
                // Parked across frames, so re-loading is a refcount bump
                // and the set's rasters are never unloaded.
                let icons = held.insert(ui.load_icons(Rc::clone(&atlas)));
                Panel::vstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .transform(TranslateScale::from_scale(zoom))
                    .show(ui, |ui| {
                        Grid::new()
                            .id_salt("icons")
                            .cols([Track::HUG; RAMP_ICONS as usize])
                            .rows([Track::HUG])
                            .show(ui, |ui| {
                                for c in 0..RAMP_ICONS {
                                    Panel::zstack().id_salt(c).grid_cell((0, c)).show(ui, |ui| {
                                        ui.add_shape(icons.shape(chip));
                                    });
                                }
                            });
                        state.render(RENDER_NODE_SCALE, ui);
                    });
            });
        });

    // A ramp that stopped missing would cost what a still frame does and
    // pin nothing this one exists for.
    assert!(
        report.mode > floor,
        "most frames counted {} blocks, no more than the empty floor of {floor} \
         — the ramp is no longer missing, so this gate is not watching the \
         rasterization path",
        report.mode,
    );
}
