# State & events

Today, per-widget state lives in `StateMap` (`WidgetId → Box<dyn Any>`),
keyed positionally and dropped on the same `removed` sweep as
`MeasureCache` / encode / compose / `TextMeasurer`. That covers
*intra-widget* state — scroll offsets, text cursors, animation phase —
and nothing else.

What's missing is a story for state whose lifetime or producer is not
the widget itself: a background task streaming results, a domain model
shared by several widgets, an event one widget emits that another
reacts to. Today the only escape hatches are "pass `&mut Model` into
`run`" and "stash an `Arc<Mutex<…>>` in `StateMap`." Both work for toy
apps and break down once you have more than one producer or more than
one observer.

## Why this matters

Immediate-mode authoring is a great fit when the frame *is* the truth.
It stops being a fit when the truth lives outside the frame —
filesystem watchers, network responses, debouncers, undo stacks,
selection that survives a re-record, results from a worker thread. egui
hit this and grew `memory.data`, channels, `ctx.request_repaint`, and
ad-hoc persisted ids. GPUI sidestepped it by going reactive end-to-end
(`Entity<T>` + `cx.observe` / `cx.subscribe`), at the cost of two
authoring models stacked on top of each other.

Neither end is right for Palantir. We don't want a reactive layer
bolted on top of an immediate-mode recorder — that's the worst of both
worlds (two mental models, view-vs-element split, lifecycle bugs at
the seam). We also don't want to discover the egui escape hatches one
at a time.

## What we want

A small, opinionated **event/store primitive** keyed off `WidgetId` the
same way `StateMap` is, with the same `removed`-sweep eviction. Frame
loop is unchanged; widgets still pull state in `show()`. The primitive
adds:

- **Stores** — values keyed by `WidgetId` (or a domain key) with a
  monotonic revision. `ui.store::<T>(id)` returns `(value, rev)`; the
  rev folds into the property tracker so encode cache invalidates
  cleanly.
- **Events** — typed bus: `ui.emit(id, ev)` enqueues, `ui.drain::<E>(id)`
  consumes during record. Cleared at frame end. No bubbling, no
  capture — explicit producer / consumer ids.
- **External writes** — `Ui::handle()` hands out a `Send` token that
  worker threads use to push values into a store and request a repaint.
  Drained into stores at frame start, before record.
- **Persistence** — opt-in serde on a store; survives process restart
  via the same `removed`-sweep contract (only persist what's still
  alive).

Crucially, **none of this changes how widgets are written.** A widget
still does `Button::new().show(ui)`. The store/event API is what you
reach for when state outlives one widget or one frame.

## What it solves

- **Cross-widget state** — selection, filter text, "currently open
  document" — without `Arc<Mutex<…>>` smuggling.
- **Background work** — file watcher, HTTP, syntax highlight worker —
  push into a store, frame picks it up, no polling.
- **Undo/redo** — store revs make a natural snapshot point.
- **Persisted UI** — collapsed sections, last-selected tab — without a
  bespoke save/load path per widget.
- **Cache correctness** — folding store revs into the property tracker
  means subtree-skip stays valid when external state changes.

## What it explicitly is not

- Not reactive views. No `Entity<T>`, no `View<T>`, no two-tier
  authoring.
- Not signals / observables / dependency tracking. Producers and
  consumers name each other by id.
- Not a global app context object threaded through every call. The
  primitive lives on `Ui`, like `StateMap` does.

## Open questions

- Granularity of the event bus: per-`WidgetId` queue, per-type queue,
  or both? Per-type is simpler; per-id avoids fan-out scans.
- Whether stores should be addressable by domain key (`"open-doc"`)
  in addition to `WidgetId`, for state that has no natural owner
  widget.
- Interaction with `request_discard` (invalidation roadmap): a store
  write mid-frame may want to re-record before paint.
- Threading model for `Ui::handle()` — channel? `Arc<Mutex<Queue>>`?
  Lockfree ring? Decide once we have a real workload.

Park until a real workload demands it (showcase tab with a worker
thread, or text-edit's undo stack, whichever lands first). Premature
design here would calcify around the wrong shape.
