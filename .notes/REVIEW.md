Delete each item after addressing it. This document lists outstanding findings only; do not add resolved markers or a history section.

# Repository review

Scope: `.` — production code across the frame pipeline, layout, input, text, rendering, primitives, widgets, host integration, and derive crate. Reviewed on 2026-09-06. Groups are ordered by severity and benefit. Test organization and test-only APIs are excluded.

**Evidence:** “Reproduced” means a small executable exercised the library through public APIs and `UiHarness`. “Source trace” means the finding follows from the cited implementation, and “derived” gives the calculation. Every item below was re-read against the current source, and each one names a defect that is reachable through the public API.

## Image filtering happens before the required alpha and boundary operations

- [ ] **Medium — Tiled images clamp within each bilinear footprint instead of filtering across tile boundaries.** Derived from [the ClampToEdge sampler](/home/xxorza/Projects/palantir/src/renderer/backend/texture_binding.rs:61) and [the shader's fract wrapping](/home/xxorza/Projects/palantir/src/renderer/backend/image_pipeline/image.wgsl:155). Wrapping the sample coordinate does not wrap the adjacent texels that bilinear filtering reads. For a two-texel opaque black/white tile, a sample exactly at a repeat boundary returns black under clamping; a repeating bilinear sample blends the last and first texels to 0.5 linear gray. The same boundary error affects each footprint tap. Repeating image draws need repeat addressing at the filter level, while nonrepeating crops need their existing edge behavior.

## Widgets use response data that cannot represent the interaction they offer

- [ ] **Medium — show_when_disabled cannot enable tooltips for ordinary disabled widgets.** [Tooltip::show](/home/xxorza/Projects/palantir/src/widgets/tooltip/mod.rs:154) requires `snapshot.state.hovered` even when the option is enabled. [Cascade](/home/xxorza/Projects/palantir/src/scene/cascade/engine.rs:476) removes disabled nodes from hover sensing, so their ordinary responses cannot satisfy that condition. Reproduced with a disabled button, pointer inside its rectangle, zero tooltip delay, and `.show_when_disabled(true)`: no bubble is recorded. The input/public response contract needs an observation path for disabled hover that does not enable the disabled widget's actions.

## Built-in widgets depend on capabilities unavailable through the public API

- [ ] **Architectural — Several widgets cannot be reimplemented externally as required by the project contract.** Source trace: [GpuView](/home/xxorza/Projects/palantir/src/widgets/gpu_view/mod.rs:111) calls crate-only `Ui::gpu_view`; [Scroll](/home/xxorza/Projects/palantir/src/widgets/scroll/mod.rs:422) rewrites private node layout state and uses crate-only `Ui::scroll_content`. Both are capabilities with no public spelling at all, unlike `TextStyle::metrics_valid`, which an outside widget could re-derive from the public fields. Define documented public operations for the capabilities the widgets actually require, then make the built-ins use those same operations. The scroll layout mutation in particular needs a supported authoring operation rather than exposing internal node storage wholesale.

## Spring integration approximates a system with a closed-form transition

- [ ] **Precision and simplification — The adaptive Euler loop adds frame-partition error and numerical parameter restrictions that the spring model does not require.** [spring::step](/home/xxorza/Projects/palantir/src/animation/spring.rs:69) chooses up to 256 substeps and repeatedly applies generic value arithmetic; its validation carries stability margins and an integration-budget limit. For the implemented equation `x'' + damping*x' + stiffness*(x-target) = 0`, with the target held constant during a step, the underdamped, critically damped, and overdamped transitions have closed forms. Use a numerically stable analytical transition to remove the substep loop and its numerical stability restriction while retaining deliberate settling/convergence policy. This promises frame-partition independence up to floating-point error for a fixed target, not a measured speedup. [Ryan Juckett's original derivation](https://www.ryanjuckett.com/damped-springs/) gives the transition and its assumptions.
