//! The gates whose cost is partly the driver's rather than palantir's.
//!
//! Every wgpu submission allocates, and how much is the adapter's call: the
//! scale ramp spends 396 blocks a frame on a GeForce RTX 4090, 1059 on Mesa's
//! lavapipe and 1759 on Apple's Metal. Validation accounts for none of it.
//!
//! So neither gate reads a strict zero. The still-tree gate measures the
//! adapter's floor on an empty scene through the same target and pins the tree
//! to it, so CI runs it on every adapter. The ramp's ceiling was measured on
//! one adapter and does not transfer, so CI skips it by name.

use std::rc::Rc;

use glam::UVec2;
use palantir::internals::frame_fixture::{BENCH_DPR, FrameFixture};
use palantir::internals::{HeadlessTestGpuLease, TEXT_SCALE_STEP, isolated_headless_test_gpu};
use palantir::{
    Configure, Grid, IconId, IconSet, IconTable, Panel, Sizing, Track, TranslateScale, Ui,
};

use crate::gates::{MEASURE_FRAMES, WARMUP_FRAMES};
use crate::harness::{Audit, OffscreenTarget, Report};

/// Surface and tree for both GPU gates, smaller than the CPU one: the first
/// pins the driver's per-frame floor, which scales with submissions, not node
/// count. Shared so the ramp reads against the still-tree floor; the ramp
/// draws this tree plus a row of icons, and its excess is the miss path.
const RENDER_SURFACE: UVec2 = UVec2::new(1280, 800);
const RENDER_NODE_SCALE: usize = 6;

/// Offscreen frames of `scene` on `target`, warmed and measured with no ceiling.
fn device_frames(
    gpu: &HeadlessTestGpuLease,
    target: &mut OffscreenTarget,
    mut scene: impl FnMut(&mut Ui),
) -> Report {
    Audit::new()
        .warmup(WARMUP_FRAMES)
        .frames(MEASURE_FRAMES)
        .budget(u64::MAX)
        .run_frames(|| {
            let _ = target.frame(gpu, BENCH_DPR, &mut scene);
        })
}

/// The adapter's per-frame floor on `target`: an empty scene's modal count.
/// Every submission allocates (command encoder and buffer Arcs, the in-flight
/// `Vec` push, `wgpu_hal` pass scratch), and the offscreen path submits its
/// backbuffer copy every frame.
///
/// The floor holds for `target` alone: the copy's texture tracker grows to
/// each texture's index, so the count depends on where the device placed the
/// backbuffer and target among live textures (an Apple M5 read 25 or 28 by how
/// many unrelated textures were alive). Compare only against frames drawn
/// through the same target.
fn empty_floor(gpu: &HeadlessTestGpuLease, target: &mut OffscreenTarget) -> u64 {
    let floor = device_frames(gpu, target, |_| {}).mode;
    // A floor of zero means the gate has stopped measuring.
    assert!(
        floor > 0,
        "an empty frame counted no allocation — the wgpu submission path \
         allocates, so this gate is no longer watching it",
    );
    floor
}

/// A still tree damages nothing, so its frames must cost the device what an
/// empty scene's do through the same target: palantir's skipped-paint path is
/// heap-free.
///
/// Compared on the mode, not the worst frame: wgpu pools its encoders and
/// tracking vectors, so a rare frame lands a few blocks higher inside
/// `create_command_encoder` and `submit`. The mode omits those, so one more
/// allocation on most frames fails the comparison.
///
/// The leased test device has no timestamp or pipeline-statistics features,
/// whose queries allocate per frame.
#[test]
fn still_tree_frame_costs_the_empty_floor() {
    let gpu = isolated_headless_test_gpu();
    let mut target = OffscreenTarget::new(
        &gpu,
        "palantir.alloc_gate.still_tree.target",
        RENDER_SURFACE,
    );
    let floor = empty_floor(&gpu, &mut target);
    let mut state = FrameFixture::default();
    let tree = device_frames(&gpu, &mut target, |ui| {
        state.render(RENDER_NODE_SCALE, ui);
    });
    assert_eq!(
        tree.mode, floor,
        "the still tree's frames over the empty floor"
    );
}

/// One `16×16` box, so the SVG pipeline is what the rasterizer spends.
const RAMP_ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" rx="3" fill="#fff"/></svg>"##;

/// Icons the ramp draws: enough that a per-raster regression reads as a
/// multiple, few enough that glyphs still dominate as in a real UI.
const RAMP_ICONS: u16 = 5;

/// Frames the ramp measures, one raster rung each. Short on purpose: the zoom
/// carries content off the surface, so the glyph count falls (over 384 frames
/// the mean drops from 478 to 334 with the same worst frame). The costly
/// frames are the early ones that hold the whole tree.
const RAMP_FRAMES: usize = 64;

/// Per-frame ceiling for the ramp, against a measured worst frame of 400 and
/// a mean of 350.
///
/// Not zero: every frame misses every glyph and icon it draws (swash scales an
/// outline per glyph, resvg renders at an unseen size, both atlases insert).
/// Shaping hits, since the rung is a raster scale that `TextShapeKey` does not
/// carry. The floor belongs to the dependencies. The glyph set is fixed (the
/// four bundled faces), so the headroom is the driver's frame-to-frame spread
/// on this adapter, 398 to 404 over repeated runs.
///
/// It pins the per-miss cost: one more allocation per glyph lifts it by the
/// glyph count, and the audit checks each frame with backtraces. Both
/// rasterizers render into retained scratch, so neither contributes a block
/// per raster.
///
/// It does not reach atlas pressure: eviction needs the mask side full, and a
/// zoom that fills it has carried most of the tree off the surface, so this
/// exercises growth and the miss path, not `RasterAtlas::evict_one`'s cascade.
const RAMP_BLOCKS_PER_FRAME_MAX: u64 = 510;

/// A continuous zoom: the raster scale steps one [`TEXT_SCALE_STEP`] rung a
/// frame, so every glyph and icon resolves to a key neither atlas holds.
///
/// Other audits paint at a fixed scale, hitting after warmup, which leaves
/// `rasterize_and_insert`, the SVG rasterizer, both atlas insert paths and the
/// encoded-run cache's miss path unmeasured. A moving scale also damages the
/// whole surface, so this is the one audit that encodes and composes a full
/// tree. The warmup ramps too, or the window would measure the steady state.
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

    let floor = empty_floor(&gpu, &mut target);
    let report = Audit::new()
        .warmup(WARMUP_FRAMES)
        .frames(RAMP_FRAMES)
        .budget(RAMP_BLOCKS_PER_FRAME_MAX)
        .run_frames(|| {
            zoom += TEXT_SCALE_STEP;
            target.frame(&gpu, BENCH_DPR, |ui| {
                // Parked across frames: re-loading is a refcount bump.
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

    // A ramp that stopped missing would cost what a still frame does.
    assert!(
        report.mode > floor,
        "most frames counted {} blocks, no more than the empty floor of {floor} \
         — the ramp is no longer missing, so this gate is not watching the \
         rasterization path",
        report.mode,
    );
}
