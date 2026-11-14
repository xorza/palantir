# Text editors in other projects (2026-11-14)

Facts behind `.notes/proposals/rich-text-editor.md`: how the field stores text, tracks positions,
lays out and shows a large document, highlights it, and renders Markdown. Read from each project's
own documentation or source on the date above; a claim read second-hand says so.

## Text storage

- **Zed: a rope that is a `SumTree` of chunks.** A B+ tree whose leaves hold items and a summary
  apiece and whose inner nodes hold the summary of their subtree; nodes are `Arc`s, so the tree is
  persistent and a snapshot "bumps a reference count". A cursor seeks along one dimension (byte
  offset) while it sums others (row and column), `O(log n)`. A chunk holds at most 128 bytes
  inline. `TextSummary` holds the UTF-8 and UTF-16 lengths, the line count and last-line length,
  and the longest row. The stated reason for the choice is concurrent access: each edit sends a
  snapshot to a background thread for the tree-sitter parse. Over twenty parts of Zed use
  `SumTree`, the display map among them
  ([Rope & SumTree](https://zed.dev/blog/zed-decoded-rope-sumtree)).
- **VS Code: a piece tree**, a piece table whose pieces sit in a red-black tree that caches each
  left subtree's length and line-break count. It replaced an array of line objects that took about
  600 MB for a 35 MB file. The line array still won on small files and "starts to choke" past 100k
  lines; after thousands of edits `getLineContent` through the pieces was slower than the array's
  `O(1)`, under 1 % of a view's render time. The hottest call was reading a line, not editing
  ([Text Buffer Reimplementation](https://code.visualstudio.com/blogs/2018/03/23/text-buffer-reimplementation)).
- **xi-editor retrospective.** The rope is the part that held up: "there aren't any cases where it
  performs badly", its cost is complexity, which Rust contains behind an interface; a contiguous
  string is fine under about a megabyte. What did not hold up: "adding async made everything more
  complicated", word wrap and scrolling most; the front-end/core process split; CRDTs for plug-ins.
  For highlighting the author would adapt an incremental parser, not regex grammars, "slow and
  imprecise" ([xi-editor retrospective](https://raphlinus.github.io/xi/2020/06/27/xi-retrospective.html)).
- **Helix: `ropey`, cloned for snapshots**, "cheap to clone"
  ([architecture](https://github.com/helix-editor/helix/blob/master/docs/architecture.md)).
- **Rope crates.** `ropey` 1.x indexes by `char`; 2.0 indexes by byte and is a beta
  ([ropey 2.0.0-beta.1](https://docs.rs/crate/ropey/2.0.0-beta.1)). `crop` indexes by byte, clones
  in `O(1)`, knows LF and CRLF alone; its README times it level with `jumprope` and about 3.5 times
  ahead of `ropey` on an editing trace, measured by its author
  ([crop](https://docs.rs/crate/crop/latest)). `jumprope` copies the text on clone. None carries a
  caller's summary, so none can index heights or ranges.

## Edits, positions and selections

- **CodeMirror 6: an immutable state and transactions.** Every change is a transaction dispatched
  to the view; a change set describes replaced ranges against the old document and maps a position
  into the new one. A selection is a sorted list of ranges, each an anchor and a head, overlapping
  ones merged; one is the main range ([System Guide](https://codemirror.net/docs/guide/)).
- **Helix: the same shape.** "Multiple selections are a core editing primitive"; "a single cursor
  … is simply a selection with a single range"; a rope is changed by "an OT-like `Transaction`"
  that "can be inverted to produce an undo", and "selections and marks can be mapped over a
  transaction". A document holds the selections of each view on it
  ([architecture](https://github.com/helix-editor/helix/blob/master/docs/architecture.md)).
- **Zed: anchors name the insertion that made the surrounding text**, as its buffer is a CRDT for
  collaboration ([Text Coordinate Systems](https://zed.dev/blog/zed-decoded-text-coordinate-systems)).
- **What makes editing hard** ([Text Editing Hates You Too](https://lord.io/text-editing-hates-you-too/)):
  a vertical move keeps a goal x that only a horizontal move resets; at a soft wrap two carets
  share one offset, so a caret carries an affinity bit; a delete takes a whole grapheme cluster; a
  selection in mixed-direction text is contiguous in bytes or on screen, never both; an input
  method's composition is styled text the document does not hold yet.

## Showing a large document

- **CodeMirror 6: a viewport and a height map.** The view "will only render that plus a margin
  around it" and tracks height for the whole document, measured or estimated. Decorations are
  mark, widget, replace and line, kept in range sets that map through changes. "Only directly
  provided decoration sets may influence the vertical block structure of the editor": a provider
  that reads the viewport runs after the viewport is known, so it can paint and not move
  ([System Guide](https://codemirror.net/docs/guide/)).
- **Zed: a stack of coordinate maps.** `InlayMap → FoldMap → TabMap → WrapMap → BlockMap`, each a
  `SumTree` of transforms with a `sync` that turns the edits of the layer below into its own; a
  display position is a row and a column, and a block's height is a count of rows
  ([`display_map.rs`](https://github.com/zed-industries/zed/blob/main/crates/editor/src/display_map.rs)).
- **Zed wraps the whole document, off the frame.** Under 100 rows the wrap runs in place; past it
  the work goes to a background task the frame waits 5 ms for (1 ms for an edit), and on a
  timeout the map serves an interpolated snapshot until the task lands
  ([`wrap_map.rs`](https://github.com/zed-industries/zed/blob/main/crates/editor/src/display_map/wrap_map.rs)).
- **Zed's line cache keys shape, not paint.** The key is the text, the font size, the wrap width
  and a list of `FontRun { len, font_id }`; colors and underlines are not in it. Two frames of
  entries are kept, and an entry not used in a frame is dropped at the end of the next
  ([`line_layout.rs`](https://github.com/zed-industries/zed/blob/main/crates/gpui/src/text_system/line_layout.rs)).
- **TextKit 2 lays out the viewport and estimates the rest**, and its users report the cost: the
  document height "is unstable" while scrolling and "frequently changes value significantly", so
  a scroll to the end lands short
  ([TextKit 2: the promised land](https://blog.krzyzanowskim.com/2025/08/14/textkit-2-the-promised-land/)).
- **VS Code leaves a line past `editor.maxTokenizationLineLength` (20 000) without tokens**, read
  second-hand ([vscode#162291](https://github.com/microsoft/vscode/issues/162291)).

## Text layout engines

- **cosmic-text 0.19**, the one palantir shapes with. A span's `Attrs` hold the family, weight,
  style, stretch, a color, metrics (a size and a line height), letter spacing, font features and a
  text decoration (underline, strikethrough, overline). No inline object, no custom advance, no
  background ([`Attrs`](https://docs.rs/cosmic-text/0.19.0/cosmic_text/struct.Attrs.html)).
- **parley 0.13.** Styles are properties over ranges; an `InlineBox` with a width, a height and a
  baseline sits in the line beside glyph runs; a layout breaks lines again at a new width without
  shaping again, and a change of text or style builds a new layout. It has cursor and selection
  types and an AccessKit bridge, shapes with HarfRust, analyses with ICU4X, finds fonts with
  Fontique, and does not rasterize ([parley](https://docs.rs/parley/latest/parley/),
  [repository](https://github.com/linebender/parley)).
- **egui's move to parley is an open draft**; its author lists cosmic-text's missing AccessKit
  support among the reasons ([egui#5784](https://github.com/emilk/egui/pull/5784)).

## Highlighting

- **tree-sitter 0.27.** A parse takes the old tree, edited first with `Tree::edit`, and reuses
  what the edit left alone; `parse_with_options` reads the text through a callback by byte offset,
  so a rope feeds it chunk by chunk, and takes a progress callback; `set_included_ranges` parses
  part of a document with the tree's ranges still the document's, which is how one language nests
  in another ([`Parser`](https://docs.rs/tree-sitter/latest/tree_sitter/struct.Parser.html)). The
  runtime is C, and so is each grammar.
- **Helix and Zed both layer trees for injected languages**, one parser a layer
  ([tree-house](https://docs.rs/tree-house/0.1.0/src/tree_house/lib.rs.html)).

## Markdown

- **A rich-text tree does not round-trip Markdown.** GitLab's editor on ProseMirror regenerates the
  whole source on save: code fences lose their markers, emphasis markers change, backslashes grow
  with each trip ([gitlab#607833](https://gitlab.com/gitlab-org/gitlab/-/issues/607833),
  [epic 7256](https://gitlab.com/groups/gitlab-org/-/epics/7256)).
- **Obsidian's live preview is decorations over the source.** The document stays the Markdown
  text; a state field supplies replace decorations that show a widget in place of the syntax
  "when the cursor is not overlapping with the target positions"
  ([forum](https://forum.obsidian.md/t/plugin-api-expose-live-edit-functionality-for-extension/73447),
  [decorations](https://docs.obsidian.md/Plugins/Editor/Decorations)). Its users report the cost:
  text moves under the caret as markers show and hide
  ([forum](https://forum.obsidian.md/t/markdown-formatting-characters-in-live-preview/50439)).
- **Zed's Markdown view parses with pulldown-cmark on a background task** and keeps each event
  with its byte range in the source. A selection is a pair of source offsets, so it crosses blocks;
  a pointer position maps back to a source index. An append parses the whole source again, and a
  parse asked for during one runs once more after it
  ([`markdown.rs`](https://github.com/zed-industries/zed/blob/main/crates/markdown/src/markdown.rs)).
- **pulldown-cmark 0.13** is a pull parser for CommonMark with an iterator of events and source
  ranges; tables, footnotes, task lists and strikethrough are options
  ([pulldown-cmark](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/)).
- **tree-sitter-markdown is for highlighting alone**: "it is not recommended to use this parser
  where correctness is important", and "there are still lots of inaccuracies in the output"
  ([README](https://github.com/tree-sitter-grammars/tree-sitter-markdown)).

## Accessibility

- **AccessKit exposes text as runs with a length per character**; egui caps a run at 255
  characters, as a run's word starts are `u8`
  ([egui `text_selection`](https://docs.rs/egui/latest/egui/text_selection/index.html)).

## Second pass: who moved engines, and why

- **Bevy 0.19 and Slint 1.14 lay text out with parley.** Slint's reason is one stack in place of
  one per renderer ([Slint 1.14](https://slint.dev/blog/slint-1.14-released)); Bevy's stated one
  is thin, "meaningfully better documentation and somewhat nicer to use", read second-hand
  ([Linebender 2026 Q1](https://linebender.org/blog/tmil-25/),
  [flui#931](https://github.com/vanyastaff/flui/issues/931)).
- **A project that weighed the move and stayed counted the cost** (flui#931, a retained-mode GUI
  on cosmic-text and wgpu): parley has no glyph atlas for wgpu, so the atlas, the batching and the
  shader are the mover's; its ICU4X analysis adds binary size; Bevy's move lost a glyph's byte
  index. Its reasons to look again: a need cosmic-text cannot express, "inline boxes" named first.
  A benchmark it cites, parley 93 % slower on an Arabic wrap, it marks unaudited.
- **iced shapes with a cosmic-text fork since September 2026**, for one need: horizontal padding
  on a span, so inline code in Markdown has room around it
  ([iced#3464](https://github.com/iced-rs/iced/pull/3464)). What it asks of upstream, open: a
  `padding` on `Attrs`, a glyph's ascender and descender, and a fix for a smaller span lowering the
  line height of its line; "a more generic API for embedding blocks in text would also solve the
  issue" ([cosmic-text#541](https://github.com/pop-os/cosmic-text/issues/541)).
- **parley gained CSS `text-indent` and the full AccessKit text properties in 2026**
  ([Linebender 2026 Q1](https://linebender.org/blog/tmil-25/)).

## Second pass: heights and wraps without measuring

- **Monaco wraps a monospace line from its characters alone.** `MonospaceLineBreaksComputer`
  takes a half-width and a full-width character width from the font, counts a tab to its stop,
  classes each character as break-before, break-after or ideographic, and decides in pixels; it
  starts a line's new breaks from its old ones. It is the wrap, not a prediction of one
  ([source](https://github.com/microsoft/vscode/blob/main/src/vs/editor/common/viewModel/monospaceLineBreaksComputer.ts)).
- **CodeMirror estimates an unmeasured line from its length**: one row with wrapping off, else
  `1 + max(0, ceil((length - lineLength) / max(1, lineLength - 5)))` rows of the measured line
  height; a run of unmeasured lines is one gap node, replaced by line nodes as measurements
  arrive ([`heightmap.ts`](https://github.com/codemirror/view/blob/main/src/heightmap.ts)).
- **Store a length, not a position.** VS Code's bracket tree holds each node's length, so an edit
  changes the edited node and its ancestors, where absolute positions shift "every node later in
  the document"; one bracket typed into a 42 000-line file went from about 10 s to under 1 ms
  ([Bracket pair colorization](https://code.visualstudio.com/blogs/2021/09/29/bracket-pair-colorization)).

## Second pass: text that keeps its formatting

- **Peritext: characters in a sequence, formatting as marks over ranges, apart from the text.** A
  mark's edge sits in the gap before or after a character, which decides whether typing at the
  edge joins it: bold and italic grow, "a link or comment span does not grow in the same way".
  Blocks, "headings, bullet points, block quotes, and tables", are not handled
  ([Peritext](https://www.inkandswitch.com/peritext/)).
- **Quill's Delta is flat too, and a line's format sits on its newline**: "attributes associated
  with a newline character describe formatting for that line"
  ([Designing the Delta Format](https://quilljs.com/guides/designing-the-delta-format)).
- **Flutter's text field holds plain text and derives its spans**: a controller's `buildTextSpan`
  returns the styled tree the field draws, which is how its code editors color
  ([`buildTextSpan`](https://api.flutter.dev/flutter/widgets/TextEditingController/buildTextSpan.html)).
- **Floem's editor shows text the rope never stores as "phantom text"**, with a map from a display
  column back to the column before it
  ([`TextLayoutProvider`](https://lapce.dev/floem/floem/views/editor/visual_line/trait.TextLayoutProvider.html)).

## Second pass: syntax

- **Zed parses on a background thread while the frame reads the earlier tree**; an injected
  language is its own tree, found by a query, and injections nest
  ([syntax-aware editing](https://zed.dev/blog/syntax-aware-editing)). The frame once waited up to
  1 ms for the parse; the wait had no point to check its timer at, and for a file of any size it
  ran out, by Zed's own commits, read second-hand.
- **One parser and a query a feature**: highlights, an outline (`@item`, `@name`), indents (a
  range whose lines sit one level in), brackets, and a selection that grows by node; "no need to
  write custom procedural code for each language" (same post).
- **Zed queries brackets for the rows on show and caches them in chunks of at most 50 rows**, a
  chunk dropped by an edit and filled again when next drawn
  ([rainbow brackets](https://zed.dev/blog/rainbow-brackets)).
- **Helix's `tree-house` 0.4 is its layering as a crate**, MPL-2.0, over `ropey` 1.6
  ([tree-house](https://docs.rs/tree-house/latest/tree_house/)).
- **`regex-cursor` 0.1.5 runs regex-automata over chunks.** It calls itself a prototype; a
  prefilter longer than a byte and UTF-8 mode do not work
  ([regex-cursor](https://docs.rs/regex-cursor/latest/regex_cursor/)).

## Second pass: Markdown

- **Streamed Markdown is shown by freezing finished blocks and parsing the open one again**, the
  common answer for a model's output; a reference definition that arrives late can change a
  finished block, and the crates that do this call themselves best-effort
  ([mdstream](https://docs.rs/crate/mdstream/latest)).
- **pulldown-cmark holds itself to linear time**, bounds link nesting and reference expansion
  against hostile input, and scans with SIMD on x86-64 alone
  ([performance](https://pulldown-cmark.github.io/pulldown-cmark/dev/performance.html)).
- **VS Code scrolls a preview with its source by line**: each rendered block carries the source
  line it starts at ([vscode#133376](https://github.com/microsoft/vscode/issues/133376),
  second-hand).
- **Live preview's complaint is movement**: "the lines shift and the cursor jumps". An editor
  that reveals markers by element, a table's cell when the caret enters it, moves less
  ([Obsidian forum](https://forum.obsidian.md/t/to-hide-markdown-syntax-in-live-preview-so-it-doesn-t-show-formatting-markers/109155)).

## Second pass: search

- **Zed's literal search streams over the rope; its regex search copies it.** The text query runs
  an Aho-Corasick automaton over `rope.bytes_in_range`, and the regex query calls
  `rope.to_string()` and runs `fancy_regex` over the string
  ([`search.rs`](https://github.com/zed-industries/zed/blob/main/crates/project/src/search.rs)).
