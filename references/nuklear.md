# Nuklear — reference notes for Palantir

Notes from reading `tmp/nuklear/nuklear.h` (single file, ~31k lines). Nuklear is the C-99, single-header, zero-dep cousin of Dear ImGui by vurtun. Same immediate-mode call-site idiom, but a *very* different middle: a **command-buffer** (typed draw ops, not vertices) and a **row-based** layout system that reserves fixed slots up-front instead of advancing a free cursor. Worth studying because two of those choices map directly onto Palantir's `Shape` arena and `HStack` driver.

All paths below are line numbers in `tmp/nuklear/nuklear.h`.

## 1. Single-header C, no allocations the user didn't approve

The whole library is one `.h`. `#define NK_IMPLEMENTATION` once before including pulls in the bodies; otherwise just declarations. There is no libc dependency by default — `NK_INCLUDE_DEFAULT_ALLOCATOR`, `NK_INCLUDE_STANDARD_IO`, `NK_INCLUDE_STANDARD_VARARGS` and friends are all opt-in feature gates. Backends (GL2/3/4, D3D9/11/12, SDL, GLFW, X11, allegro, …) live in `tmp/nuklear/demo/` — none of them are part of the core. The user supplies a `struct nk_user_font` (`text_width` callback + line height) and a draw loop that walks the command buffer; Nuklear itself never calls a graphics API.

Initialization comes in three flavors that pin the memory model (`nuklear.h:615/640/660/680`): `nk_init_default` (libc malloc), `nk_init` (caller's `nk_allocator` callbacks), `nk_init_fixed` (one preallocated block — *no* allocations after init), and `nk_init_custom` (separate buffers for command list and the page pool). The fixed-size variant is the one vurtun's blog repeatedly highlights as the point: you can ship a UI that allocates exactly zero bytes per frame.

## 2. Command buffer: typed shape ops, not vertices

The output of a frame is a *linked list of typed commands*, not a vertex/index pair. `enum nk_command_type` (`nuklear.h:4682`) enumerates 18 ops — `NK_COMMAND_SCISSOR`, `RECT`, `RECT_FILLED`, `CIRCLE`, `ARC`, `TRIANGLE`, `POLYGON`, `POLYLINE`, `TEXT`, `IMAGE`, `CUSTOM`, etc. Each is a struct that starts with a `struct nk_command { type, next }` header (`4705`) and carries its own payload (`nk_command_rect_filled` at `4745` is `{header, rounding, x, y, w, h, color}` — pixel coords, not vertices). Variable-length commands (`nk_command_polygon` at `4812`) use the C99 flexible-array trick (`nk_vec2i points[1]`).

`struct nk_command_buffer` (`4870`) is just `{base: nk_buffer*, clip, use_clipping, userdata, begin, end, last}` — an offset-pair into a shared `nk_buffer`. `nk_command_buffer_push` (`9281`) bumps `nk_buffer_alloc(b->base, NK_BUFFER_FRONT, size, align)` (`9293`), patches `cmd->next = allocated + alignment` (`9306`), and stores `b->last` so the *previous* command's `next` field can chain through. The whole command list is a singly-linked list embedded inside one byte buffer — iteration is `nk__begin` → `nk__next` (`19692`/`19714`), pointer-walking via `nk_ptr_add(struct nk_command, buffer, cmd->next)` (`19723`).

Each window owns its own `nk_command_buffer` (`5792`). At end of frame, `nk_build` (`19631`) splices them into one global linked list by overwriting the `next` of each window's `last` command to point at the next window's `begin` — popups are appended after, and the cursor `overlay` buffer is patched on at the end. A backend either:

- walks `nk__begin/nk__next` and issues a backend draw per command type (the GL2 demo does this — naïve immediate-mode), or
- calls `nk_convert` (`10984`, optional `NK_INCLUDE_VERTEX_BUFFER_OUTPUT`), which iterates the command list and tessellates into `nk_buffer` triples `{cmds, vertices, elements}` containing `nk_draw_command { elem_count, clip_rect, texture, userdata_id }` plus interleaved verts/indices. That's the path the GL3+/D3D11/wgpu-style backends take.

Scissor is a real command (`NK_COMMAND_SCISSOR`, emitted by `nk_push_scissor` at `9314`) — there's no separate "clip stack on top of vertex output," every clip change is a new command in the list, so when the backend converts to draws it just splits ranges at scissor commands.

## 3. Layout: row-based, slots reserved before you submit a widget

This is the part that's *not* like ImGui. ImGui has one cursor, you submit widgets, the cursor advances. Nuklear has rows — you declare a row's shape *first*, then submit `cols` widgets into it, each consuming one slot.

`enum nk_layout_format` (`509`) is `{NK_DYNAMIC, NK_STATIC}` — ratios summing to 1 vs absolute pixels. `enum nk_panel_row_layout_type` (`5663`) is the internal mode flag: `DYNAMIC_FIXED`, `DYNAMIC_ROW`, `DYNAMIC_FREE`, `DYNAMIC`, `STATIC_FIXED`, `STATIC_ROW`, `STATIC_FREE`, `STATIC`, `TEMPLATE`. `struct nk_row_layout` (`5675`) holds `{type, index, height, columns, ratio*, item_width, item_offset, filled, item: Rect, templates[16]}` — basically the cursor state for *the current row*.

The public API:

- `nk_layout_row_dynamic(ctx, height, cols)` (`22185`) — `cols` equal-width slots, each `panel_w / cols`. Mode `DYNAMIC_FIXED`.
- `nk_layout_row_static(ctx, height, item_width, cols)` (`22190`) — `cols` slots all of fixed pixel width. Mode `STATIC_FIXED`.
- `nk_layout_row(ctx, fmt, height, cols, ratio[])` (`22267`) — explicit ratios array; `ratio[i] < 0` marks "share leftover" and `nk_row_layout` distributes `(1 - sum) / n_undef` (`22288-22295`).
- `nk_layout_row_begin/_push/_end` (`22195/22221/22247`) — push slots one at a time when you don't know `cols` up front.
- `nk_layout_row_template_*` (`22305+`) — *the* interesting one: each slot is declared `static(px)`, `dynamic` (share leftover equally), or `variable(min_px)` (share leftover but never below `min_px`). This is essentially flex `{flex-basis, flex-grow, flex-shrink}` for one row, expressed in C.
- `nk_layout_space_begin` (`22444`) — free placement mode; `nk_layout_space_push(rect)` puts a widget at an absolute or ratio rect inside the row band.

The placement work happens in `nk_layout_widget_space` (`22614`): given the row mode and `row.index`, compute item width and offset, then `bounds = { x: at_x + item_offset + item_spacing, y: at_y - offset_y, w: item_width, h: row.height - spacing.y }` (`22740-22746`). `nk_panel_alloc_space` (`22749`) is what every widget actually calls — it bumps `row.index` and rolls over to the next row when `index >= columns` via `nk_panel_alloc_row`. So a widget body is roughly `nk_widget(&bounds, ctx) → bounds = nk_panel_alloc_space()`, then paint into `win->buffer` at those `bounds`.

Crucially, **the row's pixel widths are decided before any widget runs**, not after. `panel_space = nk_layout_row_calculate_usable_space(...)` (`22639`) reads `layout->bounds.w` (the parent panel's already-known width) and divides. This works because Nuklear's containers (windows, groups) are sized *by the user* up front (`nk_begin(name, rect, flags)`), not by their content. There's no "hug content" mode for a window; if you want that you set `NK_WINDOW_DYNAMIC` (`5733`) and the window grows in *height* only, capped by an outer max.

## 4. Widget IDs: hash names with murmur

`typedef nk_uint nk_hash` (`432`) — a 32-bit value. `nk_murmur_hash(key, len, seed)` (`7743`, exposed at `4029`) is plain MurmurHash2. IDs are *not* a stack like ImGui's `PushID/PopID`; instead, every place that needs a stable identity hashes a name string with a contextually-chosen seed. Examples:

- Window title: `nk_murmur_hash(name, len, NK_WINDOW_TITLE)` (`20730`). The seed is a fixed sentinel so a window keeps its identity across frames.
- Tree node: `nk_murmur_hash(title, len, (nk_hash)line)` (`22942`) — seeded with `__LINE__` for call-site disambiguation.
- Group: `nk_murmur_hash(id, len, NK_PANEL_GROUP)` (`23305`).
- Edit field collisions: `nk_murmur_hash(name, len, win->property.seq++)` (`29101`) — when an edit goes active, the seq bumps so the *next* same-name property gets a different hash, breaking ID collisions inside a single frame.

Persistent state lookup (scroll, tree open/close) keys on these hashes via `nk_find_value(win, hash)` / `nk_add_value(ctx, win, hash, value)` (`19913`/`19931`).

## 5. State storage: `nk_table` per window

`struct nk_table` (`5912`) is a fixed-cap page: `{seq, size, keys[NK_VALUE_PAGE_CAPACITY], values[NK_VALUE_PAGE_CAPACITY], next, prev}`. The capacity is computed (`5909`) as `max(sizeof(nk_window), sizeof(nk_panel)) / sizeof(nk_uint) / 2` — usually ~58 entries. Each window has a doubly-linked list of these pages (`nk_window.tables` at `5803`). `nk_find_value` (`19931`) is a *linear scan* across pages and across keys; `nk_add_value` (`19913`) appends to the head page or allocates a new one when full.

Why pages of fixed size? Because `nk_table`, `nk_window`, and `nk_panel` are merged in `union nk_page_data` (`5920`) and allocated from one `nk_pool` of `nk_page_element`s. The pool (`5938`) is a list of slabs (`nk_page` → `nk_page_element[capacity]`); freed elements go on `ctx->freelist` for reuse (`19808-19811`). This is why state values are `nk_uint` (32-bit) and not arbitrary `void*` — they have to fit in a uniform slot. Anything bigger (text edit buffers, property scratch) is a *named field on the window itself* (`nk_property_state`, `nk_edit_state` at `5754`/`5767`), one instance per window, distinguished by `name` hash + `seq`.

This is a big simplification vs ImGui's `ImGuiStorage` (sorted vector keyed by `ImGuiID`) and egui's `IdTypeMap` (`(Id, TypeId) → Box<dyn Any>`). Nuklear trades flexibility for *zero allocation in the steady state* — every value is one `uint`, every page is exactly one slab slot.

## 6. The strict no-alloc philosophy

Everywhere you look, allocation is either upfront or chained off a `nk_buffer`. `struct nk_buffer` (`4427`) is a double-ended bump allocator: `NK_BUFFER_FRONT` for commands (grows up), `NK_BUFFER_BACK` for window/panel/table page elements when not using the pool (grows down, see `19821`). `nk_init_fixed` hands one block to *both* the command buffer and the pool; once it's full, allocation returns null and widgets become no-ops rather than crashing. `nk_clear(ctx)` (`695`) at end of frame just resets the front pointer back to `begin` — windows survive in the pool keyed by their `seq` field (windows whose `seq != ctx->seq` after the frame are GCed by `nk_remove_table` etc., see `19541/20601`).

Compare to ImGui (`ImVector` realloc on growth), egui (`Vec` everywhere, `Arc<Mutex<…>>` for shared state), even Clay (which does have an arena but exposes growable arrays). Nuklear's bound is *hard*: pick a buffer size at init, that's the per-frame ceiling forever. Vurtun's blog explicitly cites embedded/constrained-environment use as the design constraint — the same reason imgui got picked up but more aggressive.

## 7. Lessons for Palantir

**Worth borrowing:**

- **Typed shape commands beat vertex output for a layout-decoupled middle.** Palantir's `Shape` enum is already this — `RoundedRect`, `Text`, `Line` instead of triangles. Nuklear validates that style by example: 18 op types, `nk_convert` runs CPU tessellation only at the very last step before the backend, and the same `nk_command_buffer` feeds backends that draw with native primitives (Cairo, X11, GDI) and ones that go through a vertex buffer. Palantir's wgpu pass should keep that split: walk `Shape`s, batch by SDF/glyph/path pipelines, never bake triangles into `Shape` itself.
- **`ShapeRect::Full` ≈ `nk_command_buffer` clip stack.** Both punt screen-space resolution to a later stage. Nuklear's `nk_push_scissor` emits a command instead of mutating GPU state, so the converter can split draws on the boundary; Palantir's sentinel resolves at paint pass against owner `Rect`. Same idea.
- **Row template (`nk_layout_row_template_*`).** This is the single layout primitive worth lifting wholesale. `static(px) | dynamic | variable(min_px)` per slot is exactly Palantir's `Sizing::{Fixed, Fill, Fill-with-min}` along the main axis. Even though Palantir does proper measure/arrange, an `HStack::row_template([...])` builder that takes per-child sizing hints in *one* call is more ergonomic than building each child with `.width(Sizing::Fill)` separately when you have a known column shape (a settings dialog row, a toolbar). Cheap to add on top of the existing engine.
- **Murmur-of-name + call-site seed.** Palantir's `WidgetId` can use the same trick: `hash(user_key, seed=line!())` for collision-resistant IDs without forcing the user to push/pop. Nuklear's `nk_murmur_hash(title, len, line)` for tree nodes is a clean idiom.
- **Single byte buffer with double-ended bump allocation.** Palantir's `Tree.nodes` and `Tree.shapes` are already two `Vec`s reset each frame, which is morally the same. Worth keeping that — don't over-engineer to `Box<Node>` because some retained-mode design says so.

**Avoid (or: where Palantir already does better):**

- **Caller-sized windows.** Nuklear's whole layout system works because `win->bounds.w` is given by the user at `nk_begin`. There is no honest "hug content" for a top-level — `NK_WINDOW_DYNAMIC` only relaxes the height, and even then expects a max. Palantir's measure pass returns intrinsic sizes bottom-up; a window can hug its tree on frame 1.
- **Row-only layout as the *only* primitive.** Anything that isn't a row of widgets becomes `nk_layout_space_begin` + manual `nk_layout_space_push(rect)` (free placement mode). Nested vertical layouts are nested *groups*, each of which is its own sub-window with its own command buffer. That's how you end up needing `nk_group_begin`/`end` everywhere. Palantir's `HStack`/`VStack` over a real tree gives free composition without the group-per-column tax.
- **`row.index >= cols → next row` as the only wrap rule.** No content-driven wrap, no flex `wrap`, no leftover redistribution after a child returns smaller than its slot. Slots are reserved up front; if a widget is smaller than its slot, the slot stays the slot size. Palantir's arrange pass sees actual measured sizes and can tighten — keep that.
- **`nk_uint`-only state values.** The 32-bit-slot constraint is why Nuklear has to special-case `nk_edit_state` and `nk_property_state` as named fields on the window. Palantir's planned `Id → Box<dyn Any>` is the right call; the corresponding cost (one heap alloc per stateful widget on first frame) is paid once and amortized forever.
- **Linear-scan tables.** `nk_find_value` walks every key in every page (`19937`). Fine at 50 entries per window, falls over if the model leans on keyed state. Use a hash map for the persistent state store.
- **Linked-list command buffers spliced across windows in a separate build pass.** The pointer-patch dance in `nk_build` (`19657-19690`) is clever but exists because each window writes into an allocator that doesn't know about other windows. Palantir has one `Tree.shapes` flat array; the paint pass walks the tree pre-order and emits in declaration order, no second pass to stitch.

The mental model: Nuklear = *immediate submit, command-typed shape buffer, slot-reserved row layout, all out of one fixed byte block*. Palantir = *immediate submit into a tree, typed `Shape` arena, real measure/arrange, wgpu-instanced paint*. Borrow the row-template primitive and the typed-shape-buffer staging; keep the tree and the measure pass.
