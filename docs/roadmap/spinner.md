# Spinner

**Status:** design / proposal. Motivated by app code that wants a
generic "work in progress" indicator — busy buttons, async list
loaders, saving toasts.

Visual target: the 12-spoke macOS / Aqua-style throbber. Spokes
arranged at 30° intervals, each spoke a round-capped capsule, opacity
fading from a "head" spoke (full alpha) backward through the wheel
(tail ≈ 10–15 % alpha). The head advances one slot per `cycle / N`
seconds. Continuous, no settle state.

## Three sub-problems

Today's primitives don't cover this on their own. Each blocker has
its own scope and ships standalone.

### A. No rotated capsule primitive

Each spoke is a thick line segment with round caps in an arbitrary
direction.

- `Shape::Line { a, b, width, color }` is already in
  `src/shape.rs`, complete with a structural `Hash` impl
  (`shape.rs:109–115`), but the encoder **drops it** —
  `src/renderer/frontend/encoder/mod.rs:127`:
  `tracing::trace!(?shape, "encoder: dropping unsupported shape")`.
- `Shape::RoundedRect` is axis-aligned in node-local space, and
  `ElementExtras.transform` is `TranslateScale` only (translation +
  uniform scale, **no rotation**;
  `src/primitives/transform.rs:5–8` calls this out as load-bearing
  for axis-aligned scissor and the rounded-rect SDF).
- Per-instance `Quad` is 68 B with no rotation slot. Bolting one
  onto every quad pessimizes a primitive used everywhere for a
  feature used in a few places.

So the path is: **promote `Shape::Line` to a real, GPU-rendered
primitive backed by its own pipeline**. Reusable far beyond
spinners — text underlines/strikethroughs, focus-ring connectors,
debug-overlay arrows, and eventual node-graph edges
(`deferred-shapes.md` case 7). Spinner is the forcing function.

### B. No public hook for perpetual repaint

The spinner never settles. Today `Ui::repaint_requested` is
`pub(crate)` and only flipped by `Ui::animate` for a non-settled
animation row (`src/ui/mod.rs:374`). No widget-facing way to say
"tick me again next frame."

Add two public hooks on `Ui`:

```rust
// src/ui/mod.rs
impl Ui {
    /// Force the next frame to paint even when input is idle. Used
    /// by perpetual animations (spinner, marquee) that don't fit
    /// `Ui::animate`'s settle model.
    pub fn request_repaint(&mut self) {
        self.repaint_requested = true;
    }

    /// Last `now` argument passed to `run_frame`. Monotonic.
    /// Animation widgets compute phase from this so they don't
    /// thread `dt` through their own state.
    pub fn time(&self) -> std::time::Duration {
        self.time
    }
}
```

This aligns with `animations.md`'s open question about
`request_repaint` granularity and is what `TextEdit` already fakes
via `animate` for caret blink. If the open question's
`Option<Duration>` upgrade ever lands, `request_repaint` grows a
parameter.

### C. Authoring-time geometry without a measured rect

`Ui::node` runs the body closure during the *record* pass, before
measure / arrange. A widget cannot read its own arranged rect at
authoring time — `LayoutResult` doesn't exist yet. So the spinner
can't naively compute "leaf rect center + radius" inside its
recording closure.

Two real options:

- **(c1) Read prev-frame rect via `ui.response_for(id).rect`**, the
  pattern scrollbars use today (`scroll.md` lists the F+1 settle as
  a known bug). Needs a sensible fallback for F+0.
- **(c2) Constrain the spinner to a *deterministic* size** — set
  `element.size = Sizes::Fixed(DEFAULT_SIZE)` in `Spinner::new` so
  the resolved size equals the builder's declaration regardless of
  parent. Author spoke endpoints in *leaf-local* coords (origin =
  leaf top-left, size = `self.size`); the encoder translates by
  `owner_rect.min` at emit time, just like `RoundedRect` /
  `Text::local_rect: Some`. No prev-frame state, no F+1 settle.

**Recommend (c2).** Spinners are intrinsically small fixed-size
controls; a Fill/Hug spinner is a non-feature. (c2) costs a single
`.size(...)` line in `Spinner::new` and rules out a class of
first-frame visual bugs by construction.

`Shape::Line` endpoints become owner-relative as part of bringing
it to life — convention parity with `RoundedRect`, no surprise.

---

## Line renderer — concrete implementation

Work touches three layers: a new GPU pipeline + WGSL shader, a new
command in `RenderCmdBuffer`, and one match-arm in the encoder.
The composer painter-order invariant generalizes; the schedule
gains one variant.

### 1. Per-instance type — `src/renderer/line.rs` (new)

Mirror `src/renderer/quad.rs`. 36 B, alignment 4, no padding bytes
(`8 + 8 + 4 + 16 = 36`, multiple of 4) — the `padding_struct` macro
is a no-op here today, kept for forward compatibility per
`CLAUDE.md`.

```rust
//! Per-instance capsule data (round-capped line). Frontend↔backend
//! contract; lives at the renderer root next to `Quad`.

use crate::primitives::color::Color;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
#[padding_struct::padding_struct]
pub(crate) struct LineInst {
    pub(crate) a: Vec2,      // endpoint 0, physical px
    pub(crate) b: Vec2,      // endpoint 1, physical px
    pub(crate) width: f32,   // capsule diameter, physical px
    pub(crate) fill: Color,
}

impl LineInst {
    pub(crate) fn new(a: Vec2, b: Vec2, width: f32, fill: Color) -> Self {
        Self { a, b, width, fill, ..bytemuck::Zeroable::zeroed() }
    }
}
```

Pin the size in a `#[test]` matching `quad.rs`'s
`quad_struct_is_68_bytes_no_padding` so a future field reorder
can't drift the `vertex_attr_array` offsets.

### 2. Shader — `src/renderer/backend/line.wgsl` (new)

Vertex stage emits an oriented bounding rect around the segment,
inflated by `width / 2 + 1 px` AA pad. Fragment runs a 2D capsule
SDF and AA-blends with the same `clamp(0.5 - d, 0, 1)` convention
as `quad.wgsl`'s rounded-rect SDF — so capsules and quads
composite consistently when overlapped. Pre-multiplied alpha,
matching `BlendState::PREMULTIPLIED_ALPHA_BLENDING`.

```wgsl
struct Viewport { size: vec2<f32> };
@group(0) @binding(0) var<uniform> viewport: Viewport;

struct VertexOut {
    @builtin(position) clip:  vec4<f32>,
    @location(0)       p:     vec2<f32>, // pixel-space frag pos
    @location(1)       a:     vec2<f32>,
    @location(2)       b:     vec2<f32>,
    @location(3)       half:  f32,       // width * 0.5
    @location(4)       fill:  vec4<f32>,
};

const AA_PAD: f32 = 1.0;

@vertex
fn vs(
    @builtin(vertex_index) vi: u32,
    @location(0) a:     vec2<f32>,
    @location(1) b:     vec2<f32>,
    @location(2) width: f32,
    @location(3) fill:  vec4<f32>,
) -> VertexOut {
    let half = width * 0.5 + AA_PAD;
    let d   = b - a;
    let len = length(d);
    // Degenerate point capsule (a == b): pick an arbitrary tangent
    // so the bbox still has nonzero extent for the cap discs.
    let t  = select(vec2<f32>(1.0, 0.0), d / len, len > 0.0);
    let n  = vec2<f32>(-t.y, t.x);

    var corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0,  1.0),
    );
    let c = corners[vi];
    let center = (a + b) * 0.5;
    let along  = (len * 0.5 + half) * c.x;
    let across = half * c.y;
    let pixel  = center + t * along + n * across;

    let clip = vec2<f32>(
        pixel.x / viewport.size.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.size.y * 2.0,
    );

    var out: VertexOut;
    out.clip = vec4<f32>(clip, 0.0, 1.0);
    out.p    = pixel;
    out.a    = a;
    out.b    = b;
    out.half = width * 0.5;
    out.fill = fill;
    return out;
}

fn sdf_capsule(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, half: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h  = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h) - half;
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    let d  = sdf_capsule(in.p, in.a, in.b, in.half);
    let aa = clamp(0.5 - d, 0.0, 1.0);
    let a  = in.fill.a * aa;
    return vec4<f32>(in.fill.rgb * a, a);
}
```

No `fs_mask` variant — capsules never become rounded-clip masks
(quads do that job).

### 3. Pipeline — `src/renderer/backend/line_pipeline.rs` (new)

Mirror the shape of `quad_pipeline.rs` but trimmed: no
`upload_clear`, no `upload_overlays`, no `upload_masks`, no
`mask_instance` builder. Two pipelines: base (no stencil) and
`stencil_test` (matching `super::stencil_test_state()` so
quad-and-line draws inside a rounded-clip group share stencil
behavior).

```rust
pub(crate) struct LinePipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    stencil_test: Option<wgpu::RenderPipeline>,
    // Reused inputs needed for lazy stencil build.
    shader: wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    bind_layout: wgpu::BindGroupLayout,
}
```

Reuse `QuadPipeline`'s viewport bind-group layout *and* its
`viewport_buffer` so both pipelines see the same uniform — pass
both into `LinePipeline::new(device, format, bind_layout,
viewport_buffer)`. Avoids a second viewport upload per frame.

```rust
let instance_layout = wgpu::VertexBufferLayout {
    array_stride: std::mem::size_of::<LineInst>() as u64,
    step_mode: wgpu::VertexStepMode::Instance,
    attributes: &wgpu::vertex_attr_array![
        0 => Float32x2,   // a
        1 => Float32x2,   // b
        2 => Float32,     // width
        3 => Float32x4,   // fill
    ],
};
```

`PrimitiveTopology::TriangleStrip`, `draw(0..4, range)` —
identical draw shape to quads.

`upload`, `bind`, `bind_stencil_test`, `draw_range` mirror
`QuadPipeline` 1:1; `ensure_stencil` lazy-builds the stencil-test
variant on first rounded-clip frame, just like quads do.

### 4. Output buffer — `src/renderer/render_buffer.rs`

Add a `lines` column and per-group `lines: Span`:

```rust
pub(crate) struct RenderBuffer {
    pub(crate) quads: Vec<Quad>,
    pub(crate) lines: Vec<LineInst>,    // new
    pub(crate) texts: Vec<TextRun>,
    pub(crate) groups: Vec<DrawGroup>,
    // … unchanged …
}

pub(crate) struct DrawGroup {
    pub(crate) scissor: Option<URect>,
    pub(crate) rounded_clip: Option<Corners>,
    pub(crate) quads: Span,
    pub(crate) lines: Span,             // new
    pub(crate) texts: Span,
}
```

`RenderBuffer::Default` clears `lines: Vec::new()`. Composer's
per-frame top-of-loop `out.quads.clear() / out.texts.clear() /
out.groups.clear()` grows by one `out.lines.clear()`.

### 5. Command stream — `src/renderer/frontend/cmd_buffer/mod.rs`

```rust
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmdKind {
    PushClip,
    PushClipRounded,
    PopClip,
    PushTransform,
    PopTransform,
    DrawRect,
    DrawRectStroked,
    DrawText,
    DrawLine,            // new
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawLinePayload {
    pub(crate) a: Vec2,
    pub(crate) b: Vec2,
    pub(crate) width: f32,
    pub(crate) color: Color,
}

impl RenderCmdBuffer {
    #[inline]
    pub(crate) fn draw_line(&mut self, a: Vec2, b: Vec2, width: f32, color: Color) {
        self.record_start(CmdKind::DrawLine);
        write_pod(&mut self.data, DrawLinePayload { a, b, width, color });
    }
}
```

`DrawLinePayload` is 36 B = 9 × `u32` in the arena, alignment 4 —
read via `bytemuck::pod_read_unaligned` like the existing
`DrawTextPayload` (which has higher alignment from its `u64` key).

### 6. Encoder — `src/renderer/frontend/encoder/mod.rs:127`

Replace the drop with a real emit. `Shape::Line` endpoints become
**owner-relative** (matching `RoundedRect::local_rect: Some` and
`Text::local_rect: Some` semantics; spinner authors `dir * r`
inside its leaf, encoder positions in window coords):

```rust
Shape::Line { a, b, width, color } => {
    let origin = owner_rect.min;
    out.draw_line(origin + *a, origin + *b, *width, *color);
}
```

Document the convention on `Shape::Line` itself (`shape.rs:26–31`)
when the change lands.

### 7. Composer — generalizing the painter-order rule

The current rule (`composer/mod.rs:74–81`) flushes the in-flight
group when a quad would land *after* a text in the cmd stream
(text draws are pipeline-deferred, so without the flush the quad
would paint underneath the text in the same group).

With three pipelines drawn per group in fixed painter order
**Quads → Lines → Text** (rationale below), the rule generalizes
cleanly. Replace `last_was_text: bool` with a `Last` enum tracking
the most recently emitted draw kind, and compute "would emitting
`next` violate painter order?" from a single ordinal table:

```rust
#[derive(Clone, Copy, Default)]
enum LastEmit { #[default] None, Quad, Line, Text }

const fn ordinal(k: LastEmit) -> u8 {
    match k { LastEmit::None => 0, LastEmit::Quad => 1, LastEmit::Line => 2, LastEmit::Text => 3 }
}

impl GroupBuilder {
    fn before_emit(&mut self, next: LastEmit, out: &mut RenderBuffer) {
        if ordinal(next) < ordinal(self.last_emit) {
            self.flush(out);
        }
    }
    fn push_quad(&mut self, …) { self.before_emit(LastEmit::Quad, out); …; self.last_emit = LastEmit::Quad; }
    fn push_line(&mut self, …) { self.before_emit(LastEmit::Line, out); …; self.last_emit = LastEmit::Line; }
    fn push_text(&mut self, …) { self.before_emit(LastEmit::Text, out); …; self.last_emit = LastEmit::Text; }
}
```

`flush` resets `last_emit = None`. `set_clip` calls `flush` and so
does the same. Fewer special-cases than the current
`last_was_text` pattern; existing behavior preserved on the Q/T
pair (text → quad still flushes).

**Why painter order Q → L → T per group?** Within one rounded-clip
group the backend binds three pipelines back-to-back. A natural
choice would be "whatever order matches authoring intent inside
the group," but the only viable per-group order is one fixed
sequence: opaque chrome (quads) under the text content (text),
with lines (decorations like underlines, focus rings, spinner
spokes) sandwiched between. Any other ordering forces extra
groups or breaks an existing case. The composer's job is to flush
groups whenever cmd order would fight that fixed pipeline order.

`DrawLine` handler in compose:

```rust
CmdKind::DrawLine => {
    let p: DrawLinePayload = cmds.read(start);
    let world_a = current_transform.apply_point(p.a);  // need TranslateScale::apply_point
    let world_b = current_transform.apply_point(p.b);
    // Clip-cull: bbox = AABB(a, b) inflated by width/2 + 1px AA.
    let bbox = bbox_of_segment(world_a, world_b, p.width);
    if let Some(active) = self.clip_stack.last() {
        let me = scissor_from_logical(bbox, scale, snap, viewport_phys);
        if me.intersect(active.scissor).is_none() {
            i += 1;
            continue;
        }
    }
    group.before_emit(LastEmit::Line, out);
    let phys_a = (world_a * scale).snap_if(snap);
    let phys_b = (world_b * scale).snap_if(snap);
    let phys_w = (p.width * current_transform.scale * scale).round().max(1.0);
    out.lines.push(LineInst::new(phys_a, phys_b, phys_w, p.color));
}
```

`TranslateScale::apply_point` is a small addition next to
`apply_rect` (translation + uniform scale on a `Vec2` — one
multiply-add). The `.max(1.0)` on physical width prevents a
sub-pixel line collapsing to zero (the AA fragment alpha scales
by `min(1.0, logical_width)` separately for "visually thinner" —
see open question §11).

### 8. Schedule + backend

`schedule.rs::RenderStep` gains one variant:

```rust
RenderStep::Lines { group: usize, range: Span },
```

`for_each_step` emits `Quads → Lines → Text` per group,
mirroring §7's painter order. The scissor and stencil bracketing
stay identical — lines participate in stencil-test like quads do
(no separate mask-write path).

`backend/mod.rs::WgpuBackend` grows a `line: LinePipeline` field,
constructed alongside `quad` in `WgpuBackend::new`. The
`render_groups` match adds:

```rust
RenderStep::Lines { range, .. } => {
    if use_stencil {
        self.line.bind_stencil_test(pass);
    } else {
        self.line.bind(pass);
    }
    self.line.draw_range(pass, range);
}
```

`upload` for the line pipeline runs from `WgpuBackend::submit`
once per frame, alongside the existing `quad.upload` call.

### 9. Encode-cache compatibility

`MeasureCache`, encode cache, and compose cache key on
`(WidgetId, subtree_hash, available_q)`. `Shape::Line`'s `Hash`
impl is already complete (`shape.rs:109–115`), so a leaf's
shape-rollup hash includes endpoints / width / color. No cache-key
changes; per-frame alpha mutation invalidates the spinner leaf
naturally — that's the desired behavior for an animating widget.

### 10. Tests

- `renderer/line.rs::tests`: `LineInst` size/offset pinning
  matching `quad_struct_is_68_bytes_no_padding`.
- `renderer/frontend/cmd_buffer/mod.rs::tests`: `draw_line` round-trips
  through `read::<DrawLinePayload>`.
- `renderer/frontend/encoder/tests.rs`: `Shape::Line` at known
  endpoints emits one `DrawLine` cmd at owner-relative-translated
  coordinates. Cover both the `local_rect: None` analog (since
  `Line` doesn't have one, just `a` / `b`) and a degenerate
  `a == b` (encoded; AA shader handles the disc fallback).
- `renderer/frontend/composer/tests.rs`:
  - Frame with one quad + one line + one text emitted in that
    order produces *one* group with all three ranges populated.
  - Cmd order text → line → quad produces three groups
    (painter-order regression triggers two flushes).
  - DPI smoke: physical width clamps to ≥ 1 px at scale = 2.0.
- `renderer/backend/tests.rs`: schedule emits
  `Quads → Lines → Text` per group; line range only emitted when
  non-empty.
- Showcase tab "Lines": grid of segments at varying angles /
  thicknesses / colors, including degenerate `a == b` (point
  capsule = filled disc).

---

## Spinner widget

Built on top of §A through §C. ~80 LOC, no new abstractions.

### API

```rust
ui.show(Spinner::new());                    // 24×24, theme defaults
ui.show(Spinner::new().size(48.0));         // explicit square size
ui.show(Spinner::new().style(custom));      // custom theme
ui.show(Spinner::new().paused(!loading));   // freeze phase + skip repaint
```

```rust
pub struct Spinner {
    element: Element,
    style: Option<SpinnerTheme>,
    paused: bool,
}

impl Spinner {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.size = Sizes::from(SpinnerTheme::DEFAULT_SIZE);
        Self { element, style: None, paused: false }
    }
    pub fn size(self, px: f32) -> Self { /* element.size = Sizes::from(px); */ }
    pub fn style(self, t: SpinnerTheme) -> Self { … }
    pub fn paused(self, p: bool) -> Self { … }
    pub fn show(&self, ui: &mut Ui);
}

impl Configure for Spinner {
    fn element_mut(&mut self) -> &mut Element { &mut self.element }
}
```

`Spinner::show` returns `()`, not `Response` — no hover/click
semantics by default (apps wrap in a `Button` if they want one).

### Theme — `widgets/theme.rs`

Added next to `ButtonTheme` / `ScrollbarTheme`:

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SpinnerTheme {
    pub spoke_count: u8,         // 12
    pub cycle_secs: f32,         // 1.0
    pub spoke_color: Color,      // palette::TEXT_MUTED
    pub head_alpha: f32,         // 1.0
    pub tail_alpha: f32,         // 0.15
    pub spoke_thickness: f32,    // 2.0 logical px
    pub inner_radius_ratio: f32, // 0.40 of half-extent
    pub outer_radius_ratio: f32, // 0.95
    pub easing: Easing,          // Easing::OutCubic on the head→tail ramp
}

impl SpinnerTheme {
    pub const DEFAULT_SIZE: f32 = 24.0;
}
```

Wire into `Theme::default()` and `Theme::spinner: SpinnerTheme`
the same way `button: ButtonTheme` is wired.

### Recording

The spinner's resolved size equals its `element.size = Sizes::Fixed(s)`
declaration regardless of parent (Fixed is hard-contract per the
architecture doc), so spoke geometry is fully determined at
authoring time. No prev-frame state. Endpoints are owner-local
(§6) — the encoder translates by `owner_rect.min`.

```rust
pub fn show(&self, ui: &mut Ui) {
    let style = self.style.clone().unwrap_or_else(|| ui.theme.spinner.clone());
    let element = self.element;
    let s = match element.size.w {
        Sizing::Fixed(v) => v,
        // Spinner::new always sets Fixed; .size() preserves Fixed.
        // A hypothetical Hug-spinner would need a measured-rect path.
        _ => unreachable!("spinner size must be Fixed; see docs/roadmap/spinner.md §C"),
    };
    let half = s * 0.5;
    let center = Vec2::splat(half);   // leaf-local
    let r0 = half * style.inner_radius_ratio;
    let r1 = half * style.outer_radius_ratio;
    let n = style.spoke_count as f32;

    let phase = if self.paused {
        0.0
    } else {
        (ui.time().as_secs_f32() / style.cycle_secs).rem_euclid(1.0)
    };

    ui.node(element, |ui| {
        for i in 0..style.spoke_count {
            let theta = std::f32::consts::TAU * (i as f32 / n);
            let dir = Vec2::new(theta.cos(), theta.sin());
            let delta = ((i as f32 / n) - phase).rem_euclid(1.0);
            let t = style.easing.apply(delta);
            let alpha = (1.0 - t) * style.head_alpha + t * style.tail_alpha;
            let color = Color { a: style.spoke_color.a * alpha, ..style.spoke_color };
            ui.add_shape(Shape::Line {
                a: center + dir * r0,
                b: center + dir * r1,
                width: style.spoke_thickness,
                color,
            });
        }
    });

    if !self.paused {
        ui.request_repaint();
    }
}
```

(`Color` carries no `with_alpha` helper — struct-update syntax
multiplies the theme's own alpha by the per-spoke ramp, which is
the desired behavior when a theme deliberately picks a
semi-transparent base color.)

Per frame: `spoke_count` (default 12) `Shape::Line` pushes onto
`Tree.shapes`. Zero allocations — `Vec<Shape>` reuses capacity. No
`StateMap` row, no `AnimMap` row — fully derived from `ui.time()`.

### Tests — `widgets/tests/spinner.rs`

Following CLAUDE.md's "extend existing tests" guidance, group as a
single table-driven test where it makes sense:

- 12 spokes at theme defaults emit 12 `Shape::Line` entries; the
  spoke at index `floor(N * phase)` carries `head_alpha`, alphas
  monotonically decrease modulo `N`.
- `paused(true)` emits the same shapes as `phase = 0` and does
  **not** flip `FrameOutput::repaint_requested`. Existing
  `ui/tests.rs:580–636` repaint-flag pattern fits — extend it
  with a `paused`/`!paused` axis.
- Advance `now` by `cycle_secs / N` between two frames — the
  highest-alpha spoke advances by exactly one slot.
- `Spinner::new().show(ui)` reports a square `DEFAULT_SIZE` rect;
  `.size(48.0)` reports a 48×48 rect — both via
  `Response::rect()`-equivalent on the leaf.
- DPI smoke: at scale = 2.0 the encoded `LineInst.width` is at
  least 1 physical px (anti-zero-width clamp from §7).

### Showcase

New `examples/showcase/` tab "Spinner":

- Default 24 px spinner.
- Sizes row (16 / 24 / 48 / 96 px) showing thickness scales sensibly.
- Inside a button (`<spinner> Loading…` row) — verifies layout
  composition and asymmetric padding.
- Slider for `cycle_secs` (0.4 → 4.0) and `spoke_count` (3 → 24).
- "Pause" toggle proving `paused(true)` collapses repaint requests
  (debug HUD's `repaint_requested` flag goes quiet).

---

## Order of work

Each step ships standalone with its own tests + showcase wiring.

1. **`Shape::Line` end-to-end** (§1–§10). The bulk of the work.
   Showcase tab "Lines" lands first to validate AA, snapping, and
   the painter-order generalization without spinner-specific
   code. `cargo bench` baseline: record 200 lines + 200 quads,
   ensure encode/compose stay alloc-free.
2. **`Ui::request_repaint()` + `Ui::time()` public** (§B). Trivial
   visibility flip + doc comments. Pin: calling `request_repaint`
   flips `FrameOutput.repaint_requested` for one frame and resets
   at the next `run_frame` (extends `ui/tests.rs:580–636`).
3. **`SpinnerTheme` + `Spinner` widget**. Pure composition over
   (1) and (2). Showcase tab "Spinner".

Steps 1 and 2 are independently useful (lines for debug overlays
and eventual underlines; `request_repaint` for any future
continuous animation) — even if step 3 changed shape we'd want
them.

## Non-goals

- **Determinate / progress variant.** Single moving arc with
  `progress: f32` is a different widget (`ProgressArc`). Add later
  once arc / pie shapes show up; don't conflate.
- **Pulse / breathing variants.** Same primitive (one `f32` phase)
  but different visual; add via a separate widget when there's a
  caller.
- **`Animatable` slot for phase.** Phase isn't a tween between two
  values — it's `ui.time()` mod cycle. `AnimMap` is the wrong tool.
- **Hug / Fill spinner sizing.** Forces a measured-rect read with
  F+1 settle for no real authoring win. Locked to Fixed by
  construction.
- **CPU-side tessellation of capsules.** A pre-tessellated
  triangle-mesh path would land via `mesh-shapes.md`; spinners
  don't justify it on their own.

## Open questions

- **Repaint cost when many spinners are visible.**
  `request_repaint` is global — N spinners idempotently set the
  same flag. But a tab full of 50 spinners means 50 leaf encode
  misses per frame from alpha mutation. Park; if it surfaces, a
  virtualization clamp or hoisting phase into a single
  `Animatable f32` consumed by every spinner removes the per-leaf
  miss.
- **Sub-frame phase precision.** `ui.time()` ticks at host frame
  rate. At 60 Hz with `cycle = 1.0 s` the head moves ~83 ms / slot
  (5 frames) — visually smooth. At very fast cycles (< 0.3 s) the
  alpha ramp's fractional `delta` carries the smoothness. Pin a
  visual test once motion is real.
- **Anti-aliased thin lines at fractional logical width.** The
  shader's half-pixel AA band assumes thickness ≥ 1 logical px.
  For `spoke_thickness < 1.0` the segment is sub-pixel and the
  AA region merges across both edges of the capsule — appears
  washed out. Fix when first reported: scale fragment alpha by
  `min(1.0, logical_width)` so a 0.5 px line draws at half opacity
  rather than ghosted. Plumbing: pass `logical_width` as a fifth
  vertex attribute alongside `width` (which is physical).
- **`TranslateScale::apply_point`.** Composer needs it for line
  endpoints (§7). Exists for `apply_rect` already — adding the
  point variant is mechanical but worth landing in step 1 so the
  helper is in place when the composer change drops.
- **Per-line bbox clip-cull.** §7's `bbox_of_segment` is a small
  helper (AABB of `a`, `b` inflated by `width/2 + 1`). Could
  arguably live on `Shape::Line` itself for reuse by the eventual
  damage system. Park until damage starts reading shape bboxes
  (`damage.md`).
