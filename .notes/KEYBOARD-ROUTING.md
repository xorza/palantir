# Keyboard routing: who gets this key?

Working note. Investigation started from one hack in darkroom; the
conclusion is a palantir-side design proposal. Not accepted, not
scheduled.

## 1. The trigger

`darkroom/src/gui/dock/mod.rs:302`:

```rust
pub(crate) fn typing_focus_held(ui: &Ui, doc: &Document) -> bool {
    ui.focused_id()
        .is_some_and(|id| !doc.layout.groups().any(|g| pane_wid(g.id) == id))
}
```

It answers "is a text field being edited?" by **set subtraction against the
dock's pane ids** — anything focused that isn't a pane container is assumed
to be typing. Callers: `shortcuts.rs:54` (Ctrl+Z/Y stand-down),
`shortcuts.rs:86` (Esc / Delete / Ctrl+D / Ctrl+0 stand-down).

Why it exists: panes are recorded `.focusable(true)` (`dock/mod.rs:249`)
purely so palantir's left-press focus hit-test routes dock focus for
`scan_focus` (`dock/mod.rs:279`). That side effect makes `ui.focused_id()`
`Some` essentially always, so the natural test — "something is focused, don't
steal the chord" — degenerates to "always" and every graph-level chord dies.

Why it's fragile: correctness rests on an unwritten global invariant that its
own doc comment admits — the pane container is darkroom's *only* non-text
focusable. Mark any other widget `.focusable(true)` (a list, a scroll region —
`widgets/scroll/mod.rs:327` already propagates the flag) and undo/Delete
silently go dead while it holds focus. No compile error, no test failure.

## 2. It is a class, not an instance

Three darkroom workarounds share one root cause:

| Site | Question it needs answered | What it does instead |
|---|---|---|
| `dock/mod.rs:302` `typing_focus_held` | does the focused widget consume typed keys? | subtract pane ids from `focused_id()` |
| `dock/mod.rs:315` `drop_target` | which pane is the pointer inside? | hand-rolled `rect.contains(p)` — its doc says "deliberately *not* `hover_within`", because hover resolves only to *sensing* widgets and pane content is inert |
| `widgets/inline_rename.rs:206` | is this Escape/Enter mine? | read them globally, rely on an unenforced "only one rename is active" invariant |

### Root cause

Palantir's keyboard input is a **broadcast stream with exactly one arbiter,
and that arbiter is whole-stream and layer-granular.**
`InputState::keyboard_events_for` (`src/input/mod.rs:510`) gates a reader only
on layer order:

```rust
(Some(capture), Reader::Unclaimed(layer)) => capture.layer.idx() <= layer.idx(),
```

Every reader on the top layer sees every key, so per-chord arbitration must be
hand-written by the app. Focus is a single `Option<WidgetId>` behind a single
`focusable` bit (`src/scene/node/columns.rs:210`), so it serves two unrelated
jobs — *where typed keys go* and *which region the user last reached into*.
Darkroom needs the second, palantir offers it only via the first, and darkroom
subtracts the first back out.

Compounding it: `TextEdit` gates on `is_focused` then drains
`keyboard_events(layer)` wholesale (`widgets/text_edit/input.rs:191`). It eats
every key while focused and tells nobody.

### What palantir already has

Two mechanisms that are 80% of a routing system:

- `claims: Vec<InputOwner { layer, id }>` resolved once in `finish_record`
  (`input/mod.rs:569`) — a route table with one route ("all keys") and one
  scoring dimension (layer).
- `Watches.keys: Vec<Shortcut>` (`input/watch.rs:104`) — a per-chord,
  per-pass, deduped subscription list, used only for the wake-gate. Carries no
  owner.

And the discipline is already argued for in-tree: the doc comment at
`input/mod.rs:558-568` defends deferred end-of-pass resolution over live
recomputation on order-independence grounds — the same argument Dear ImGui
makes for its routing system.

**The capture system already _is_ a routing system.** It has exactly one scope
tree, hard-coded: `Layer`. Five nodes, totally ordered, gate is arithmetic on
that order.

## 3. Prior art

Reference clones under `.tmp/` (`scripts/fetch-refs.sh`).

### egui

Solves it twice, in both directions.

- **The focused widget declares what it takes.** `EventFilter`
  (`crates/egui/src/data/input/event_filter.rs`) rides on
  `FocusWidget { id, filter }`; `InputState::filtered_events(&filter)`
  (`input_state/mod.rs:907`) hands the widget only matching events. The
  default is the interesting part (`event_filter.rs:50`): every key returns
  `true` — exclusive to the focused widget — **except** Tab/arrows/Escape,
  which fall through unless opted into. `TextEdit` opts into both arrow axes;
  `Slider` into horizontal only.

  Consequence worth knowing: egui ships "focused widget eats everything but
  navigation keys", and egui apps live with Ctrl+S not reaching the app
  mid-edit. Coarse absorb is survivable in practice.

- **Structurally, egui never creates the problem.** Containers are never
  focusable, and "which region was pressed" is a *separate axis* — area/layer
  ordering, `memory.areas().layer_id_at(pos)`. Two questions, two answers.

- **Where it does need darkroom's predicate, it uses a type probe, not id
  subtraction.** `Context::text_edit_focused()` (`context.rs:2889`) loads
  `TextEditState` for the focused id and checks presence. Note
  `egui_wants_keyboard_input()` right above it (`context.rs:2884`) is just
  `focused().is_some()` — the naive version darkroom cannot use.

### Dear ImGui

Went furthest; this got a dedicated subsystem in 1.89–1.90 after years of
ad-hoc `WantCaptureKeyboard`. Two layers.

- **Key ownership** (`imgui_internal.h:3594`): *"instead of 'eating' a given
  input, we can link to an owner id."* `SetKeyOwner` / `TestKeyOwner`; a query
  passes if the key is unowned **or** owned by you. Auto-released the frame
  after key release. Mouse buttons are keys, so pointer rides the same
  machinery.

- **Shortcut routing** (`imgui.h:1073`, `imgui_internal.h:3627`):
  `Shortcut(Ctrl+S)` *registers a route request*; all requests resolve in
  `NewFrame()`; exactly one owner is granted and the grant calls
  `SetKeyOwner`. Scoring (`imgui.cpp:9766 CalcRoutingScore`) is **depth in the
  focus path** (`NavFocusRoute`): active item = 300, else
  `199 - index_in_focus_path` — deepest focused scope wins, ancestors are the
  fallback. Policies `RouteFocused` (default) / `RouteActive` / `RouteGlobal` /
  `RouteAlways`, plus `RouteOverFocused` / `RouteOverActive`.

The canonical example (`imgui.h:1082`) is darkroom's case verbatim:

```
Parent   -> call Shortcut(Ctrl+S)    // When Parent is focused, Parent gets the shortcut.
  Child1 -> call Shortcut(Ctrl+S)    // When Child1 is focused, Child1 overrides Parent.
  Child2 -> no call                  // When Child2 is focused, Parent gets the shortcut.
```

And the property called out as load-bearing: *"The whole system is order
independent, so if Child1 makes its calls before Parent, results will be
identical. This is an important property as it facilitates working with
foreign code or larger codebase."* That falls out of register-then-resolve
rather than first-come-reads.

Also relevant: **focus scope** (`imgui_internal.h:3646`) — "used to identify a
unique input location", one per window automatically, *and the default route
owner when none is given*. One concept serving both jobs.

### Browsers (retained contrast)

`document.activeElement` + capture/bubble along the ancestry chain +
`preventDefault()`. ImGui's focus-path scoring is the immediate-mode
restatement of DOM bubbling, made order-independent by deferring resolution to
a fixed point in the frame.

## 4. Directions considered

### A. Consumption / eating — rejected

`ui.take_key(sc) -> bool`, first reader in record order wins. Dead simple, no
scoring, no scope stack.

Fatal here for a specific reason: darkroom's chord reads happen in the
navigation phase, *before* the dock records, so the app would always win over
any `TextEdit` — the exact bug inverted. Also the order-dependence imgui
explicitly designed away from.

### B. Per-chord route table (imgui-shaped) — viable, not chosen

Promote `Watches.keys` to carry `(chord, owner, Route)`. Resolve at record-pass
start against the current focus path into a small grants vec;
`ui.shortcut(owner, sc, route)` registers for next pass and returns this pass's
grant.

```rust
pub enum Route { Focused, Global, GlobalOverTyping, Always }
```

Scoring: `GlobalOverTyping` 400, `claim_typing` wildcard 300, `Focused` at
depth *d* on the focus path `200 - d`, `Global` 1, off-path 0.

Works, and is the most precise option. Costs: `key_pressed` grows an owner and
a policy argument (34 call sites across palantir + darkroom); one frame of
registration lag for newly-appearing widgets; `modal_layer`'s claim has to be
taught about the table separately; needs a dedicated tier so menu accelerators
beat a focused editor.

### C. Scope-gated stream (chosen)

Extend the existing capture gate instead of adding a table: let the app add
nodes to the scope tree that `Layer` currently hard-codes.

The wall a naive version hits: stream gating is **all-or-nothing per scope**.
Focus in a node-title field, user presses Ctrl+R — the field's scope holds the
keyboard, the pane is cut off, and Run is silently swallowed. Fixed by giving
each scope a *filter* (egui's `EventFilter`, moved from the focused widget onto
the scope) and walking the focus path outward until one matches.

## 5. Proposal

### 5.1 `KeyClass` / `KeyFilter`

Every chord classifies into exactly one class. The classifier is an exhaustive
`match`, not a catch-all, so adding a class fails to compile until every chord
is re-homed.

```rust
/// What kind of thing a chord *is*. Exactly one per `Shortcut`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyClass {
    /// Printable characters, IME commits, Enter. Only a text field wants these.
    Text,
    /// The clipboard/undo family: Ctrl+Z/X/C/V/A, Delete, Backspace.
    /// The contested class — a text field and a canvas both want it, and
    /// which one gets it is the whole problem this system solves.
    Edit,
    /// Arrows, Home/End, PgUp/PgDn, Tab. Caret movement, or canvas nudge.
    Motion,
    /// Escape alone. Its own class because cancel is hierarchical: the
    /// innermost thing that can be cancelled should be.
    Escape,
    /// Everything else — modified chords and function keys. Ctrl+S, Ctrl+R,
    /// F12. Application commands, not editing.
    Accel,
}

bitflags! {
    /// Which classes a scope takes while it is the active one.
    pub struct KeyFilter: u8 { /* one bit per KeyClass */ }
}

impl KeyFilter {
    /// A focused text field. `ACCEL` is **absent**: Ctrl+S and Ctrl+R fall
    /// through to the app while the user is typing, which is the behavior
    /// that otherwise needs a special global tier.
    pub const TEXT_FIELD: Self = Self::TEXT.union(Self::EDIT)
                                          .union(Self::MOTION)
                                          .union(Self::ESCAPE);
}
```

### 5.2 Declaring a scope

A builder, not a block — so no extra nodes and no recorder stack. The scope
*path* is the ancestor chain the cascade already computes.

```rust
impl Configure {
    /// Make this node a keyboard scope taking `takes` while it is active.
    ///
    /// Scopes nest. The **active scope** is the innermost scope containing
    /// the focused widget, or — when nothing is focused — the innermost
    /// scope the last left-press landed in. A pressed chord walks the
    /// active scope's ancestor chain deepest-first and is granted to the
    /// first scope whose filter contains its class; scopes below that point
    /// never see it.
    ///
    /// Deliberately **not** focus: a scope is where input *belongs*, focus
    /// is where typing *goes*. Conflating them is what forces an app to
    /// reconstruct one from the other.
    fn input_scope(self, takes: KeyFilter) -> Self;
}
```

Cost: one new `NodeFlags` bit (bit 9; `focusable` at bit 8 unchanged) plus a
`KeyFilter` byte in the cascade entry. Bits 10-15 stay free.

The two-clause definition of "active scope" is deliberate. Making a press set
*focus* to the enclosing scope would collapse it to one clause and reintroduce
the original hack.

### 5.3 Reading

```rust
impl Ui {
    /// Unchanged signature. Now returns `true` only if the chord was granted
    /// to the scope currently being recorded.
    pub fn key_pressed(&mut self, sc: Shortcut) -> bool;

    /// Unchanged signature. Now yields only events granted to the current
    /// scope — so a focused `TextEdit` draining this wholesale no longer
    /// silently eats the app's accelerators.
    pub fn keyboard_events(&self) -> &[KeyboardEvent];

    /// Read as `id`'s scope without recording inside it. For a reader that
    /// acts on a region it does not draw — an app's per-frame chord handling
    /// that runs before the tree records.
    ///
    /// Enters an existing scope; it does not declare one. Reading as an id
    /// that was not recorded with `input_scope` grants nothing.
    pub fn in_scope<R>(&mut self, id: WidgetId, body: impl FnOnce(&mut Ui) -> R) -> R;

    pub fn active_scope(&self) -> Option<WidgetId>;

    /// Innermost scope containing `pos`, topmost-first — through the same hit
    /// index as every other hit-test, so occlusion, clipping and layers come
    /// along. Answers for a scope whose content senses nothing, which
    /// `hover_within` cannot.
    pub fn scope_at(&self, pos: Vec2) -> Option<WidgetId>;

    /// The scope the last left-press landed in. Preserved on a press that
    /// hits no scope, so chrome clicks leave the current region alone.
    pub fn pressed_scope(&self) -> Option<WidgetId>;
}
```

`wants_text_input()` (host-side IME / on-screen keyboard) becomes "the active
scope's filter contains `TEXT`" — no separate node flag needed.

### 5.4 Resolution

At record-pass start — focus is already committed by then
(`input/mod.rs:759`) — walk from the active scope to the root into a retained
`path: Vec<(WidgetId, KeyFilter)>`, cleared with capacity kept.
`key_pressed` classifies the chord, scans `path` deepest-first for the first
matching filter, grants iff that is the recording scope. Depth is single
digits; a handful of bit tests per call, no per-frame allocation.

- **No registration lag** — the path derives from focus, not from last frame's
  calls.
- **Order-independent** — the path does not depend on where in the pass
  anything recorded.
- **Composes with `modal_layer` unchanged** — the layer gate runs first, the
  scope walk runs within the surviving layer.

### 5.5 What darkroom becomes

```rust
// pane
Panel::vstack()
    .id(pane_wid(group.id))
    .input_scope(KeyFilter::EDIT | KeyFilter::ESCAPE | KeyFilter::MOTION)

// app root, once, in MainWindow
Panel::vstack().id(APP_ROOT).input_scope(KeyFilter::ACCEL)

// TextEdit, in palantir
.input_scope(KeyFilter::TEXT_FIELD)
```

```rust
// Editor::frame navigation phase — records nothing, so it enters the scope
ui.in_scope(pane_wid(open.document.layout.focused), |ui| {
    self.apply_undo_redo(ui, open);
    self.apply_canvas_shortcuts(ui, open);
});
```

`apply_undo_redo` and `apply_canvas_shortcuts` are **unchanged inside** — same
`ui.key_pressed(UNDO_SHORTCUT)` calls, minus the two `typing_focus_held`
early-returns. `typing_focus_held` is deleted.

```rust
// inline_rename: read Enter/Escape as the field, not as the pane —
// otherwise the TextEdit's scope has already taken them
let (enter, escape) = ui.in_scope(id, |ui| {
    (ui.key_pressed(Shortcut::key(Key::Enter)), ui.escape_pressed())
});
```

```rust
// scan_focus / drop_target — exact id match, no ancestry scan, no rect test
let scope = ui.pressed_scope()?;              // or ui.scope_at(p)? for the drop
let group = doc.layout.groups().find(|g| pane_wid(g.id) == scope)?;
```

Trace, focus in a node-title field inside pane A — path
`[app_root(ACCEL), pane_a(EDIT|ESCAPE|MOTION), field(TEXT_FIELD)]`:

| chord | class | granted to |
|---|---|---|
| `Ctrl+Z` | Edit | field — text undo |
| `Delete` | Edit | field — deletes a char, not the node |
| `Escape` | Escape | field — cancels the rename, canvas keeps its selection |
| `Ctrl+R` | Accel | app_root — Run fires mid-edit |
| `Ctrl+S` | Accel | app_root — Save fires mid-edit |

With nothing focused, path is `[app_root, pane_a]` and `Ctrl+Z` / `Delete` /
`Escape` all land on the pane. No predicate anywhere.

Grounding for "coarse classes are precise enough here": darkroom binds **13
chords total**, all static consts in one file (`shortcuts.rs:22-35`, plus
Enter/Escape/Delete).

## 6. B vs C

| | B: route table | C: scopes |
|---|---|---|
| `key_pressed` signature | changes; 34 sites migrate | **unchanged** |
| new palantir state | route vec + prev vec + grants vec + resolve | one path vec |
| new `NodeFlags` bits | 2 | 1 |
| registration lag | one frame for new widgets | none |
| `modal_layer` | taught separately | same mechanism |
| global accelerators | dedicated `GlobalOverTyping` tier | falls out of the outermost scope |
| precision | per chord, per reader | per class, per scope |
| new failure mode | forgetting a frame's subscription | wrapping the wrong block |

The decisive points: `key_pressed` keeps its signature, and leaving `ACCEL` out
of `TEXT_FIELD` gets accelerator-beats-typing for free — which was the main
thing the route table bought, and it bought it with a special rung.

## 7. Open questions

- **`Motion` may want splitting.** Tab-as-focus-traversal and arrows-as-caret
  are one class today only because palantir has no focus traversal. When it
  gains one, Tab needs to sit above the focus path entirely. Too early to
  build; this is the seam.
- **`Enter` is in `Text`.** Right for a multiline editor, workable for
  commit-on-Enter via `in_scope` — but that means a single-line field's submit
  is read through an escape hatch. Cleaner fix is `TextEdit` surfacing a
  `submitted` flag on its response; separate change, and it would delete the
  `in_scope` call in `inline_rename`.
- **`in_scope` on an id that isn't a scope** grants nothing, silently.
  `debug_assert!` against last frame's cascade — a typo'd id is otherwise a
  chord that just stops working.
- **Two scopes on one node** — allow, one filter. No case yet for more.
- **Pointer routing.** ImGui unifies this (mouse buttons *are* keys, one
  ownership table). Palantir's pointer path has `captures` + `pointer_claim`
  and was not audited here. Out of scope until a real case pushes on it.
