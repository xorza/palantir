# Paint-time animation on the public API

Design proposal for gap **G2** in `widget-public-api.md`, widened by the ask:
let a user write their **own** paint animation, not pick from a menu of two.

Nothing here is committed.

## What exists today

`Ui::add_shape_animated(shape, PaintAnim)` registers an animation the *encoder*
samples, one pass after record. The recorded subtree is byte-identical every
frame, so the widget never re-records and its layout cache entry survives. That
is the whole point of the mechanism: `Ui::animate` + `request_repaint` produces
the same pixels at the cost of a record pass per frame.

```rust
pub(crate) enum PaintAnim {
    BlinkOpacity { half_period, started_at, stop_after },
    Spin { speed, started_at },
}

pub(crate) struct PaintMod { alpha: f32, rotation: f32 }
```

### Two surprises worth stating before designing anything

**`alpha` is not a multiplier.** The encoder reads it as a gate:

```rust
let paint_mod = self.paint_anim_cursor.sample(shape_idx, self.now);
if noop_f32(paint_mod.alpha) { return; }        // layer_ctx.rs:127
```

Nothing multiplies it into the brush. `BlinkOpacity` only ever answers `0.0` or
`1.0`, and the code comment says fractional alpha "arrives with a future
`Pulse` variant". So today the channel is *hide or show*, not *fade*.

**`rotation` reaches three shape kinds.** It is consumed only where a
`StrokeBounds` is built — the polyline, curve and arc arms. A quad, an image or
a text run cannot be spun.

So the honest description of the current capability is: **hide or show any
shape, and spin a stroked one.**

### The four places a paint animation is read

| pass | call | what it decides |
|---|---|---|
| cascade `paint_rect.rs:100` | `anims.rotates(shape_idx)` | whether the damage bound is the recorded bbox or the swept square |
| damage `damage/mod.rs:437` | `anim.next_wake(prev)` | whether this shape's rect is re-damaged this frame |
| `post_record` `forest.rs:192` | `anim.next_wake(now)` | when the host is woken next |
| encoder `layer_ctx.rs:127` | `cursor.sample(shape_idx, now)` | the value |

Only the last one is the animation's *output*. The other three are the
framework asking questions it must answer correctly **or paint outside the
region it cleared**. Hold on to that — it decides the design.

## The ask splits in two

1. **What can be animated?** The channel vocabulary. Two channels today, one
   of them binary, one of them limited to stroked shapes.
2. **Who computes the value?** A closed enum today.

A design that answers only (2) ships "custom animations" that can blink and
spin. That is not the feature.

## Question 1 — the channels

| channel | cost | verdict |
|---|---|---|
| **alpha, fractional** | Mostly an encoder-side multiply: a solid quad fill, a text run and an image all carry a per-draw colour lane already (`GpuFill.color`, `DrawTextPayload::color`, `DrawImagePayload::tint`), so the encoder scales the alpha before the payload is built and no shader changes. Only a **gradient** fill needs shader work, and only in the two pipelines that resolve one (`quad.wgsl`, `curve.wgsl`) — where `GpuFill.color` is zeroed because the atlas row carries the colour, so that lane is free to become the multiplier. The mesh path was not checked. | **Do it.** This is what unlocks fade, pulse and breathe — most of what "custom animation" means in a UI. |
| **rotation on quads / text / images** | The quad pipeline draws axis-aligned rects; a rotation means a real transform through the vertex path. | Defer. Large, and the stroked case already covers spinners. |
| **translation** | The cascade would need the *swept union over the path*, which it cannot compute without sampling — and it deliberately answers `rotates()` without a `now`. A rotation is cheap because the swept cover is the same square at every angle. A translation has no such shortcut. | Defer. This is the expensive one, and it needs a design of its own. |
| **scale** | Same bound problem as translation. | Defer. |
| **colour tint** | Folds like alpha, but wants three more lanes rather than one free one. | Defer until asked. |

**Recommendation: make `alpha` a real multiplier, and ship the open authoring
form on top of the two channels there are.** Everything past that is a renderer
change, not an API change, and each can arrive later without breaking the
surface.

## Question 2 — the authoring form

### Option A — a richer closed enum

Grow `PaintAnim` with `Fade`, `Pulse`, `Marquee`. Cheap, keeps every invariant
in framework hands, and is not what was asked for.

### Option B — `Rc<dyn PaintAnimation>`

A trait with `sample`, `next_wake` and `rotates`. It has a precedent in this
crate: `Ui::gpu_view` already takes an `Rc<RefCell<dyn GpuPaint>>` handed in at
record time and called at paint time.

**Rejected, and the reason is the table above.** A `GpuPaint` that misbehaves
draws the wrong picture inside its own target. A `PaintAnimation` that answers
`rotates()` or `next_wake()` wrongly makes the framework damage the wrong
region — the shape paints outside what was cleared for it, and the artefact
lands on unrelated widgets. Two of the three trait methods are correctness
machinery, not output, and neither is checkable at any cost the frame can pay.

It also costs `Copy` on `PaintAnim` and `PaintAnimEntry`, which are today flat
rows in a per-frame `Vec`.

### Option C — a `fn` pointer for the whole animation

`fn(Duration) -> PaintMod` is `Copy` and cannot capture, so it is pure by
construction. It still puts `next_wake` and `rotates` in user hands, and it
cannot carry parameters — every "pulse between 0.3 and 0.8" needs its own
`fn` item.

### Option D — split the schedule from the shape (recommended)

**The framework owns when and what. The user owns the curve.**

```rust
/// Phase in `[0, 1)` → unit value in `[0, 1]`. A plain `fn`, so it is
/// `Copy`, allocation-free, and cannot capture — which is what makes it
/// pure without asking anyone to promise.
pub type PaintCurve = fn(f32) -> f32;

/// What the unit value drives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintChannel {
    /// Multiplies the shape's alpha, lerped `from` → `to`.
    pub alpha: Option<(f32, f32)>,
    /// Turns the shape about its owner box's centre, in full turns.
    pub turn: Option<(f32, f32)>,
}

/// When it runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaintTiming {
    pub started_at: Duration,
    pub period: Duration,
    pub repeat: PaintRepeat,
    /// How finely the curve is read. `Continuous` wakes every frame.
    /// `Steps(n)` holds one value per `1/n` of the period and wakes only
    /// on the boundaries — which is what keeps a blinking caret from
    /// asking for 120 frames a second.
    pub steps: PaintSteps,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaintRepeat {
    /// One pass, then hold the end value.
    Once,
    Forever,
    /// Repeat until this much has elapsed, then hold the end value.
    Until(Duration),
}

pub struct PaintAnim { channel: PaintChannel, timing: PaintTiming, curve: PaintCurve }
```

Sampling is `channel.at(curve(timing.phase(now)))`. The framework answers:

- `rotates()` from `channel.turn`, with no `now` and no user call — the same
  question the cascade asks today.
- `next_wake()` from `timing` alone — boundaries for `Steps(n)`, every frame
  for `Continuous`, `None` once it has settled.

Both invariants stay derivable. The user supplies only the part whose worst
case is a wrong picture.

The two shipped animations become curves the crate exports:

```rust
pub mod curves {
    pub fn linear(t: f32) -> f32;
    pub fn square(t: f32) -> f32;      // the caret blink
    pub fn sawtooth(t: f32) -> f32;    // the spinner
    pub fn sine(t: f32) -> f32;        // breathing
}
```

and a user's own is just a `fn`:

```rust
fn ease_out_back(t: f32) -> f32 { /* whatever they want */ }

ui.add_shape_animated(
    Shape::rect(r).fill(colour),
    PaintAnim::alpha(0.0, 1.0)
        .period(Duration::from_millis(240))
        .repeat(PaintRepeat::Once)
        .curve(ease_out_back),
);
```

The caret and the spinner then read as ordinary uses of the same surface:

```rust
PaintAnim::alpha(0.0, 1.0)
    .period(blink_period)
    .steps(2)
    .repeat(PaintRepeat::Until(idle_cutoff))
    .curve(curves::square)

PaintAnim::turn(0.0, 1.0)
    .period(Duration::from_secs_f32(TAU / speed))
    .curve(curves::sawtooth)
```

## Where the entry point goes

**On `Ui`, not on `Shape`.** My earlier note suggested `Shape::blink(..)` /
`Shape::spin(..)` builders. That was wrong once the animation became a value:
carrying an `Option<PaintAnim>` on every shape grows `ShapeRecord`, which is
pinned at 88 bytes by `hot_struct_sizes.rs` and paid per recorded shape per
frame. The animation belongs beside the shape, not inside it, so
`Ui::add_shape_animated(shape, anim)` becomes `pub` as it stands.

## What it costs

| piece | size |
|---|---|
| **stage one — fractional alpha** | see the breakdown below |
| `PaintChannel` / `PaintTiming` / `PaintRepeat` / `PaintSteps` / `PaintCurve` and the builder | a day |
| rewrite `BlinkOpacity` and `Spin` as uses of it, delete the enum | half a day — the existing tests in `paint_anims/tests.rs` pin the behaviour and should survive unchanged |
| publish the four types, `curves`, and `Ui::add_shape_animated` | documentation |

Staged: fractional alpha first, standing on its own with the existing enum,
because it is the renderer half and it is what the feature is worth. The
authoring form second.

### Stage one, in detail

**Encoder only, no shader.** Every one of these already carries the colour in
its payload, so the encoder scales the `RgbaF16` alpha on the way in:

| payload | lane |
|---|---|
| `DrawTextPayload` | `color` |
| `DrawImagePayload` | `tint` |
| `DrawIconPayload` | `tint` |
| `DrawMeshPayload` | `tint` |
| `DrawQuadPayload` | `fill` (solid), `stroke` |
| `DrawCurvePayload` | `fill` (solid) |

**Shader, two files.** A gradient fill takes its colour from the atlas row, and
a polyline takes its vertex colours from a span in the record store — neither
is in the payload to scale. Both get the same one-line multiply:

- `quad.wgsl`, gradient branch. `GpuFill.color` is zeroed for a gradient, so
  that lane is free to carry the multiplier.
- `curve.wgsl`, gradient branch and the polyline colour read. `DrawPolylinePayload`
  gains one `alpha: f32` lane, because its colours are a span it does not own.

**Provably inert for existing content.** Every scene in the visual suite paints
at alpha 1, so the whole change must leave the golden images byte-identical.
The suite is the test.


## What it does not cover

- **One animation per shape.** `PaintAnimEntry` keys on a strictly increasing
  `shape_idx`, so a second entry for one shape breaks the cursor's invariant.
  `PaintChannel` carrying both alpha and turn under one curve covers the common
  pair; two independent timings on one shape do not fit.
- **Anything needing a swept bound** — translation, scale.
- **An animation whose schedule is genuinely irregular.** If one ever turns up,
  `PaintAnim` gains an `Rc<dyn PaintAnimation>` variant additively, and that
  variant carries the documented burden of answering `rotates` and `next_wake`
  honestly. Do not ship it before something needs it.

## Decisions

1. **Fractional alpha ships first**, standing on its own with the existing
   enum. Without it the first custom animation anyone writes can only blink.
2. **One animation drives both channels under one curve.** `PaintChannel`
   carries `alpha` and `turn` as independent `Option<(f32, f32)>` ranges, so
   fade-and-spin together works and the entry stays `Copy` and flat.
3. `curves` ships as a module of free `fn`s. A curve is a function value and
   has nowhere else to sit.
