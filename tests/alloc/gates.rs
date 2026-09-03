//! Coarse gates over the whole pipeline, the counterpart to the
//! fine-grained fixtures next door.
//!
//! Those audit ~20 small scenes, most of them GPU-less, so a failure can
//! name the line that allocated. These three answer what a small scene
//! cannot:
//! whether the pipeline allocates at all at *full* scale, whether the
//! wgpu floor beneath it has drifted, and what a frame costs when every
//! glyph and icon on it misses its atlas.
//!
//! | gate | covers | budget |
//! |---|---|---|
//! | [`full_tree_cpu_frame_alloc_free`] | record → measure → arrange → cascade → damage over the frame bench's own tree, through real cosmic shaping. `Ui::frame` stops before the frontend, so no paint | strict zero |
//! | [`offscreen_frame_stays_at_driver_floor`] | a whole frame through `OffscreenHost::frame_offscreen` — encode, compose, and the wgpu submission, over a still tree | the driver floor |
//! | [`scale_ramp_rasterizes_at_a_flat_cost_per_frame`] | the same frame under a continuous zoom: full damage, glyph and icon rasterization, both atlases' insert paths | the measured miss cost |
//!
//! All three audit each measured frame on its own rather than summing a
//! window, so an intermittent grow-on-Nth-frame allocation (`Vec`
//! doubling, a `HashMap` rehash) fails on the frame that did it and
//! arrives with that frame's backtraces attached.

use std::rc::Rc;

use glam::UVec2;
use palantir::internals::{TEXT_SCALE_STEP, headless_test_gpu};
use palantir::{
    BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, Configure, FrameFixture, Grid, IconId, IconSet,
    IconTable, Panel, Sizing, Track, TranslateScale,
};

use crate::harness::{Audit, OffscreenTarget};

/// Measured, the fixture stabilizes by frame 4 — at 1 it still leaks
/// ~10 blocks — so this is margin. Too short is safe in the direction
/// that matters: the leftovers land inside the measured window and trip
/// the gate rather than hiding under it.
const WARMUP_FRAMES: usize = 16;
/// Long enough for a once-every-N-frames allocation to land inside the
/// window rather than after it.
const MEASURE_FRAMES: usize = 256;

/// Pins the `AGENTS.md` claim: "Per-frame allocation is a real metric.
/// Steady-state must be heap-alloc-free after warmup." Strict zero,
/// because everything on this path is ours.
///
/// Renders the frame bench's tree at its surface and dpr, through real
/// cosmic shaping rather than the mono fallback — so what clears here is
/// the tree that bench times, not a smaller stand-in whose quieter
/// caches prove less.
///
/// Coverage stops where `Ui::frame` does, at damage. The encode and
/// compose passes need a device: the two gates below run them over the
/// whole tree, and `fixtures/renderer.rs` runs them one shape kind at a
/// time.
#[test]
fn full_tree_cpu_frame_alloc_free() {
    let mut state = FrameFixture::default();
    Audit::new()
        .text()
        .surface(BENCH_SURFACE)
        .dpr(BENCH_DPR)
        .warmup(WARMUP_FRAMES)
        .frames(MEASURE_FRAMES)
        .run(|ui| state.render(BENCH_SCALE, ui));
}

/// Ceiling over the driver floor on the current wgpu/cosmic-text pin.
/// All attribution is wgpu_core/wgpu_hal — no palantir-side per-frame
/// allocs on this path.
///
/// **A ceiling over noise, not a pin on the floor.** The floor itself
/// is flat, the same count on every one of the 256 measured frames.
/// But wgpu pools its command encoders and tracking vectors, and how
/// often a call hits that pool depends on state palantir does not own,
/// so a rare frame lands several blocks above the rest — inside
/// `create_command_encoder` and `submit`, where palantir allocates
/// nothing itself. Sized against that spike, because a ceiling set to
/// the flat value fails on driver noise and reads as a regression.
///
/// Which means a regression is not what this catches. A palantir change
/// worth knowing about, or a wgpu bump, moves the *flat* value — printed
/// as the mean on every run, beside the worst. That line is the signal;
/// this constant only keeps the noise quiet.
const RENDER_BLOCKS_PER_FRAME_MAX: u64 = 44;

/// Surface and tree for both GPU gates, deliberately smaller than the
/// CPU one above: what the first of them pins is the driver's per-frame
/// floor, which scales with submissions rather than with node count. A
/// bigger tree would only make the same number slower to reach.
///
/// Shared so the ramp's number reads against the still-tree floor: the
/// ramp draws this tree plus a row of icons, and what it costs over the
/// floor is the miss path.
const RENDER_SURFACE: UVec2 = UVec2::new(1280, 800);
const RENDER_NODE_SCALE: usize = 6;

/// **Not** strict zero, and cannot be: every wgpu submission allocates a
/// `CommandEncoder` Arc, a `CommandBuffer` Arc, the queue's in-flight
/// `Vec` push, and per-pass scratch from `wgpu_hal`. So the gate catches
/// *drift* from that floor — a palantir regression, or a wgpu bump worth
/// looking at.
///
/// The leased test device carries no timestamp or pipeline-statistics
/// features, which matters: the queries an instrumented device runs
/// allocate per frame, and that is the very thing being counted here.
#[test]
fn offscreen_frame_stays_at_driver_floor() {
    let gpu = headless_test_gpu();
    let mut target =
        OffscreenTarget::new(&gpu, "palantir.alloc_gate.render.target", RENDER_SURFACE);
    let mut state = FrameFixture::default();
    let report = Audit::new()
        .warmup(WARMUP_FRAMES)
        .frames(MEASURE_FRAMES)
        .budget(RENDER_BLOCKS_PER_FRAME_MAX)
        .run_frames(|| target.frame(&gpu, BENCH_DPR, |ui| state.render(RENDER_NODE_SCALE, ui)));

    // A gate that reads zero has stopped measuring, and only the number
    // says so.
    assert!(
        report.worst > 0,
        "counted no allocation at all across {MEASURE_FRAMES} frames — the wgpu \
         submission path allocates, so this gate is no longer watching it",
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
/// palantir, and the headroom above it is for the platform fallback
/// faces, which differ per machine and so change how many glyphs a run
/// resolves to.
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
    let gpu = headless_test_gpu();
    let mut target = OffscreenTarget::new(
        &gpu,
        "palantir.alloc_gate.scale_ramp.target",
        RENDER_SURFACE,
    );

    let atlas = Rc::new(IconTable::from_svgs([("chip", RAMP_ICON_SVG)]));
    let chip = IconId(0);
    let mut held: Option<IconSet> = None;
    let mut state = FrameFixture::default();
    let mut zoom = 1.0f32;

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

    // A ramp that stopped missing would read as the still-tree gate above
    // and pin nothing this one exists for.
    assert!(
        report.worst > RENDER_BLOCKS_PER_FRAME_MAX,
        "counted {} blocks on the worst frame, no more than the still-tree floor \
         of {RENDER_BLOCKS_PER_FRAME_MAX} — the ramp is no longer missing, so this \
         gate is not watching the rasterization path",
        report.worst,
    );
}
