//! Backdrops: a pane that shows the pixels painted before it, blurred and shaded.
//!
//! [`backdrop_blur`] runs the blur a [`BlurPlan`](crate::shape::paint::blur_plan::BlurPlan)
//! describes, from a full-resolution capture of the pixels behind a pane, copied out of the target
//! where the main pass breaks at the pane; [`backdrops`] keeps each pane's result in an atlas
//! across frames, and blurs again the panes
//! [`PaneCaptures`](crate::renderer::pane_captures::PaneCaptures) marks to capture. Facts and
//! measurements: `.notes/research/2026-10-28-backdrop-effects.md`.
//!
//! # Decisions
//!
//! - **A capture is copied out of the main pass, broken at the pane.** The main pass ends at each
//!   capturing pane's barrier, the footprint is copied into the capture and blurred, and the next
//!   pass loads the target, as Impeller's `FlipBackdrop` and Chromium's render pass per backdrop
//!   do. It won whole frames on the AMD 680M (by up to 26 %) and the Apple M5 (by up to 14 %),
//!   and cost the M5 6 % on a partial frame, where the tiler stores and loads the whole
//!   attachment; the worst frame decides, and one path is kept. Refused: replaying the draws
//!   below a pane into its own target, which draws them twice and grows with the scene; a split
//!   for whole frames beside a replay for partial ones; last frame's pixels, which lag and hold
//!   the pane itself.
//! - **A result is cached across frames.** It is kept per render owner and pane key (the owner's
//!   `WidgetId` and its row in the owner's paint span) with its plan and region, in one atlas a
//!   roster keeps alive, and dropped with the owner. A hover or a caret inside a frosted menu
//!   repaints its rect and reads the cache: no capture, no blur.
//! - **The composer infers sharing.** A pane opens a barrier, where groups and text batches end,
//!   so nothing prunes or coalesces across it; panes that share a barrier share the pass break.
//!   Refused: an explicit group, as Flutter's `BackdropGroup` and SwiftUI's
//!   `GlassEffectContainer` ask for; merging captures, which no workload gained from, as
//!   neighboring panes lie in each other's footprints.
//! - **The pane is a fourth quad form, `QuadForm::Backdrop`.** SDF, corners, border, AA, turn,
//!   rounded clips and scissors hold as for any quad, and the form keeps the backdrop's reads and
//!   lens math off the solid form's registers. The fill is the tint; a gradient tint draws as its
//!   own quad above. A per-frame slot table carries each pane's atlas origin, lattice and lens.
//! - **Glass refracts by a closed form.** The bezel's height is a squircle
//!   `(1 − (1 − x)⁴)^¼` of the inward distance; its slope along the SDF gradient is the normal,
//!   Snell's law gives the bent ray, and thickness times its tangent the displacement (kube.io
//!   bakes the same into a map). Dispersion samples red and blue at their own index in the bezel;
//!   the highlight is Schlick's Fresnel toward a fixed top-left lamp. Its reach is the closed
//!   bound `thickness · √(n² − 1)`, so no frame samples the bezel to find it.
//! - **No banding.** The levels are `Rgba16Float`; before an 8-bit target writes, the composite
//!   adds interleaved gradient noise of one encoded step, scaled to linear by the sRGB curve's
//!   slope. Grain (`noise`) is separate and fixed to the pane, so it moves and turns with it.
//! - **A still pane costs nothing.** No allocation after warmup: the capture, the levels and the
//!   atlas grow with the largest surface and result seen and are kept.
//!
//! Out of scope: a filter on a subtree's own pixels (CSS `filter: blur`), which needs isolated
//! offscreen groups and has no workload; a progressive blur; OS window materials (Mica,
//! `NSVisualEffectView`), as Palantir draws its own opaque background.

pub(crate) mod backdrop_blur;
pub(crate) mod backdrops;
#[cfg(feature = "internals")]
pub(crate) mod bench;

#[cfg(test)]
mod tests;
