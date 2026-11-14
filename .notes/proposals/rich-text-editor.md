# A text editor

One document and display core for a code editor, a Markdown editor with a live or side-by-side
preview, a read-only Markdown view, and rich text that keeps its own formatting. The decisions,
then the work in steps. Facts and sources: `.notes/research/2026-11-14-text-editors.md`. Every
name here is a proposal: the public API waits for a go-ahead.

## Today

- **`TextEdit` edits the host's `String`** with one caret and one selection, undo groups, an input
  method's composition, grapheme and word walks, multi-click and a context menu
  (`src/widgets/text_edit/`). A multi-line field scrolls and has no scroll bar.
- **The whole text is one run, and each frame pays for all of it.** The record pass copies it into
  the frame's text arena and hashes it (`Ui::intern`, `TextStore::record`), `EditState::reconcile`
  compares it with the copy the history keeps, and an edit is a new key, shaped from the whole
  text.
- **A run has one face and one color.** `EncodedKey` bakes the color into the cached quads and
  names what a second color needs, "a color-span fingerprint"; `extract_glyphs` asserts no glyph
  carries its own.
- **Nothing is virtual.** `Scroll` has no virtual list, and a scroll offset is an `f32`, exact to
  a pixel only below 2²⁴ px, about 800 000 lines of 20 px.
- **Text reuse and damage are per node.** A reuse slot is a widget's id and an ordinal, a damage
  entry a widget's id: a paragraph that is a node of a stable id keeps both across a scroll.
- **No worker.** Shaping is single-threaded behind `Rc<RefCell<…>>`; `Ui::waker` lets a thread ask
  for a frame.
- **cosmic-text 0.19 shapes spans** of differing family, weight, slant and size in one buffer
  (`Buffer::set_rich_text`), which palantir does not use. It has no inline object, no padding on a
  span and no hanging indent, and upstream holds an open report that a smaller span lowers its
  line's height (cosmic-text#541).

## Decisions

### The model

- **The document is plain text; what it looks like is decoration (D1).** A decoration is derived
  from the text, as a highlight or a hidden Markdown marker, or stored beside it, as a bold in
  rich text that has no markup. Derived, as CodeMirror 6 and Flutter's `buildTextSpan` show a
  document and Obsidian builds its live preview; stored, as Peritext and Quill keep formatting as
  marks over a flat sequence. Refused: a node tree (ProseMirror, Lexical), which cannot hold every
  Markdown document and rewrites the source on save, as GitLab's editor does. Cost: no block
  inside a block for stored rich text, a table or a list in a quote, which a flat sequence cannot
  say.
- **Text is a rope over one `SumTree`**, a persistent B+ tree with a caller's summary on each
  node, as Zed's. An edit is `O(log n)` in the worst case, with no gap to move and no table to
  compact; a snapshot is one `Arc` clone, which is what lets a parse run on another thread; and
  the same tree indexes heights and ranges below. Refused: `String`, `O(len)` an edit and a copy a
  snapshot; a gap buffer, whose far edit moves the gap in one frame; a piece table, whose reads
  slow as pieces pile up (VS Code's `getLineContent`); `ropey` and `crop`, text alone, so heights
  and ranges would need a second tree. Cost: the tree is ours to test.
- **Every tree stores lengths, never positions**, so an edit changes the edited leaf and its
  ancestors and nothing after it, the lesson of VS Code's bracket tree.
- **An unshared node is edited in place** (`Arc::make_mut`), so typing with no snapshot out
  allocates only where a node splits.
- **Offsets are UTF-8 bytes, a position a row and a byte column**, as `TextEdit` and the shaper
  count. A UTF-16 dimension joins the summary when a caller needs one.
- **Line ends are `\n` in the rope.** A load turns `\r\n` and `\r` into `\n` and keeps which the
  file used, for the save, as CodeMirror counts every break as one unit.
- **Every change is a transaction through one gate**, `TextDocument::apply`: typing, a
  composition's commit, paste, undo, and an app's own edit alike. It holds a change set, sorted
  replacements against the text before it, which maps a position, inverts into its undo and
  composes into a group, as CodeMirror's and Helix's; and the changes to stored marks made with
  it, so one undo takes back both.
- **A selection is a sorted list of ranges, from the first step.** A caret is an empty range, one
  range is the primary, overlapping ranges merge, and each command takes the list: the batch is
  the base operation. As Helix and CodeMirror. Refused: one caret now and a list later, which
  re-signs every command.
- **A range holds an anchor, a head, an affinity and a goal x.** The affinity says which row a
  caret at a soft wrap sits on; the goal x survives vertical moves alone.
- **A view owns its selections and its scroll; the document owns text, stored marks and
  history.** So two views show one document, source beside preview. A view's state lives in the
  state map under the widget's id, behind associated functions, as every widget's does.
- **A consumer catches up by revision.** The document keeps a bounded log of recent change sets; a
  view, a decoration layer or a parser maps what it holds through the sets since the revision it
  last saw. One too far behind rebuilds from the text.
- **Ranges that must survive edits sit in a range set** on the `SumTree`, as CodeMirror's
  `RangeSet`. Refused: Zed's anchors into a CRDT, built for collaboration, which is out of scope.
  Cost: collaboration later changes what an anchor is.
- **A stored mark's edge says whether typing there joins it**: a bold grows, a link does not, as
  Peritext's anchors. A paragraph's stored format sits on its newline, as Quill's.
- **Undo keeps today's groups**, 300 ms and a word (`history.rs`), over inverted change sets;
  bounded by count and by bytes.

### Layout

- **The paragraph, a hard line, is the unit of shaping, caching and damage.** Each visible
  paragraph is one run under its own key, so an edit shapes one paragraph and every other is a
  cache hit. As Zed caches a line and TextKit 2 a paragraph's fragment.
- **A run takes spans.** The shape key grows a digest of the font runs, each a family, weight,
  slant and size over a range; a run of one face keeps today's key. Labels gain spans by the same
  change.
- **Color is paint, not shape.** Color, underline, strikethrough, squiggle and background ride
  beside the shaped run and are applied when it is encoded, by a glyph's byte offset; the encoded
  key takes the fingerprint its doc asks for. A highlight that lands a frame late recolors and
  shapes nothing, as Zed's `FontRun` key leaves color out.
- **cosmic-text stays the engine (D2).** Its spans cover every font run above. Refused for now:
  parley, which Bevy and Slint moved to and which has an inline box, a hanging indent and an
  AccessKit bridge, at the cost of `src/text/cosmic`, the glyph key the atlas is built on, and
  every text golden; a fork of cosmic-text, as iced carries one for a span's padding. To take up
  again when a line needs a widget in it.
- **Inline content is text; anything else is a block.** A replace decoration shows nothing or
  other text in place of a range: a fold's `…`, an inlay hint, a box for `[ ]`, as Floem's phantom
  text. An image, a table or a rule is a block between paragraphs. Cost under D2: no image or
  widget inside a line, inline code's background hugs its glyphs, and a wrapped code line
  continues at the margin.
- **A list's marker is drawn in its paragraph's indent**, not in the run, so wrapped rows align
  under the text, as a browser's marker box.
- **A paragraph past a byte cap shapes in pieces**, cut at a word or grapheme boundary, so a
  megabyte on one line is shaped a screen at a time. Refused: leaving such a line undrawn or
  uncolored, which shows wrong text.

### Display

- **Decorations are the one input to presentation**: a *mark* styles a range, a *replace* hides
  or substitutes one, a *line* decoration sets a paragraph's indent, size, spacing, background
  and gutter marker, a *block* puts a widget between paragraphs. CodeMirror's four kinds.
  Highlights, the Markdown preview, search matches, diagnostics, folds, bracket colors, a diff and
  an input method's composition are each a layer of them, and an app adds its own.
- **A structural decoration is known for the whole document; a paint one is asked for by range.**
  Replace, line and block decorations change heights, so they live in range sets mapped through
  edits. Marks are queried for the visible range each frame, so a highlighter works on a screen
  of text. CodeMirror's rule, for its reason: the viewport depends on the block structure.
- **One height index over display blocks, not a stack of coordinate maps.** A `SumTree` whose
  items are a paragraph, a block, a fold or a run of unmeasured paragraphs, summed by bytes, rows
  and height, as CodeMirror's height map. Refused: Zed's five maps, whose display position is a
  row and a column on a grid of equal rows, which a heading or an image breaks.
- **A height is exact where a closed form gives it, and converges where it does not (D3).**
  Exact with no shaping: an unwrapped paragraph, one line of its role's height; and a wrapped one
  that is ASCII in a fixed-pitch face and fits, its columns times the advance against the width.
  The rest start from CodeMirror's estimate by length; each frame measures a fixed count of bytes
  outward from the viewport until none is left, and an edit measures its own paragraph. The
  widest row converges the same way, for the horizontal bar. Refused: estimates for good, TextKit
  2's unstable height; exact on a worker, as Zed wraps, a second font system to keep equal to the
  first; a second line breaker that predicts the shaper's wraps, which Monaco can have since its
  breaker is the wrap. Cost: until a wrapped prose document converges its scroll thumb drifts,
  and a new wrap width starts it again.
- **The budget is bytes, not time**, so a test sees the same frames on every machine.
- **The view scrolls by an anchor**, a paragraph and an offset inside it, as Zed's `ScrollAnchor`:
  a height that changes above the viewport moves nothing on screen. Heights sum in 1/64 px
  integers, the shape key's quantum, as an `f32` cannot address a pixel in a long file; the bars
  take the fraction.
- **Only the viewport and a margin are recorded.** Each visible paragraph is a node with a stable
  id, placed in a canvas, so its damage entry and its text reuse slot follow it through a scroll.
  A frame costs `O(visible + log n)`, whatever the length.
- **Visible text goes through the frame's text arena** as every run's does: the paragraphs on
  show are interned, and nothing else of the document is read in a frame.

### Languages

- **Code is parsed by tree-sitter (D4)**: incremental, fed from rope chunks through its callback,
  with included ranges for a language inside another. As Zed, Helix and Neovim. Refused: TextMate
  grammars (`syntect`), a regex state a line with no tree; Lezer, with no Rust runtime;
  `tree-house`, Helix's layering, bound to `ropey` and MPL-2.0. Cost: a C runtime and a C grammar
  a language.
- **A language is a grammar and a query a feature**, both the app's: highlights, injections,
  brackets, indents, folds and an outline, as Zed's. palantir ships no grammar.
- **A query runs for the rows on show and its result is cached by row chunk**, dropped by an edit
  and filled when next drawn, as Zed's bracket cache.
- **Markdown structure comes from pulldown-cmark**, with source ranges, the whole document parsed
  again after an edit, as Zed's Markdown view. Refused: tree-sitter-markdown, which its own README
  rules out "where correctness is important", and a wrong tree here hides the wrong bytes.
  Refused for now: parsing only the changed blocks or only an appended tail, which a late
  reference definition makes wrong; a measurement in step 0 decides whether to earn it.
- **A parse never runs in a frame, and a frame never waits for one.** A worker takes a snapshot
  and the frame takes the result when it lands, woken through `Ui::waker`. Until then the old
  tree, moved by the edit, keeps the colors. One parse in flight and one waiting per document, the
  newest snapshot. Refused: a short wait in the frame, which Zed found ran out for a file of any
  size; async through the core, which xi found "made everything more complicated".
- **Fenced code in Markdown is an injection**: the block's range parsed by its language.

### Search

- **A literal search streams over the rope's chunks; a regex searches a copy (D8).** The worker
  copies the snapshot into one string and runs `regex` on it, with its prefilters and its whole
  syntax, as Zed's search does. Refused: `regex-cursor`, Helix's, which searches chunks in place
  and calls itself a prototype, with no prefilter past a byte. Cost: one copy of the document a
  search, off the frame.
- **Matches are a mark layer**, as any highlight.

### Markdown

- **One pipeline, three presentations.** *Source* colors the syntax. *Live* hides a construct's
  markers unless a selection touches it, as Obsidian. *View* hides them all and is read-only, with
  a selection in source offsets, so copy crosses blocks, as Zed's. Refused: a second renderer of
  widgets for the view (egui_commonmark, iced), a second layout and selection to keep right.
- **Live preview reveals by construct, not by line**, so only the span under the caret moves,
  the complaint against Obsidian's.
- **A table, an image and a rule are blocks.** In live preview a table shows as a widget until a
  selection enters it, then as source. A code block wraps; one that scrolls alone waits for a
  block that holds a view.
- **Heading sizes are the theme's type ramp**; a highlight's style is a theme slot by capture
  name.
- **A side-by-side preview is two views of one document**, scrolled together by source offset, as
  VS Code's by source line. The headings are an outline an app can list and jump by.

### Widgets, crates and API

- **A view and an editor are two widgets over one display.** `TextView` shows a `&TextDocument`
  with selection, copy and links; `TextEditor` edits a `&mut TextDocument`. As a field's
  read-only twin was refused (`src/widgets/AGENTS.md`), so is a read-only switch here.
- **The core is modules of `palantir`; each language is a crate over the public API (D5).**
  `common::sum_tree`, a `document` module beside `text`, the widgets in `widgets`.
  `palantir-markdown` holds pulldown-cmark and `palantir-syntax` tree-sitter. As the showcase is
  a crate: a feature inside `src` would reach privates, and a C toolchain stays off an app that
  edits plain text.
- **The app owns the document; a widget borrows it for a frame.**

  ```rust
  let mut doc = TextDocument::from(source);            // kept by the app
  TextEditor::new(&mut doc)
      .line_numbers(true)
      .wrap(TextWrap::Wrap)
      .layer(&highlights)                              // a decoration layer; any count
      .show(ui);

  TextView::new(&doc).show(ui);                        // read-only: select, copy
  Markdown::new(&doc).show(ui);                        // palantir-markdown's view
  ```

- **`TextEdit` moves onto the core last (D6)**, its API as it is, so one implementation moves a
  caret, as Zed's single-line inputs are its editor in another mode. Until then there are two.

### Out of scope

- **Collaboration**, **accessibility** (palantir has none; a paragraph's runs are what AccessKit
  takes), **a language server** (its diagnostics and hints are decoration layers an app adds),
  **inline boxes** (D2), **math**, **a minimap**, **excerpts of several documents in one view**,
  **copy as HTML** (the clipboard holds text and an image).

## Decided (2026-11-14)

| | decision | taken | refused |
|---|---|---|---|
| D1 | the model | plain text; decorations derived or stored | derived alone; a node tree |
| D2 | the layout engine | cosmic-text, inline content as text | parley now; a cosmic-text fork |
| D3 | heights of wrapped text | closed forms, then converge by a byte budget | exact on a worker; estimates for good |
| D4 | code highlighting | tree-sitter, the app's grammars and queries | `syntect`; none in palantir |
| D5 | where languages live | core in `palantir`, two crates over the public API | features of `palantir`; a crate for the core |
| D6 | `TextEdit` | rebased on the core, last | left as it is; rebased first |
| D7 | what follows the plain editor | Markdown, then code | code, then Markdown |
| D8 | regex search | `regex` over a snapshot's copy, on the worker | `regex-cursor`; literal alone |

Dependencies taken with D4, D5 and D8, each entering a `Cargo.toml` in the step that first uses
it: `pulldown-cmark` (`palantir-markdown`); `tree-sitter` and `tree-sitter-language`
(`palantir-syntax`); `regex` and `memchr` (`palantir`), both in its lockfile today through other
crates.

## Plan

Each step lands alone and leaves the gate green. A step with a public API change ends with
`cargo drive surface`, one a user can see with `cargo shot`.

### Exit criteria

- **A frame's cost does not follow the length**: the frame benches at 1 KiB, 1 MiB and 100 MiB
  agree within noise, idle, typing and scrolling.
- **A keystroke shapes one paragraph**, and a scroll shapes the paragraphs that enter: the
  shaper's counters say so in a test.
- **No allocation in an idle or a scrolling frame** after warmup.
- **Text is byte-exact**: a model test replays random transactions against a `String`.
- **A closed-form height equals the shaped one**, for every paragraph a test sweeps.
- **A Markdown file opened and saved untouched is the same bytes.**

### 0. Measure first

1. Bench `TextEdit::multiline` at 1 KiB, 100 KiB and 1 MiB: a keystroke's frame, a scroll's frame.
2. Time one paragraph's shape, 80 and 2000 bytes, plain and with spans: it sizes D3's budget.
   Beside it `Shaping::Basic` on ASCII in a fixed-pitch face, taken only if its glyphs are the
   same.
3. Spike `set_rich_text`: mixed sizes in one line and the line height it reports (cosmic-text#541),
   the key's cost.
4. Time pulldown-cmark over 10 KiB, 1 MiB and 10 MiB, and a tree-sitter parse and an edit's
   reparse over 1 MiB, on a worker.
5. Results to `.notes/research/`, dated; a step below whose pick they overturn is rewritten first.

### 1. `SumTree` and the rope (`src/common/`)

1. `SumTree<T>`: summaries, dimensions, a cursor's seek and slice, append, split, `Arc::make_mut`.
2. `Rope`: chunks, the text summary, byte ↔ position, chunk and line iterators, a snapshot.
3. Tests against a `String` over random edits, boundaries inside a multi-byte character and a
   `\r\n`; a bench beside `String` at each size.

### 2. The document (`src/document/`)

1. `ChangeSet`: map with a bias, invert, compose; `Transaction`.
2. `Selections`: normalize, merge, map through a change set.
3. `RangeSet<T>` on the tree; `History`, ported from `text_edit/history.rs`.
4. `TextDocument`: the gate, the revision log, the line ending.
5. Commands as functions from a snapshot and selections to a transaction: moves by grapheme,
   word, line and document; insert, delete, newline; the walks of `text_edit/unicode.rs` over
   chunks. Tested with no `Ui`.

### 3. Runs with spans (`src/text/`, `src/shape/`, `src/gpu/`)

1. Font runs in `TextShapeKey` and through `set_rich_text`; a probe's caret, hit and selection
   rects over a spanned run.
2. Paint runs at encode: color, underline, strikethrough, squiggle, background; the fingerprint
   in `EncodedKey`.
3. `Text` takes spans. Goldens for a mixed run.

### 4. The view (`src/widgets/text_view/`)

1. The height index, the scroll anchor, the viewport; paragraphs as nodes in a canvas.
2. D3: the closed forms, the estimate, the byte budget; the widest row.
3. Selections painted for every range; drag, multi-click, autoscroll; copy.
4. Soft wrap and the affinity; the gutter and line numbers; the scroll bars over the anchor.
5. The decoration API: `layer`, the four kinds, structural against paint; the long-paragraph cap.
6. `TextView`, a showcase page, the benches of the exit criteria, a `cargo shot` script.

### 5. The editor (`src/widgets/text_editor/`)

1. Keys through one table of chord to command; carets for every range; column select.
2. Clipboard, the context menu, undo and redo.
3. The composition as a replace decoration and a mark, the document unchanged until it commits.
4. Vertical moves and pages through the layout; the current line; find and replace, literal,
   over chunks.
5. `TextEditor`, its showcase page and benches.

### 6. Blocks

1. Block decorations: a widget between paragraphs, its height in the index.
2. A block in place of a range of paragraphs.

### 7. Markdown (`palantir-markdown`)

1. The worker: a snapshot in, a result out, one job in flight and one waiting per document. On
   it, the parse over pulldown-cmark, events with source ranges, and the regex search of D8.
2. Source presentation; then live: replace and line decorations, reveal by construct.
3. `Markdown`, the view: links with hover and click, task boxes, images through an app's
   resolver, tables, rules, alerts, footnotes, the outline.
4. Two views of one document, scrolled together.

### 8. Code (`palantir-syntax`)

1. The tree-sitter parse on the worker of step 7: trees out, the old tree edited meanwhile.
2. Highlight marks for the visible range, cached by row chunk; injections, Markdown's fences
   among them.
3. Folds as replace decorations; bracket match and colors; indent on newline; comment toggle;
   auto-pairs; a selection that grows by node.
4. Indent guides, visible whitespace.

### 9. Stored marks

1. Marks in the document's range sets, changed through a transaction, with their edge rule.
2. A paragraph's format on its newline; the marks a caret types with.
3. Commands that set and clear a mark over every range; a text with its runs as a value an app
   saves and loads.

### 10. `TextEdit` on the core (D6)

1. A field's state holds a document, reconciled with the host's `String` by the change sets.
2. Delete `text_edit/{editor, edit_state, history}`; its tests stay and pass.

### 11. Cleanup and docs

1. Decisions into `src/document/AGENTS.md`, `src/widgets/AGENTS.md` and the two crates' own.
2. `.notes/TODO.md` loses "rich text edit" and "md viewer"; this file is deleted.
