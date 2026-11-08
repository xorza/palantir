//! Backdrops: a pane that shows the pixels painted before it, blurred and shaded.
//!
//! [`backdrop_blur`] runs the blur a [`BlurPlan`](crate::shape::paint::blur_plan::BlurPlan)
//! describes, from a full-resolution capture of the pixels behind a pane, copied out of the target
//! where the main pass breaks at the pane; [`backdrops`] keeps each pane's result in an atlas
//! across frames, and blurs again the panes
//! [`PaneCaptures`](crate::renderer::pane_captures::PaneCaptures) marks to capture. Facts and
//! measurements: `.notes/research/2026-10-28-backdrops.md`.
//!
//! # Decisions
//!
//! - **A capture is copied out of the main pass, broken at the pane.** The main pass ends at each
//!   capturing pane's barrier, the footprint is copied into the capture and blurred, and the next
//!   pass loads the target, as Impeller's `FlipBackdrop` and Chromium's render pass per backdrop
//!   do. It won whole frames on a mobile AMD iGPU (by up to 26 %) and an Apple laptop (by up to 14 %),
//!   and cost the Apple laptop 6 % on a partial frame, where the tiler stores and loads the whole
//!   attachment; the worst frame decides, and one path is kept. Refused: replaying the draws
//!   below a pane into its own target, which draws them twice and grows with the scene; a split
//!   for whole frames beside a replay for partial ones; last frame's pixels, which lag and hold
//!   the pane itself. A frame that paints the backbuffer, which a pass can sample, copies
//!   nothing: its blurs read the backbuffer as their level 0, between the passes that write it
//!   (fanless: −8 % on `*/under`, −6 % on `*/moving`). A swapchain image is sampled by no pass, as a
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
//! - **The composer infers sharing.** A pane whose footprint misses every draw since the last break
//!   captures at that break, as it would see the same pixels at its own place; else it opens a
//!   barrier, where groups and text batches end, so nothing prunes or coalesces across it. The
//!   panes of one break blur in one set of passes, each pass drawing every pane under its own
//!   scissor, their first-axis rows apart in the scratch: a pass costs the Apple laptop about 48 µs whatever
//!   it draws (`.notes/research/2026-10-28-backdrops.md`, "Measured: a blur pass has a fixed
//!   cost"), and on the Apple laptop the shared break took 8 % off two panes apart and 6 % off the three-pane
//!   frames. Refused: an explicit group, as Flutter's `BackdropGroup` and SwiftUI's
//!   `GlassEffectContainer` ask for, which lets a pane miss what another painted under it; one
//!   capture for the union of the footprints, which copies and blurs the gap between them.
//! - **The pane is palantir's own backdrop effect** (`effect_pipeline/pane.wgsl`), drawn through
//!   the effect pipeline an app's backdrop effect takes, on one capture, ledger and atlas: one
//!   backdrop path. It shades the rect a quad of it would, packed as one, so the SDF, corners,
//!   stroke, AA, turn, window, rounded clips and scissors hold as for any quad, and the pixels
//!   match the quad forms it replaced. A per-frame slot table carries each pane's atlas origin,
//!   lattice and the filter's look (`FilterLook`); the solid tint and the stroke draw in the
//!   pane's own stage, as two draws would cross their antialiased bands, and a gradient tint draws
//!   as its own quad above. Frost, glass of one index and glass that spreads the spectrum draw
//!   through forms of their own (`PaneForm`): one shader with the lens behind a branch held its
//!   registers for every pane, and V3D spilled it, so a blurred pane's composite took the Pi 2.2
//!   times as long. Refused: the quad forms beside an app's effects, two composite paths to keep.
//!   Cost: an opaque quad over a pane no longer prunes it, as the occlusion pruner reads quads, so
//!   a covered pane captures and blurs. Not yet measured against the quad forms on the
//!   `gpu/backdrop` benches, as the owner decided on 2026-11-08.
//! - **Glass refracts by a closed form.** The bezel's height is a squircle
//!   `(1 − (1 − x)⁴)^¼` of the inward distance; its slope along the SDF gradient is the normal,
//!   Snell's law gives the bent ray, and thickness times its tangent the displacement (kube.io
//!   bakes the same into a map). The bent sample's reach is the closed bound
//!   `thickness · √(n² − 1)` at the densest index, so no frame samples the bezel to find it, and
//!   a pane wider than its bezel, that reach and a pixel together reads only itself
//!   (`LensOptics::margin`), so its footprint grows by a pixel, not the reach: glass frames 7 to
//!   10 % faster on a handheld and the Apple laptop. A pane a clip cuts or a paint animation turns, and glass
//!   chrome, keep the reach, as the cascade knows only their screen rect.
//! - **The glint fades into the flat top.** Schlick's Fresnel at the bezel's tilt, times the 3D
//!   normal against a fixed lamp low at the top left: the flat top faces it with none, so the
//!   glint reaches 0 where the bezel meets it. Refused: facing by the screen normal alone, which
//!   lit the flat face's Fresnel across the inner bezel and stepped at its edge, 5 sRGB levels
//!   over the reel's grey pane.
//! - **Dispersion spreads the whole spectrum** (`lens_spectrum` in `quad_pipeline/shader.wgsl`).
//!   Each wavelength refracts at its index by Cauchy's model through `n_D`, and the reads sum back
//!   to color in CIE XYZ under the 1931 observer, normalized so a field of one color keeps it, as
//!   Filament's specgen builds its matrices; the weight between reads is integrated in closed form
//!   (`.notes/research/2026-10-28-backdrops.md`, "Measured: dispersion"). Refused: red, green and
//!   blue at their own index (luma.gl, Haze's `Simple`), two sharp ghosts of a bright edge; seven
//!   reads weighted by eye (Haze's `Full`); a sum over the reads alone, which fringes a white edge
//!   with a pure green; a weight loop over a fine grid, 19–36 % of a glass frame. The tables are a
//!   buffer: as a constant array indexed at run time, V3D took 1288 uniforms and a select over
//!   every entry, 2.6 times the glass composite's time at 512².
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
