# Damage rect proximity-merge research

**Status:** research only. No code change yet — landing this doc to capture
the survey + measurement plan before standing up the GPU bench. Live
implementation ships exactly the LVGL strict-overlap rule today
(`src/ui/damage/region/mod.rs`).

## Question

When two damage rects are nearby but disjoint, should the region
collapse them into one bbox (overdraw the gap) or keep them separate
(extra render pass)? This is a CPU-vs-GPU tradeoff:

- **Keep separate:** N render-pass setups per frame (pipeline state,
  scissor change, possibly extra queue submission cost).
- **Merge:** one pass, but unchanged pixels in the gap get redrawn
  (fragment-shader cost on the gap area).

Today: strict overlap-only merge (LVGL rule). Iced ships a proximity
threshold (`AREA_THRESHOLD = 20_000 px²`). Should we adopt one?

## What other systems do

| System | Policy | Cap | Cap overflow |
|---|---|---|---|
| **Iced** | `union_excess ≤ 20_000 px²` (absolute) | unbounded `Vec` | n/a |
| **Slint** | strict min-growth, only on overflow | 3 | absorb into best slot |
| **LVGL** | strict overlap (`union < A+B`) | 8 (`LV_INV_BUF_SIZE`) | full-screen invalidate |
| **Skia / pixman** | y-banded RLE, exact, no tolerance | unbounded RLE | n/a |
| **Chromium `cc::DamageTracker`** | single bbox | 1 | n/a |
| **WebRender / Servo** | single bbox; multi-rect opt-in | configurable, default 0 | platform-gated |
| **Qt Quick scenegraph** | bbox per layer | 1 per layer | n/a |
| **Hyprland / wlroots** | pixman canonicalization + bbox at present | unbounded RLE | n/a |
| **Palantir today** | strict overlap (LVGL) | 8 | min-growth (Slint-style) |

Notable findings from the survey:

- **Iced's 20_000 has no rationale in the commit or PR** (introduced
  Apr 2024 in `1e802e7` "Reintroduce damage tracking for
  iced_tiny_skia"). The number is scale-naive: 0.5 % of 1080p but
  12 % of a 320×240 embedded screen.
- **LVGL's rule** is *strict overlap*, not "overlap or adjacent" as
  earlier notes claimed: `union < A + B` algebraically requires
  `intersect > 0`. Forum discussion confirms no proximity tolerance.
- **Most production compositors collapse to a single bbox.** Chromium,
  WebRender's default, Qt Quick, wlroots-on-EGL all do this. The
  proximity-merge threshold is an Iced-specific call.
- **None of the bundled Rust references except Iced** (egui, vello,
  makepad, floem, xilem, quirky, clay) implement per-rect damage at
  all. Iced and Slint are the only precedents and they took opposite
  stances.

## Cost model

The actual decision is a one-line cost model:

```
merge if  gap_area * pixel_cost  <  pass_setup_cost
```

Iced's `union_excess ≤ 20_000` implicitly assumes
`pass_cost / pixel_cost ≈ 20_000`. That ratio is driver-, backend-,
and shader-dependent:

- **Desktop IMR (NVIDIA / AMD / Intel):** pass setup ≈ tens of µs.
  Low ratio → bias toward many rects.
- **TBDR (Apple GPU / ARM Mali / Qualcomm Adreno / PowerVR):** pass
  setup ≈ hundreds of µs (tile store + tile load on every pass
  start/end). High ratio → bias toward merging hard. Apple's docs
  explicitly warn against many-pass dirty-rect strategies.
- **Heavy fragment shaders (alpha glyph mask, rounded SDF):** pixel
  cost dominates → bias toward separate rects even on TBDR.

**Translation:** any single hardcoded threshold is wrong for a
framework that runs across desktop + mobile + heavy and light
shaders. It has to be a backend-init-time tunable.

## Measurement approach

No published `setup µs + pixel ns` for wgpu — varies by driver,
version, bind-group churn. Concrete recipe to derive numbers
ourselves:

1. **`wgpu::Features::TIMESTAMP_QUERY`** + `RenderPassTimestampWrites`
   per pass. Multiply raw timestamps by `Queue::get_timestamp_period()`
   to get nanoseconds.
2. **`wgpu-profiler`** crate (already in the wgpu ecosystem) has
   scoped pass timing — quickest off-the-shelf.
3. **Synthetic crossover sweep:**
   - Render N (1..32) disjoint 50×50 rects in N passes vs one
     bounding rect of equivalent covered area in one pass.
   - Sweep gap area 0..200_000 px².
   - Two shaders: trivial color fill, glyph alpha-mask (matches
     palantir's two real shader profiles).
   - Read off the gap area where `N-pass total time =
     merge-pass total time`. That crossover *is* the right
     `pass_cost / pixel_cost` for the active backend.

The crossover number, not Iced's 20 000, is the right default for
the active backend.

## Pitfalls of each policy

- **Iced absolute threshold:** doesn't scale with DPI / surface size.
  At 4K a 200×100 gap merges silently; on a small embedded surface
  it never does.
- **LVGL strict overlap:** "corner-pair pathology" — two 8×8 rects in
  opposite corners stay disjoint; an N-slot cap fills with these
  patterns and degenerates to full-screen on the next event.
  Mitigated for LVGL only because their target surfaces are sub-100k
  pixel where full-screen is cheap.
- **Slint min-growth on cap:** bounded memory, bounded work, but the
  absorbed slot can grow huge and cover most of the screen anyway,
  defeating the cap. Fine for embedded.
- **Chromium single-bbox:** zero CPU work, maximum overdraw. Correct
  call at compositor scale where each layer is already its own pass
  anchor.

## Recommendation

For Palantir's wgpu + desktop-first posture:

1. **Keep the 8-rect `tinyvec::ArrayVec` cap + 70 % coverage→Full
   escalation.** Both are sound; neither is the bottleneck.
2. **Replace strict overlap with a cost-model merge:**
   `merge if union_excess * pixel_cost < pass_cost`. Default
   `pass_cost = 5_000 px²` for desktop IMR (≈ 4× tighter than
   Iced's). On TBDR set `pass_cost = surface_area` (always merge).
   On software fallback set `pass_cost = 0` (LVGL semantics).
   Configured per backend at `WgpuBackend::new`, observable through
   `support::internals` for tests.
3. **Keep Slint min-growth as the cap-overflow fallback** (strictly
   better than LVGL's "promote to Full" on overflow).
4. **Build a `wgpu-profiler`-instrumented sweep bench**
   (`benches/damage_merge_gpu.rs`). Output: per-backend calibration
   constants + a settled default. Without GPU timing data we can't
   actually pick the threshold; without the bench every choice is a
   guess on top of Iced's unjustified 20 000.

## Open questions

- Should the threshold be **purely fixed-cost**
  (`union_excess < CONST`) or include a **per-rect cost** mod
  (`N_passes * setup + total_excess * pixel`)? The cost model above
  pairwise-merges greedily; a global formulation might collapse 3
  nearby rects together where pairwise would only get 2.
- **AA-fringe scissor padding** (open follow-up in
  `multi-rect-damage.md`) interacts with merge — a merge that
  fills a gap also fills two padding strips that were previously
  hazardous. Possibly the merge fix and the AA fix want to land
  together.
- **DPI scaling:** `pass_cost` and `pixel_cost` calibration is in
  physical px; the encoder-filter and damage rects are logical px.
  Convert at the threshold check (or store the threshold in
  physical px and convert at compare time).

## Starting point

Stand up the GPU sweep bench. That's the single piece of infra
blocking every threshold question. Numbers from it decide whether
to ship a proximity-merge threshold at all, and if so what the
per-backend defaults should be.

## Sources

- [iced damage.rs](https://github.com/iced-rs/iced/blob/master/graphics/src/damage.rs)
- [iced PR #377: damage tracking](https://github.com/iced-rs/iced/pull/377)
- [iced issue #367](https://github.com/iced-rs/iced/issues/367)
- [LVGL forum: lv_refr_join_area discussion](https://forum.lvgl.io/t/suggested-change-to-lv-refr-join-area/3082)
- [LVGL `lv_refr.c`](https://github.com/lvgl/lvgl/blob/master/src/core/lv_refr.c)
- `tmp/slint/internal/core/partial_renderer.rs:213-274`
- [Mozilla bug 1699603](https://bugzilla.mozilla.org/show_bug.cgi?id=1699603)
- [Mozilla bug 1582624](https://bugzilla.mozilla.org/show_bug.cgi?id=1582624)
- [Chromium `cc/trees/damage_tracker.cc`](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/cc/trees/damage_tracker.cc)
- [Chromium `cc` README](https://chromium.googlesource.com/chromium/src.git/+/master/docs/how_cc_works.md)
- [pixman-region.c](https://cgit.freedesktop.org/pixman/tree/pixman/pixman-region.c)
- [Magcius — Regional Geometry](https://magcius.github.io/xplain/article/regions.html)
- [Skia SkRegion API](https://api.skia.org/classSkRegion.html)
- [Qt Quick Ultralite Partial Framebuffer](https://doc.qt.io/QtForMCUs/platform-porting-guide-partial-framebuffer.html)
- [Apple — Tailor your apps for TBDR](https://developer.apple.com/documentation/metal/tailor-your-apps-for-apple-gpus-and-tile-based-deferred-rendering)
- [ARM Vulkanised 2017: Multipass mobile (PDF)](https://www.khronos.org/assets/uploads/developers/library/2017-khronos-uk-vulkanised/003-Vulkan-Multipass_May17.pdf)
- [Imagination — PowerVR TBDR architecture](https://blog.imaginationtech.com/a-look-at-the-powervr-graphics-architecture-tile-based-rendering/)
- [wgpu `QueryType` docs](https://docs.rs/wgpu/latest/wgpu/enum.QueryType.html)
- [wgpu timestamp_queries example](https://wgpu.rs/doc/wgpu_examples/timestamp_queries/index.html)
- [wgpu-profiler](https://github.com/Wumpf/wgpu-profiler)
- [Hyprland PR #39](https://github.com/hyprwm/Hyprland/pull/39)
