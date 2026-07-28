# Keyboard routing: who gets this key?

Working note. Investigation started from one hack in darkroom; the
conclusion is a palantir-side design that **replaces the existing capture
system** rather than sitting beside it. Resolved, not scheduled.

Supersedes the earlier revision of this file, which proposed scopes as an
addition layered on top of `modal_layer`. Two mechanisms doing one job is
the thing this is trying to stop.

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

It also **over-fires today**. `shortcuts.rs:86` gates the whole body of
`apply_canvas_shortcuts`, so Ctrl+D (duplicate) and Ctrl+0 (reset zoom) are
dead whenever any field is focused — even though `TextEdit` binds neither.
A live bug, found while sanity-checking the design below.

## 2. It is a class, not an instance

Two darkroom workarounds share one root cause — plus a third that looks
like one and isn't:

| Site | Question it needs answered | What it does instead |
|---|---|---|
| `dock/mod.rs:302` `typing_focus_held` | does the focused widget consume typed keys? | subtract pane ids from `focused_id()` |
| ~~`dock/mod.rs:315` `drop_target`~~ | which pane is the pointer inside? | **not actually broken** — see below |
| `widgets/inline_rename.rs:206` | is this Escape/Enter mine? | read them globally, rely on an unenforced "only one rename is active" invariant |

`drop_target` was in this table for a revision and does not belong. It
hand-rolls `rect.contains(p)` and its doc says "deliberately *not*
`hover_within`", which reads like the same workaround — but the argument it
gives is sound: *panes tile the dock area without overlapping, so plain
containment against last-frame rects is exact*. It also needs the pane rect
anyway, to classify the drop zone. A pointer-geometry question answered with
pointer geometry. (It could be `hover_within` if panes took `Sense::HOVER` —
that hit-test is **not** capture-suppressed, `refresh_pointer_targets:945`
sets `input.hovered` from the raw hit regardless of a live drag, unlike
`ResponseState::hovered`. It just buys nothing over exact containment.)

The other two are one problem.

### Root cause

Palantir's keyboard input is a **broadcast stream with exactly one arbiter,
and that arbiter is whole-stream and layer-granular.**
`InputState::keyboard_events_for` (`input/mod.rs:510`) gates a reader only
on layer order:

```rust
(Some(capture), Reader::Unclaimed(layer)) => capture.layer.idx() <= layer.idx(),
```

Every reader on the top layer sees every key, so per-chord arbitration must be
hand-written by the app. Focus is a single `Option<WidgetId>` behind a single
`focusable` bit (`scene/node/columns.rs:211`), so it serves two unrelated
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
- `Watches.keys: Vec<Shortcut>` (`input/watch.rs:97`) — a per-chord,
  per-pass, deduped subscription list, used only for the wake-gate. Carries no
  owner.

And the discipline is already argued for in-tree: the doc comment at
`input/mod.rs:558-568` defends deferred end-of-pass resolution over live
recomputation on order-independence grounds — the same argument Dear ImGui
makes for its routing system.

**The capture system already _is_ a routing system.** It has exactly one scope
tree, hard-coded: `Layer`. Five nodes, totally ordered, gate is arithmetic on
that order. The design in §5 makes that tree real and demotes `Layer` to its
outermost tier — it does not add a second tree next to it.

## 3. Prior art

Reference clones under `.tmp/` (`scripts/fetch-refs.sh`).

### egui

Solves it twice, in both directions.

- **The focused widget declares what it takes.** `EventFilter`
  (`crates/egui/src/data/input/event_filter.rs`) rides on
  `FocusWidget { id, filter }`; `InputState::filtered_events(&filter)`
  hands the widget only matching events. The default is the interesting
  part: every key returns `true` — exclusive to the focused widget —
  **except** Tab/arrows/Escape, which fall through unless opted into.
  `TextEdit` opts into both arrow axes; `Slider` into horizontal only.

  Consequence worth knowing: egui ships "focused widget eats everything but
  navigation keys", and egui apps live with Ctrl+S not reaching the app
  mid-edit. Coarse absorb is survivable in practice. §5.1 declines that
  particular trade — `ACCEL` stays out of the text filter.

- **Structurally, egui never creates the problem.** Containers are never
  focusable, and "which region was pressed" is a *separate axis* — area/layer
  ordering, `memory.areas().layer_id_at(pos)`. Two questions, two answers.

- **Where it does need darkroom's predicate, it uses a type probe, not id
  subtraction.** `Context::text_edit_focused()` loads `TextEditState` for the
  focused id and checks presence. Note `egui_wants_keyboard_input()` right
  above it is just `focused().is_some()` — the naive version darkroom cannot
  use.

### Dear ImGui

Went furthest; this got a dedicated subsystem in 1.89–1.90 after years of
ad-hoc `WantCaptureKeyboard`. Two layers.

- **Key ownership** (`imgui_internal.h:3594`): *"instead of 'eating' a given
  input, we can link to an owner id."* `SetKeyOwner` / `TestKeyOwner`; a query
  passes if the key is unowned **or** owned by you. Mouse buttons are keys, so
  pointer rides the same machinery.

- **Shortcut routing** (`imgui.h:1073`): `Shortcut(Ctrl+S)` *registers a route
  request*; all requests resolve in `NewFrame()`; exactly one owner is granted.
  Scoring (`CalcRoutingScore`) is **depth in the focus path** — deepest focused
  scope wins, ancestors are the fallback.

The canonical example (`imgui.h:1082`) is darkroom's case verbatim:

```
Parent   -> call Shortcut(Ctrl+S)    // When Parent is focused, Parent gets the shortcut.
  Child1 -> call Shortcut(Ctrl+S)    // When Child1 is focused, Child1 overrides Parent.
  Child2 -> no call                  // When Child2 is focused, Parent gets the shortcut.
```

And the property called out as load-bearing: *"The whole system is order
independent… This is an important property as it facilitates working with
foreign code or larger codebase."* That falls out of register-then-resolve
rather than first-come-reads.

Also relevant: **focus scope** (`imgui_internal.h:3646`) — "used to identify a
unique input location", one per window automatically, *and the default route
owner when none is given*. One concept serving both jobs. §5 takes that shape.

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

### B. Per-chord route table (imgui-shaped) — rejected

Promote `Watches.keys` to carry `(chord, owner, Route)`; resolve at pass start
into a grants vec. The most precise option, and the wrong shape here:
`key_pressed` grows an owner and a policy argument (34 call sites), newly
appearing widgets get a frame of registration lag, `modal_layer`'s claim has
to be taught about the table separately, and menu accelerators need a
dedicated tier to beat a focused editor.

### C. Scope-gated stream — chosen

Extend the existing capture gate: let the app add nodes to the scope tree that
`Layer` currently hard-codes.

The wall a naive version hits — `if focused { ui.capture() }` — is that stream
gating is **all-or-nothing per scope**. Focus in a node-title field, user
presses Ctrl+R: the field holds the keyboard, the pane is cut off, and Run is
silently swallowed. It also doesn't work as written, because the gate is `<=`:
a same-layer claim silences nothing on its own layer, and tightening it to `<`
re-breaks `TextEdit`-inside-`Popup`, the regression the `<=` comment at
`input/mod.rs:501` documents.

Both are fixed by giving each scope a **filter** (egui's `EventFilter`, moved
off the focused widget onto the scope) and walking outward until one matches.

#### C1: filter the claim, keep `modal_layer` — rejected

A cheaper variant: leave `claims` alone, add a second single-slot
`typing: Option<TypingOwner { layer, id, filter }>` written only by focused
text fields, and filter `key_pressed_for` against it. ~60 lines, no
`NodeFlags` bit, no path walk.

It works, and it fixes `typing_focus_held`. It was rejected for being a
**third parallel mechanism** — overlay claims arbitrate by layer, text fields
by class, and neither knows about the other. Adding a third axis to arbitrate
input is worse than making the one that already exists real, whatever it
costs in diff.

#### C2: scopes replace the capture system — **chosen**

`modal_layer`'s claim *is* a scope with `KeyFilter::ALL`; a focused field is a
scope with `TEXT_FIELD`. One list, one resolution, one gate — and `InputClaim`
with its release protocol disappears. The scope tree earns itself inside
palantir, replacing the overlay machinery; darkroom only names a root.

## 5. The design

### 5.1 `KeyClass` / `KeyFilter`

Every press classifies into exactly one class. The classifier is an exhaustive
`match` over `Key`, not a catch-all, so a new key variant fails to compile
until it is re-homed.

```rust
/// What kind of thing a key press *is*. Exactly one class per press.
pub enum KeyClass {
    /// Printable characters and bare Enter. Only a text field wants these.
    Text,
    /// The clipboard/undo family plus the destructive edit keys:
    /// Ctrl+Z/X/C/V/A, Delete, Backspace. The contested class — a text
    /// field and a canvas both want it, and deciding between them is the
    /// whole problem this solves.
    Edit,
    /// Caret movement, or canvas nudge: arrows, Home/End, PgUp/PgDn, Tab.
    Motion,
    /// Escape alone. Its own class because cancel is hierarchical — the
    /// innermost thing that can be cancelled should be.
    Escape,
    /// Everything else: command chords outside the edit family, and the
    /// function keys. Ctrl+S, Ctrl+R, F12. Commands, never editing.
    Accel,
}
```

Command chords match on the **physical** key — the same fallback
`Shortcut::matches` uses — so Ctrl+Z is `Edit` on a layout whose logical Z is
Cyrillic. The edit-chord set (`z x c v a`) is the one hand-maintained list;
`widgets/text_edit/tests.rs` pins it by asserting every `EditAction::shortcut()`
classifies as `Edit`, so a seventh edit action that forgets to extend it fails
a test instead of silently becoming an accelerator.

```rust
bitflags! {
    /// The classes a scope takes while it is on the active path.
    pub struct KeyFilter: u8 { /* one bit per KeyClass */ }
}

impl KeyFilter {
    /// A focused text field. `ACCEL` is **absent**: Ctrl+S and Ctrl+R fall
    /// through to the app while the user is typing. That omission is the
    /// entire reason this is a filter and not a capture, and it is what
    /// the route table's dedicated global tier (§4 B) bought at more cost.
    pub const TEXT_FIELD: Self = Self::TEXT.union(Self::EDIT)
                                           .union(Self::MOTION)
                                           .union(Self::ESCAPE);
}
```

`bitflags` is already a dependency (`Sense` uses it).

### 5.2 Declaring a scope

A builder, not a block — no extra nodes, no recorder stack. The scope *path*
is the ancestor chain the cascade already computes.

```rust
impl Configure {
    /// Make this node an input scope taking `takes` while it is active.
    ///
    /// Scopes nest. A press walks the active path deepest-first and is
    /// granted to the first scope whose filter contains its `KeyClass`;
    /// scopes further out never see it.
    ///
    /// Deliberately **not** focus: a scope is where input *belongs*,
    /// focus is where typing *goes*. Conflating them is what forces an
    /// app to reconstruct one from the other.
    fn input_scope(self, takes: KeyFilter) -> Self;
}
```

Cost: one `NodeFlags` bit (bit 9; `FOCUSABLE` at bit 8 unchanged, 10–15 stay
free) plus a `KeyFilter` byte in the cascade entry beside `sense` / `focusable`.

### 5.3 Resolution

At record-pass start — focus is already committed by then
(`input/mod.rs:759`):

```
active_layer = topmost layer declaring any scope
anchor       = focused widget, else last-press widget, if either is in active_layer
path         = scopes in active_layer containing anchor, deepest-first
               (just the layer's outermost scope when there is no anchor)
```

`active_layer` is what replaces `keyboard_claim.layer` and `pointer_claim`.
Layers are separate trees (`Cascades::is_within`: *"a popup is never within
its anchor"*), so a path never spans them — which is not a limitation to work
around, it *is* today's layer gate restated. A popup declaring a scope makes
`Popup` the active layer and the `Main` tree stops seeing keys, exactly as
`modal_layer` does now. The no-anchor clause is what keeps a popup owning
input when nothing inside it is focused.

**No parent pointers needed.** Per layer, keep the scope entries' pre-order
node indices; "scopes containing N" are those whose `[node, subtree_end)`
interval covers N — the same interval test `is_within` already runs. Deepest
is the largest index. Scope count is single digits, one pass, retained `Vec`,
no per-frame allocation.

- **No registration lag** — the path derives from focus and the cascade, not
  from last frame's calls.
- **Order-independent** — it does not depend on where in the pass anything
  recorded.

### 5.4 Reading

```rust
impl Ui {
    /// Unchanged signature. Now true only when the chord was granted to
    /// the scope enclosing this record position — or, outside every
    /// scope, to the active layer's outermost one.
    pub fn key_pressed(&mut self, sc: Shortcut) -> bool;
    pub fn escape_pressed(&mut self) -> bool;

    /// Withdraw a scope recorded this pass, so the resolution at its end
    /// does not see it. The pass you call it in is unaffected —
    /// resolution is deferred for order-independence, so an overlay that
    /// decides it is closing must say so or it owns input for one frame
    /// after it is gone. The only survivor of `InputClaim`'s lifecycle.
    pub fn close_scope(&mut self, id: WidgetId);
}
```

**"Outside every scope reads as the outermost scope" is what removes the
`in_scope` hatch** the earlier revision needed. Darkroom's chord handling runs
in the navigation phase and records nothing; it resolves as the app root,
which is exactly where those chords belong.

`keyboard_events` keeps today's layer gating and is **not** class-filtered.
It returns `&[KeyboardEvent]`, a borrowed slice of the frame buffer;
partitioning by class would break the arrival order `TextEdit`'s drain depends
on. The only wholesale drainer is `TextEdit`, reading inside its own scope, so
nothing needs the filtered form.

`wants_text_input()` (host-side IME / on-screen keyboard), if it ever lands,
becomes "the deepest scope's filter contains `TEXT`" — no separate node flag.

### 5.5 What gets deleted

| today | after |
|---|---|
| `Ui::modal_layer(layer, anchor, size, owner, body)` | `.input_scope(KeyFilter::ALL)` + `Ui::overlay_layer` |
| **`InputClaim`** — `keyboard_events` / `key_pressed` / `escape_pressed` / `pointer_events` / `release` | **gone.** An overlay reads `ui.escape_pressed()`; outside its body that resolves to its own scope |
| `PopupHandle::{keyboard_events, key_pressed, escape_pressed}` | forward to plain `Ui` methods |
| `claims` / `InputOwner` / `keyboard_claim` / `pointer_claim` | one scope list + the resolved path |
| `Reader::{Owner, Unclaimed}` | gone — every read resolves by scope |
| `dock::typing_focus_held` | gone |

Five public methods and a public type collapse to one lifecycle call. That is
the argument for replacing rather than extending: it removes a lifecycle
problem class — `InputClaim::release`'s twelve-line doc about owning input for
one frame after the overlay is gone — instead of adding a second one beside it.

### 5.6 What darkroom becomes

Two declarations, total:

```rust
// MainWindow root, once. Everything except TEXT — a canvas has no typing.
Panel::vstack().id(APP_ROOT).input_scope(KeyFilter::all() - KeyFilter::TEXT)

// TextEdit, in palantir
.input_scope(KeyFilter::TEXT_FIELD)
```

**Panes are not scopes**, and that is what removes every remaining call-site
change. Darkroom's chord reads are all app-level; *which graph* an edit
targets already comes from `focused_target()`, never from key routing. So the
path is two entries deep and `apply_undo_redo` / `apply_canvas_shortcuts` /
`menu_shortcut` keep their bodies **unchanged**, minus the two
`typing_focus_held` early-returns. `typing_focus_held` is deleted.

`scan_focus` and `drop_target` are **untouched**. Panes stay
`.focusable(true)`: the side effect §1 complains about — `focused_id()` reading
`Some` essentially always — stops mattering the moment nothing consults focus
to answer "is the user typing". Focus still serves two jobs; it just no longer
has to be un-served by hand.

```rust
// inline_rename: read the editor's own signals, not `ui` — a focused field
// has claimed Enter (Text) and Escape (Escape), so polling here sees nothing
let (submitted, cancelled) = {
    let edit = TextEdit::new(&mut draft).id(id)/* … */.show(ui);
    (edit.submitted, edit.cancelled)
};
```

`TextEditResponse::submitted` **already exists** (`text_edit/input.rs:206`,
`mod.rs:382`). `cancelled` is new but its value is already computed —
`InputResult.blur` is set only by `KeyOutcome::Blur`, which only Escape
produces (`input.rs:265`); today it folds into `lost_focus` via
`request_focus(None)` and is indistinguishable from clicking away.

Trace, focus in a node-title field — path
`[field(TEXT_FIELD), app_root(ALL−TEXT)]`:

| chord | class | granted to |
|---|---|---|
| `Ctrl+Z` | Edit | field — text undo, not document undo |
| `Delete` | Edit | field — deletes a char, not the node |
| `Escape` | Escape | field — cancels the rename; canvas keeps its selection, breaker keeps its scribble |
| `Ctrl+R`, `Ctrl+S` | Accel | app_root — Run and Save fire mid-edit |
| `Ctrl+D`, `Ctrl+0` | Accel | app_root — **currently dead** while typing (§1) |

With nothing focused the path is `[app_root]` and everything lands on the app.
No predicate anywhere.

## 6. Grounding: the actual chord set

Darkroom binds 13 chords, all static consts in one file
(`shortcuts.rs:22-35`, plus Enter/Escape/Delete/Backspace). Cross-referenced
against `EditAction::shortcut` (`text_edit/action.rs:68`) and `apply_key`
(`text_edit/input.rs:246`):

| chord | class | `TextEdit` binds it |
|---|---|---|
| Ctrl+Z, Ctrl+Shift+Z | Edit | **yes** — `EditAction::Undo`/`Redo` are character-identical to darkroom's consts |
| Delete, Backspace | Edit | **yes** |
| Escape | Escape | **yes** |
| Enter (`inline_rename`) | Text | **yes** — single-line submit |
| Ctrl+N/O/S/Shift+S/R/Q/0/D | Accel | no |

Contested set: **five chords plus Enter** — exactly what `typing_focus_held`
gates. `TextEdit` also binds Ctrl+A/X/C/V, which darkroom does not bind at
all. Nothing straddles two classes. Coarse classes are precise enough here
because the whole set is small, static, and read from one place.

## 7. Migration

1. `KeyClass` / `KeyFilter` + classification tests. Inert.
2. `SCOPE` bit, cascade column, path resolution. Inert — nothing declares a
   scope yet.
3. `key_pressed` switches to the path; `modal_layer` becomes an internal
   `input_scope(ALL)`; `InputClaim` deleted; `Popup` / `Modal` / `ComboBox` /
   `ContextMenu` / `Tooltip` migrated. **Behaviour-neutral by construction** —
   one scope per overlay reproduces the layer gate exactly.
4. `TextEdit` declares `TEXT_FIELD`; add `TextEditResponse::cancelled`. First
   behaviour change.
5. Darkroom: one app-root scope, delete `typing_focus_held`, rewrite
   `inline_rename` onto `submitted` / `cancelled`.

Steps 1–3 land and verify alone.

## 8. Costs

- Largest diff of the four directions, though it is concentrated: every
  overlay widget in palantir, and in darkroom only the app root, `shortcuts.rs`
  and `inline_rename`. The dock is untouched.
- One-frame staleness does not disappear, it gets one honest name:
  `close_scope`. Deferred resolution is what buys order-independence
  (`input/mod.rs:558-568`); staleness is its price either way. A scope that
  simply stops recording releases automatically — `close_scope` is only for
  "I decided I am closing *after* I already recorded", which is precisely
  `Popup`'s and `Modal`'s shape.
- The scope tree is not uniform: scopes nest within a layer, layers order
  between them. Worth stating rather than discovering.

## 9. Open questions

- **`Motion` may want splitting.** Tab-as-focus-traversal and arrows-as-caret
  are one class today only because palantir has no focus traversal. When it
  gains one, Tab needs to sit above the path entirely. Too early to build;
  this is the seam.
- **`close_scope` on an id that isn't a scope** does nothing, silently.
  `debug_assert!` against last frame's cascade — a typo'd id is otherwise an
  overlay that quietly keeps input for a frame.
- **Two scopes on one node** — allow, one filter. No case yet for more.
- **Pointer `captures`** (per-button press/release/drag) stay untouched and
  orthogonal to scopes. ImGui unifies them — mouse buttons *are* keys, one
  ownership table — and nothing here pushes on it yet.

### Resolved since the last revision

- ~~`Enter` is in `Text`, so a single-line field's submit needs an escape
  hatch~~ — `TextEditResponse::submitted` already ships; `cancelled` is the
  symmetric half. No hatch.
- ~~`in_scope` on an id that isn't a scope grants nothing, silently~~ —
  `in_scope` no longer exists (§5.4).
