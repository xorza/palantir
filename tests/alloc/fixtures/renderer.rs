//! Fixtures that push shape *count* and *variety* through a whole frame; the
//! other widget fixtures push structure.
//!
//! **These encode and compose.** `Audit::run` drives a `UiHarness`, which stops
//! at damage; these four use a [`FrontendHarness`], running the deviceless
//! frontend on every painting frame. They cover a shape's cost from `add_shape`
//! to the render buffer: record-store copies, the shape arena, cascade paint
//! rows, the encoder's command and the composer's scratch.
//!
//! No device, so the budget is strict zero; real-adapter cost is
//! `gates::on_gpu`'s.

use crate::harness::{Audit, new_ui};
use palantir::Stroke;
use palantir::internals::harness::frontend_harness::FrontendHarness;
use palantir::widget::{Mesh, Shape};
use palantir::{
    Block, Configure, FramePaint, Grid, IconId, IconSet, IconTable, Panel, RgbaF32, Sizing, Track,
    TranslateScale, Ui,
};
use std::rc::Rc;

/// Distinct nudge positions: more than one so damage is full every frame, few
/// enough that the tree isn't culled off the surface.
const NUDGE_POSITIONS: u32 = 4;

/// Whole nudge cycles of warmup; the scene cycles, so the probe could settle
/// before its widest frame (see `Warmup::Probe`).
const WARMUP_CYCLES: u32 = 4;

/// Frames of `scene` through the deviceless frontend, nudged one pixel sideways
/// each frame so every shape is re-encoded.
///
/// **The nudge makes this an audit of the frontend:** a still tree damages
/// nothing, so encode and compose would never run. It isn't free of the record
/// side, though: the panel row folds the transform into its node hash, so the
/// measure cache misses and layout re-runs. These are whole-frame numbers.
#[track_caller]
fn frontend_audit(mut scene: impl FnMut(&mut Ui)) {
    let mut frontend = FrontendHarness::new(new_ui());
    let mut step = 0u32;
    Audit::new()
        .warmup((WARMUP_CYCLES * NUDGE_POSITIONS) as usize)
        .run_frames(|| {
            step = (step + 1) % NUDGE_POSITIONS;
            let report = frontend.frame(|ui| {
                Panel::zstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .transform(TranslateScale::from_translation(glam::Vec2::new(
                        step as f32,
                        0.0,
                    )))
                    .show(ui, |ui| scene(ui));
            });
            // A small scene repaints partially, still encoding every shape the nudge moved.
            assert_ne!(report.paint(), FramePaint::Skip, "the frame repaints");
        });
}

/// 16x16 grid of `Block`s: 256 quads re-encoded every frame, stressing
/// `RenderCmdBuffer` and `RenderBuffer.quads` capacity reuse harder than
/// `grid_8x8` (64 quads).
#[test]
fn many_rects_compose_alloc_free() {
    frontend_audit(|ui| {
        Grid::new()
            .auto_id()
            .cols([Track::FILL; 16])
            .rows([Track::FILL; 16])
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for r in 0..16u16 {
                    for c in 0..16u16 {
                        Block::new()
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
/// window's record store at `add_shape`, so the closure can hold the `Vec`.
/// Pins the composer's polyline point / index / direction scratch reuse.
#[test]
fn polyline_static_alloc_free() {
    let points: Vec<glam::Vec2> = (0..32)
        .map(|i| glam::Vec2::new(i as f32 * 20.0, 100.0 + (i as f32).sin() * 30.0))
        .collect();
    frontend_audit(move |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::polyline(&points, Stroke::new(RgbaF32::WHITE, 2.0)));
            });
    });
}

/// Static `Mesh` pushed every frame via `Ui::add_shape`; vertex and index bytes
/// are copied into the tree's mesh arena. Pins that mesh encoding doesn't
/// allocate at steady state.
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
    frontend_audit(move |ui| {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::mesh(&mesh));
            });
    });
}

/// 200 icons per frame, each going record, encode, compose: a push onto
/// `RenderBuffer.icons` and nothing else. Rasters and atlas slots belong to the
/// backend, which this frontend never runs (`gates::on_gpu`'s scale ramp
/// reaches them).
///
/// The set is re-loaded inside the scene, as an immediate-mode caller writes it,
/// pinning that re-loading is a refcount bump: `IconRegistry::register` finds
/// the live `IconSet` and clones it. A second slot would show here first.
///
/// The set is *parked* across frames: an `IconSet` owns its parses and rasters,
/// so dropping it would unload and re-rasterize each frame (hence `#[must_use]`
/// on `load_icons`).
#[test]
fn many_icons_compose_alloc_free() {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" rx="3" fill="#fff"/></svg>"##;
    let atlas = Rc::new(IconTable::from_svgs([("chip", SVG)]).unwrap());
    let chip = IconId(0);
    let mut held: Option<IconSet> = None;

    frontend_audit(move |ui| {
        // `insert` drops last frame's clone *after* this frame's exists, so the shared
        // owner never reaches zero.
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
