# Overlay nesting: practice and the current recorder (2026-10-27)

## Practice

- HTML / Fullscreen "top layer": an ordered set of elements, rendered in set
  order, the last on top; dialogs and popovers enter it as they open, so one
  opened from another paints above it. <https://github.com/whatwg/html/issues/9036>
- egui `Areas`: a back-to-front `order` list of layers, each area its own
  layer; a `sublayers` map keeps a child layer directly above its parent.
  <https://doc.servo.org/egui/memory/struct.Areas.html>
- Flutter `Overlay`: a `Stack` of `OverlayEntry`s; `OverlayState.insert` puts
  an entry on top, or above or below another. Entries build apart from the
  widget that inserts them. <https://api.flutter-io.cn/flutter/widgets/OverlayState/insert.html>
- WPF: each `Popup` is its own top-level window.

## The recorder today

- One `Tree` per `Layer` (`PerLayer<Tree>`), pre-order, with
  `subtree_end` ranges; `Tree::roots` already holds many roots per layer, each
  placed by a `RootSlot` (`scene/tree/root_slot.rs`).
- `Forest::push_layer` asserts the new layer ranks strictly above the active
  one (`scene/forest.rs:284-293`), as a layer with an open node cannot start a
  second root: the new root would land in the open root's subtree.
- Main records under a synthetic `ZStack` viewport root
  (`ui/frame_cycle.rs:207`, `scene/node/mod.rs:245`).
- Layer meaning is read by `Cascade::tab_domain` (`cascade/mod.rs:290-310`),
  the modal-root rule in `input_state/focus.rs`, `RootSlot` (Main fills the
  surface) and scope silencing (`input_state/scope.rs:121`).
- Fixed per-layer storage (`PerLayer`, `Layer::COUNT`, `PAINT_ORDER`) is read
  at 31 sites in 12 files outside `scene`'s layer module.
