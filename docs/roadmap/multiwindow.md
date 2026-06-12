# Multi-window

Let one `WinitHost` drive **N top-level OS windows**, each its own
independent UI tree (`Ui` + `WindowRenderer`), all sharing one GPU device. The
target use is a multi-document / tear-off-panel editor (darkroom opening
a second document window, a detached tool palette as a real OS window) —
*not* one logical UI whose sub-regions escape into separate windows.

Status: **Slices 1–4 landed** (shared `Gpu`, window map + per-`WindowId`
routing + token-aware `App::frame`, in-frame `Ui::open_window` /
`close_window`, and the **one shared `WgpuBackend`** — per-format pipeline
map + shared atlases/caches/arena/shaper). The showcase opens a second
"inspector" window on F8. Everything below `WindowRenderer` —
`Forest`/`Tree`, layout, `CascadesEngine`, `DamageEngine`, every widget —
is window-agnostic and was untouched.

## Model: N independent UI trees, one shared device

Two different things get called "multi-window"; this committed to the
first and explicitly shelves the second.

- **Model A (built) — N windows, each its own `WindowRenderer`.** Every window is a
  separate document: its own `Ui` (input / focus / layout / `Display`),
  its own `WgpuBackend`, all built from one shared `wgpu::Instance` /
  `adapter` / `Device` / `Queue`. Maps almost verbatim onto the existing
  `WindowRenderer` boundary (one `WindowRenderer` already == one logical UI).

- **Model B (shelved) — one `Ui` spanning multiple OS windows**
  (egui-style "viewports"): a `ComboBox` dropdown or a detached toolbar
  that escapes the main window's bounds but stays in the same widget
  tree, input routing, and focus model. This needs per-viewport input,
  a single `InputState` split across surfaces (today: one `pointer_pos`,
  one `focused`, one `hovered` per `Ui`), a single `Display` split across
  sizes/DPRs, and a multi-root `Forest`. High cost, no motivating
  workload — **too early.** Revisit only when a concrete feature (an
  overflowing popup, a detach-but-stay-linked panel) demands it.

The realistic darkroom needs (separate document / tool windows) are all
Model A. Model B stays a separately-justified future project.

## Why it's cheap (the two non-problems)

1. **The GPU device is already shared by clone.** wgpu's `Device` /
   `Queue` are `Arc`-backed handles — `window_renderer.rs:97-109` already `.clone()`s
   them into `Ui`, `Frontend`, and `WgpuBackend`. Surfaces created from
   the *same* `Instance` / `adapter` share one device for free; no `Arc`
   wrapper, no second device. The only catch the refactor fixed: `resumed`
   used to build the `Instance` + `adapter` locally and **drop them** — now
   they're retained on a `Gpu` struct so window #2 can be created later.

2. **The UI engine doesn't know windows exist.** `Forest`/`Tree`,
   measure/arrange, `CascadesEngine`, `DamageEngine` all run against a
   single `Display` rect with zero window awareness (already per-`Layer`,
   not per-window). Model A reuses them unchanged; N `WindowRenderer`s == N
   independent engines, no shared mutable state between windows.

## Where single-window lived (now removed)

- `RuntimeState` **was** the per-window state — one `window` / `surface` /
  `device` / `config` / `renderer` / `scale_factor` / `next` — with no map
  of them. Now `windows: HashMap<WindowId, WindowState>`.
- `window_event(&mut self, _id: WindowId, …)` ignored the `WindowId`; it
  now routes on it.
- `Instance` + `adapter` were local to `resumed` and dropped; now on `Gpu`.
- `App::frame(&mut self, ui)` had no window notion; now
  `frame(&mut self, win: WindowToken, ui)`.
- `WindowRenderer::frame(surface, config, display, …)` was already parameterized by
  surface/config/display — it paints whatever it's handed, so it needed
  **no change**; it's just called once per dirty window.

## App contract: one app, window-aware frame

A single `App` owns all windows' content and switches on a
**caller-chosen opaque token**.

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowToken(pub u64);

pub trait App {
    fn frame(&mut self, win: WindowToken, ui: &mut Ui);
}
```

The token is supplied at `Ui::open_window` (and `WinitHost::new` for the
first window), handed back to `frame` each paint, and names a window in
`Ui::close_window` / `HostHandle::request_repaint`. The app owns the
semantics — use it as an enum discriminant, an index, a document-id hash.
Palantir only stores it in `WindowState` and compares it; winit's opaque
`WindowId` stays internal (event routing) and never reaches the app.

The alternative identities considered: raw winit `WindowId` (app can't
correlate until it has seen the id, which fights fire-and-forget open) and
a typed associated `type Window` on the `App` trait (compiler-checked
exhaustive matching, but threads a generic through `WinitHost`/`WindowState`).
The opaque `u64` token is the sweet spot — zero generics, app-defined
meaning, and it makes open fire-and-forget because you named the window
yourself. The app-per-window factory (stronger isolation, a second
lifecycle) is also rejected; the single-app form keeps cross-window state
(shared selection, a global undo stack) trivially reachable as `&mut self`.

## Config split: per-window vs app-global

`Ui::open_window` takes a **`WindowConfig`** — backend-agnostic (no winit
or wgpu types), just `title` + optional `inner_size` / `min_inner_size`
as `UVec2` logical pixels (the same integer-extent type `Display` uses;
re-exported as `palantir::UVec2`). Opening a window from app code
therefore doesn't pull the windowing backend into the `Ui` API.

The app-global GPU knobs — adapter `power_preference`, swapchain
`present_mode`, `collect_gpu_stats` — are fixed once at launch and shared
by every window, so they live on **`WinitHostConfig`** (which embeds a
`WindowConfig` for the first window). Secondary windows inherit them; they
*can't* meaningfully vary per window (the adapter and device are chosen
once). `present_mode` is stored on `Gpu` and applied to every surface.

## Migration plan

Four slices. Each landed green (single-window kept working;
per-frame-alloc and visual goldens stayed clean) before the next.

### Slice 1 — hoist the GPU context out of per-window state

Pulled `Instance` + `adapter` + `Device` + `Queue` (+ `present_mode` +
`collect_gpu_stats`) into a `Gpu` struct on `WinitHost`, built once on the
first `resumed`. Surface creation + the sRGB-format pick moved into
`Gpu::make_surface` / `configure_surface` (returning a named
`WindowSurface`, no tuple). The adapter-probe surface is reused as the
first window's swapchain via `GpuInit { gpu, first_surface }` rather than
recreated.

### Slice 2 — window map + per-`WindowId` event routing

`Option<RuntimeState>` → `HashMap<WindowId, WindowState>` (`WindowState`
== the old `RuntimeState` minus the GPU handles). Every handler routes on
the incoming `WindowId`: `window_event` looks up `windows[&id]`; `draw`
takes a `WindowId`; `about_to_wait` folds **every** window's `next` into
one `ControlFlow` (nearest `At(t)` wins, any `Immediate` self-requests its
redraw, all-`Idle` → `Wait`, applying the `At(t) <= now → Immediate` fold
per-window first); `CloseRequested` removes from the map and only
`exit()`s when it empties. `App::frame` gained the `WindowToken` param.

### Slice 3 — in-frame `Ui` window lifecycle

Window creation is a **UI authoring action** ("the user clicked *Open
Inspector*"), not a background concern — so it lives on `Ui`, callable
only during `App::frame` (which already runs on the event-loop thread).
There is **no off-thread window-creation path** and therefore no
cross-thread `WindowId` round-trip to design around.

```rust
impl Ui {
    pub fn open_window(&mut self, token: WindowToken, config: WindowConfig);
    pub fn close_window(&mut self, token: WindowToken);
}
```

These don't create the window inline — we're mid-`host.frame`, deep in
the tree walk with the backend borrowed. They **enqueue** onto retained
scratch on `Ui` (`pending_windows` / `pending_closes`, capacity reused so
steady-state stays alloc-free). `WinitHost` **drains** the queues in
`about_to_wait` (the one callback that always holds `&ActiveEventLoop`
after event processing): for each open, `event_loop.create_window`
synchronously → `WindowId` materializes on the same thread, same tick →
build surface + `WindowRenderer` against the shared `Gpu` → insert into the map →
request first redraw. Draining there (not only in `draw`) also catches an
`open_window` issued from inside a `run_on_main` closure, which is
serviced in `user_event`. The borrow trap: collect requests out of the
per-window queues *before* calling `create_window`, since creation
inserts into the same `windows` map.

The split that falls out:

| | where | thread | affects |
|---|---|---|---|
| `Ui::open_window` / `Ui::close_window` | in `App::frame` | event-loop | **structure** — the window set |
| `HostHandle::request_repaint(token)` / `run_on_main(token, …)` | anywhere | any | **pokes** — existing windows |

So the `UserEvent` proxy keeps only `Repaint` / `RunOnMain` (now
token-tagged) + `Quit` — **no** `OpenWindow` / `CloseWindow` variants. A
background thread that wants a new window does what every other
immediate-mode state change does: set a flag (channel / `run_on_main`),
`request_repaint`, and next frame `App::frame` calls `ui.open_window`.

`CloseRequested` and `Ui::close_window` share one removal path;
`event_loop.exit()` only when the map empties.

Pin: `window_requests_queue_and_survive_the_frame` (in `ui/tests.rs`)
checks the requests survive the frame that filed them + a quiet frame, so
the host can drain them after the fact.

### Slice 4 — shared GPU resources (**landed**)

`WgpuBackend` became the **one** shared GPU renderer for all windows,
owned by `WinitHost` and passed `&mut` into every `WindowRenderer::frame`.
`WindowRenderer` slimmed to per-window state only: its `Ui`, a per-window
`Frontend` (CPU encode/compose scratch — this window's draw list), the
persistent `Backbuffer`, and the frame-scheduling/occlusion clock. So N
windows render through one device, one set of atlases, one shaper.

What's shared (one instance on the backend, cloned into each window's
`Ui`/`Frontend`):

- **CPU-side caches + frame arena + shaper + GPU-stats handle.**
  `RenderCaches` (image registry + gradient atlas), `FrameArena`,
  `TextShaper`, and `GpuPassStats` are built once in `WgpuBackend::new`
  and cloned via `make_window_ui` / `make_frontend`. (Previously each
  window built its own — even `TextShaper::with_bundled_fonts()` per
  window.) Safe under the **serialized-render invariant**: winit delivers
  one `RedrawRequested` at a time, so one window completes record → submit
  before the next clears the shared arena.
- **GPU-resident atlases + image cache.** Glyph atlas (`TextBackend`),
  gradient LUT atlas (`GradientResources`), and the image texture cache
  (`ImagePipeline.cache`) live on the backend's format-independent
  resource structs — content-keyed, one upload serves every window.
- **Render pipelines** — extracted into `FormatPipelines` (the only
  format-dependent state: the `wgpu::RenderPipeline` objects) and held in
  a `HashMap<TextureFormat, FormatPipelines>`, **built lazily per format**
  on first submit (`WgpuBackend::ensure_format`). Windows on
  different-format outputs (one sRGB, one HDR) each get their own set
  while sharing every atlas/buffer — no thrash, no per-window duplication.
  The stencil-test pipeline twins are now built eagerly per format
  (dropping the lazy `ensure_stencil` state machine).

What stays per-window — genuinely surface-bound:

- The persistent damage **backbuffer** (one surface's pixels for
  `LoadOp::Load`); recreates on size *or* format change (self-heal).
- The per-window `Frontend` scratch (this window's `RenderBuffer`).
- The surface config + scheduling clock.

The `staging_belt`, `viewport_size`, and `gpu_timings` live on the shared
backend (transient/per-submit; serialized, so one suffices).

**Possible follow-ups:** the per-window `Frontend` could be shared too
(it's serialized scratch); `WgpuBackend` could be renamed `WgpuRenderer`
now that it's the renderer rather than a per-window backend; GPU
instrumentation reflects the last-submitted window (key per-window if it
matters).

## Trade-offs

**Pro.**

- Reuses the `WindowRenderer` boundary verbatim; the hard parts (layout, cascade,
  damage, input) need zero changes.
- GPU device shared for free via wgpu's clone semantics — no device-loss
  coordination, no cross-window sync.
- Each window is independently damage-tracked and scheduled; an idle
  inspector window sleeps while the document window animates.

**Con.**

- Per-window backends duplicate font/gradient/image uploads and pipeline
  compiles until Slice 4 (shared GPU resources).
- `about_to_wait` reduces N scheduling states into one `ControlFlow` — a
  subtle spot. A window stuck in `Immediate` busy-loops the whole loop;
  the `At(t) <= now → Immediate` fold has to apply per-window before the
  reduction.
- No shared input/focus across windows (that's Model B). Dragging a value
  from one window into another, or a single focus ring spanning windows,
  is out of scope by construction.

## Open questions

- **Per-window scale factor & monitor refresh.** `draw` requeries
  `current_monitor().refresh_rate_millihertz()` per frame; with N windows
  on different monitors this Just Works per-window since each `WindowRenderer`
  carries its own `Display`. Confirm the per-window `next` scheduling
  paces each window to its own monitor, not a global min.
- **Token ergonomics.** `WindowToken(u64)` is decided; revisit a typed
  associated `type Window` only if the switch-on-token in `App::frame`
  gets unwieldy in darkroom (the `u64` loses compiler-checked
  exhaustiveness). Duplicate-token `open_window` is a caller bug —
  `spawn_window` warns-and-skips rather than inserting a second window the
  token can't unambiguously address.
- **Reaching the real `winit::Window`.** Set-title / focus / position /
  drag all need the `winit::Window`, which the app never sees (it only
  holds tokens). Expose a `window(token) -> Option<&Window>`-style
  accessor keyed by token when a consumer needs it — not in the first cut.
- **`run_on_main` targeting.** Token-tagged: the closure sees that one
  window's `&mut Ui`. A "run with access to everything" variant can come
  later if cross-window mutation needs it.
- **Exit policy.** Last-window-closed → exit is the obvious default, but a
  headless/background app might want to outlive its windows (reopen on a
  tray click). Make it a `WinitHostConfig` flag if a consumer needs it.
