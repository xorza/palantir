# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-contract two-pass
layout with flex-shrink sizing**, **wgpu rendering**.

## Posture

State-of-the-art UI framework, craft-driven.

- **Break things freely.** Rename, refactor, big-bang migrations welcome — no
  deprecation shims, compat aliases, feature flags, or migration helpers.
  Releases break; that's what pre-1.0 buys. Bar is "fmt + clippy + tests pass
  and the showcase still feels right by eye."
- **Per-frame allocation is a real metric.** Steady-state must be heap-alloc-free
  after warmup. New per-frame allocation or map rebuilding is a regression; push
  onto retained scratch with capacity reuse.
- **API ergonomics matter.** Builder chains read like prose, defaults are right,
  surprise behavior gets a pinning test. When in doubt, prioritize call-site
  readability.
- **Optimize aggressively when motivated.** Micro-wins (struct packing, const
  fns, scratch reuse, cache layout) are encouraged even without a workload
  demanding them.
- **Ship in measurable slices.** One feature with tests and a showcase section
  beats a half-finished cluster. If a change is structurally complex with no
  motivating workload, say "too early" and shelve with a note rather than ship
  speculation.
- **Docs are starting positions, not commitments.** Treat roadmaps, module
  docs, and this file as evolving and possibly wrong. When a doc contradicts
  user intent or current code, double-question rather than defer — flag the
  conflict and ask.

## Architecture

Five passes per frame over a tree rebuilt every frame: **record → measure →
arrange → cascade → encode + compose + paint**. Colour is linear-RGB f32
everywhere on the CPU side; sRGB encoding happens on the GPU at swapchain
write.

Rendering changes (shaders, encoder/composer, atlases, colour pipeline, layout
that moves pixels) need the visual suite run as well — the unit tests alone
won't catch a render regression.
