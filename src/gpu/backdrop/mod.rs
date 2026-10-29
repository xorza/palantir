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
//!   the pane itself. A frame that paints the backbuffer, which a pass can sample, copies
//!   nothing: its blurs read the backbuffer as their level 0, between the passes that write it
//!   (m2: −8 % on `*/under`, −6 % on `*/moving`). A swapchain image is sampled by no pass, as a
//!   group for it would be made each frame.
//! - **A result is cached across frames.** It is kept per render owner and pane key (the owner's
//!   `WidgetId` and its row in the owner's paint span) in one atlas a roster keeps alive, and
//!   dropped with the owner; the window's pane ledger holds its plan and region. A hover or a
//!   caret inside a frosted menu repaints its rect and reads the cache: no capture, no blur.
//! - **A full atlas evicts the least recently used result, and tells its window.** Past the
//!   device's largest texture, a result that finds no room evicts results no pane of the frame
//!   reads, oldest first and from any window, as WebRender's texture cache evicts at a failed
//!   allocation (Mozilla bugs 1641751, 1505639); each window's ledger drops the ones it held
//!   before its next frame decides, so they capture again. One that finds no room even then
//!   shows its tint over the clear for the frame. Refused: a release panic on data-driven
//!   content; a standalone texture per result that misses the atlas, which splits the
//!   composite's draw run.
//! - **The composer infers sharing.** A pane whose footprint misses every draw since the last
//!   break captures at that break, as it would see the same pixels at its own place; else it
//!   opens a barrier, where groups and text batches end, so nothing prunes or coalesces across
//!   it. The panes of one break blur in one set of passes, each pass drawing every pane under its
//!   own scissor, their first-axis rows apart in the scratch: a pass costs the M2 about 48 µs
//!   whatever it draws (`.notes/research/2026-10-29-backdrop-passes.md`), and on the M2 the
//!   shared break took 8 % off two panes apart and 6 % off the three-pane frames. Refused: an
//!   explicit group, as Flutter's `BackdropGroup` and SwiftUI's `GlassEffectContainer` ask for,
//!   which lets a pane miss what another painted under it; one capture for the union of the
//!   footprints, which copies and blurs the gap between them.
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
//! - **A still pane costs nothing, and an app with none pays for nothing.** The blur's pipelines
//!   and the atlas are made on the first frame with a pane. The capture and the levels hold one
//!   break's footprints from the first texel the break reads, and grow by 256-texel steps to the
//!   largest break seen, so a resize drag makes them again only now and then; the atlas grows with
//!   the largest result. No allocation after warmup. Refused: a capture and levels of the whole
//!   surface, 55 MB at 4K for a menu of a few hundred px.
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
