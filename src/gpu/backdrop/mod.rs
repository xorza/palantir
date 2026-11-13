//! Backdrops: the pixels painted before a material or a backdrop effect, captured, blurred and
//! shaded.
//!
//! [`backdrop_blur`] runs the blur a [`BlurPlan`](crate::shape::paint::blur_plan::BlurPlan)
//! describes, from a full-resolution capture of the pixels behind a backdrop, copied out of the
//! target where the main pass breaks at the backdrop; [`backdrops`] keeps each backdrop's result in
//! an atlas across frames, and blurs again the backdrops
//! [`BackdropCaptures`](crate::renderer::backdrop_captures::BackdropCaptures) marks to capture.
//! Facts and measurements: `.notes/research/2026-10-28-backdrops.md`.
//!
//! # Decisions
//!
//! - **A capture is copied out of the main pass, broken at the backdrop.** The main pass ends at
//!   each capturing backdrop's barrier, the footprint is copied into the capture and blurred, and
//!   the next pass loads the target, as Impeller's `FlipBackdrop` and Chromium's render pass per
//!   backdrop do. It won whole frames on a mobile AMD iGPU (by up to 26 %) and an Apple laptop (by
//!   up to 14 %), and cost the Apple laptop 6 % on a partial frame, where the tiler stores and
//!   loads the whole attachment; the worst frame decides, and one path is kept. Refused: replaying
//!   the draws below a backdrop into its own target, which draws them twice and grows with the
//!   scene; a split for whole frames beside a replay for partial ones; last frame's pixels, which
//!   lag and hold the backdrop itself. A frame that paints the backbuffer, which a pass can sample,
//!   copies nothing: its blurs read the backbuffer as their level 0, between the passes that write
//!   it (fanless: −8 % on `*/under`, −6 % on `*/moving`). A swapchain image is sampled by no pass,
//!   as a group for it would be made each frame.
//! - **A result is cached across frames.** It is kept per render owner and backdrop key (the
//!   owner's `WidgetId` and its row in the owner's paint span) in one atlas a roster keeps alive,
//!   and dropped with the owner; the window's backdrop ledger holds its plan and region. A hover or
//!   a caret inside a frosted menu repaints its rect and reads the cache: no capture, no blur.
//! - **A full atlas evicts the least recently used result, and tells its window.** Past the
//!   device's largest texture, a result that finds no room evicts results no backdrop of the frame
//!   reads, oldest first and from any window, as WebRender's texture cache evicts at a failed
//!   allocation (Mozilla bugs 1641751, 1505639); each window's ledger drops the ones it held
//!   before its next frame decides, so they capture again. One that finds no room even then
//!   shows its fill over the clear for the frame. Refused: a release panic on data-driven
//!   content; a standalone texture per result that misses the atlas, which splits the
//!   composite's draw run.
//! - **The composer infers sharing.** A backdrop whose footprint misses every draw since the last
//!   break captures at that break, as it would see the same pixels at its own place; else it opens
//!   a barrier, where a group and the text batch end, so nothing prunes or coalesces across it. The
//!   backdrops of one break blur in one set of passes, each pass drawing every backdrop under its
//!   own scissor, their first-axis rows apart in the scratch: a pass costs the Apple laptop about
//!   48 µs whatever it draws (`.notes/research/2026-10-28-backdrops.md`, "Measured: a blur pass has
//!   a fixed cost"), and on the Apple laptop the shared break took 8 % off two backdrops apart and
//!   6 % off the three-backdrop frames. Refused: an explicit group, as Flutter's `BackdropGroup`
//!   and SwiftUI's `GlassEffectContainer` ask for, which lets a backdrop miss what another painted
//!   under it; one capture for the union of the footprints, which copies and blurs the gap between
//!   them.
//! - **A material draws through its effect's material branch.** A box's material and an app's
//!   backdrop effect draw through the effect pipeline, on one capture, ledger and atlas: one
//!   backdrop path. A backdrop effect's one fragment stage branches on `PALANTIR_MATERIAL`
//!   (`effect/contract.rs`, `MATERIAL_BRANCH`) to `effect/material.wgsl` for a material's draw: it
//!   calls the effect, holds what it shows within the luminance bounds, lays the box's fill, a
//!   color or a gradient, and its border over it in the same draw, dithers, and shades the shape a
//!   quad of the box would, so the corners, border, AA, turn, window, rounded clips and scissors
//!   hold as for any quad. A per-frame slot table carries each backdrop's atlas origin, lattice,
//!   the bounds, the fill and the border (`MaterialLook`). Refused: a stage of palantir's own per
//!   look, a second form of one thing; and, as `src/shape/AGENTS.md` ("A box's fill reaches under
//!   its border, a material under both, in one draw") holds, the gradient as a quad above and a
//!   material stage compiled apart. Cost: an opaque quad over a material does not prune it, as the
//!   occlusion pruner reads quads, so a covered material captures and blurs.
//! - **One glass, with its spectrum behind a branch.** The stock glass reads the spectrum only
//!   where its lens disperses. Cost, not yet measured: one shader for every look held the lens's
//!   registers for every backdrop and V3D spilled it, a blurred backdrop's composite 2.2 times as
//!   long on the Pi, so glass of one index and glass that spreads the spectrum drew through
//!   pipelines of their own; the frost is its own effect, and glass of one index may still spill
//!   there. If it does, the spectrum becomes a stock effect of its own.
//! - **Glass refracts by a closed form.** The bezel's height is a squircle `(1 − (1 − x)⁴)^¼` of
//!   the inward distance; its slope along the SDF gradient is the normal, Snell's law gives the
//!   bent ray, and thickness times its tangent the displacement (kube.io bakes the same into a
//!   map). The bent sample's reach is the closed bound `thickness · √(n² − 1)` at the densest
//!   index, so no frame samples the bezel to find it, and a backdrop wider than its bezel, that
//!   reach and a pixel together reads only itself (`LensOptics::margin`), so its footprint grows by
//!   a pixel, not the reach: glass frames 7 to 10 % faster on a handheld and the Apple laptop. A
//!   backdrop a clip cuts or a paint animation turns, and glass chrome, keep the reach, as the
//!   cascade knows only their screen rect.
//! - **The glint fades into the flat top.** Schlick's Fresnel at the bezel's tilt, times the 3D
//!   normal against a fixed lamp low at the top left: the flat top faces it with none, so the
//!   glint reaches 0 where the bezel meets it. Refused: facing by the screen normal alone, which
//!   lit the flat face's Fresnel across the inner bezel and stepped at its edge, 5 sRGB levels
//!   over the reel's grey backdrop.
//! - **Dispersion spreads the whole spectrum** (`glass_spectrum` in `effect/stock/glass.wgsl`).
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
//!   slope. Grain (the frost's `noise`) is separate and fixed to the box, so it moves and turns
//!   with it.
//! - **A still backdrop costs nothing, and an app with none pays for nothing.** The atlas and the
//!   slot table are made on the first frame with a backdrop, and the blur's pipelines on the first
//!   that blurs, so a frame of held results compiles no blur. The capture and the levels hold
//!   one break's footprints from the first texel the break reads, and grow by 256-texel steps to
//!   the largest break seen, so a resize drag makes them again only now and then; the atlas grows
//!   with the largest result. No allocation after warmup. Refused: a capture and levels of the
//!   whole surface, 55 MB at 4K for a menu of a few hundred px.
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
