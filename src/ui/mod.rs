mod damage;
mod theme;
pub use theme::ButtonTheme;

pub(crate) use damage::Damage;

use crate::cascade::Cascades;
use crate::element::Element;
use crate::input::{InputEvent, InputState, PointerState, ResponseState};
use crate::layout::{LayoutEngine, LayoutResult};
use crate::primitives::{Rect, WidgetId};
use crate::shape::Shape;
use crate::text::{MeasureResult, SharedCosmic, TextMeasurer};
use crate::tree::{NodeId, Tree};
use rustc_hash::{FxHashMap, FxHashSet};

/// Snapshot of one node's per-frame state used by damage-rendering.
/// Populated at the end of `Ui::end_frame()` and read on the *next*
/// frame to detect what changed (Stage 3, Step 2 of the damage-rect
/// plan). Indexed by stable [`WidgetId`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// Arranged rect in logical pixels — same value
    /// `LayoutResult::rect` returned for the node this frame.
    pub rect: Rect,
    /// Authoring hash from `Tree.hashes` this frame.
    pub hash: u64,
}

/// Recorder + input/response broker. Lives across frames; rebuilds the tree each frame
/// while persisting input state via [`InputState`].
///
/// All public coordinate inputs and recorded rects are in **logical pixels** (DIPs).
/// `scale_factor` is the conversion to physical pixels; the renderer applies it at
/// upload time. Pointer events from winit are converted at the boundary
/// (`handle_event` / `InputEvent::from_winit`).
pub struct Ui {
    pub(crate) tree: Tree,
    pub theme: ButtonTheme,
    parents: Vec<NodeId>,
    root: Option<NodeId>,

    /// Per-frame collision set: every `WidgetId` that has been recorded this
    /// frame. Cleared in `begin_frame`. Used to enforce id uniqueness — a
    /// repeat insertion in `Ui::node` is a release-`assert!` panic, not a
    /// warning, because duplicate ids silently corrupt every per-id store
    /// (focus, scroll, click capture, hit-test rect lookup).
    seen_ids: FxHashSet<WidgetId>,

    input: InputState,
    /// Persistent layout engine: holds reusable scratch buffers across frames.
    /// Accessed via `Ui::layout(surface)`.
    pub(crate) layout_engine: LayoutEngine,
    /// Per-frame cascade resolution shared by the renderer encoder (skip
    /// invisible subtrees) and the input hit index (screen rects + sense).
    /// Rebuilt in `end_frame`.
    cascades: Cascades,

    scale_factor: f32,
    pixel_snap: bool,

    /// Surface rect from the most recent [`Ui::layout`] call. The
    /// renderer pipeline, damage heuristic, and any future backbuffer
    /// resize logic read this to know the canvas size in logical
    /// pixels. `Rect::ZERO` until the first `layout()`.
    surface: Rect,

    /// Text shaping & measurement, with the `CosmicMeasure` / mono dispatch
    /// hidden inside. Install a real shaper with [`Ui::install_text_system`]
    /// to get shaping + rendering; otherwise runs use the mono placeholder.
    pub(crate) text: TextMeasurer,

    /// Frame-skipping gate. `true` when something has changed since the
    /// last successful `end_frame()`; the host calls
    /// [`Ui::should_repaint`] to decide whether to run the pipeline this
    /// tick. Set conservatively by [`Ui::on_input`] and
    /// [`Ui::set_scale_factor`], or explicitly by
    /// [`Ui::request_repaint`] (animations, async). Cleared at the end
    /// of `end_frame()` so the next idle check returns `false`.
    /// Defaults to `true` so the first frame always renders.
    repaint_requested: bool,

    /// Last frame's per-widget snapshot (rect + authoring hash), keyed
    /// by stable [`WidgetId`]. Rebuilt at the end of every
    /// `end_frame()`. Step 3 of the damage-rendering plan reads this
    /// *before* the rebuild to diff against the just-recorded frame.
    /// Capacity retained across frames.
    pub(crate) prev_frame: FxHashMap<WidgetId, NodeSnapshot>,

    /// This frame's damage output: dirty `NodeId`s + union rect.
    /// Computed in `end_frame()` after `compute_hashes()` and *before*
    /// `rebuild_prev_frame()` — the diff needs the previous snapshot
    /// intact. Currently computed but not consumed; Step 5 (encoder
    /// filter) is the first reader.
    pub(crate) damage: Damage,
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            tree: Tree::new(),
            theme: ButtonTheme::default(),
            parents: Vec::new(),
            root: None,
            seen_ids: FxHashSet::default(),
            input: InputState::new(),
            layout_engine: LayoutEngine::new(),
            cascades: Cascades::new(),
            scale_factor: 1.0,
            pixel_snap: true,
            surface: Rect::ZERO,
            text: TextMeasurer::new(),
            // First frame must render so the host has something to
            // present. Subsequent idle frames flip back to `false`.
            repaint_requested: true,
            prev_frame: FxHashMap::default(),
            damage: Damage::default(),
        }
    }

    /// Install a shared shaper handle. Apps construct one
    /// [`SharedCosmic`] at startup and clone it into both `Ui` and the wgpu
    /// backend so they see the same buffer cache. Tests usually leave this
    /// unset and run on the deterministic mono fallback.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.text.set_cosmic(cosmic);
    }

    /// One-off text measurement. Widgets don't need this any more — layout
    /// shapes during measure — but external callers (debug overlays, etc.)
    /// can use it.
    pub fn measure_text(
        &mut self,
        text: &str,
        font_size_px: f32,
        max_width_px: Option<f32>,
    ) -> MeasureResult {
        self.text.measure(text, font_size_px, max_width_px)
    }

    /// Logical→physical conversion factor (e.g. 2.0 on a 2× retina display).
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Update on `WindowEvent::ScaleFactorChanged` or any DPI change. Clamped to
    /// a non-zero positive value. Schedules a repaint — physical-pixel
    /// rasterization changes with scale factor.
    pub fn set_scale_factor(&mut self, scale: f32) {
        self.scale_factor = scale.max(f32::EPSILON);
        self.repaint_requested = true;
    }

    /// Whether the renderer snaps rect edges to integer physical pixels.
    /// Default `true` — sharper edges, no half-pixel blur.
    pub fn pixel_snap(&self) -> bool {
        self.pixel_snap
    }

    pub fn set_pixel_snap(&mut self, on: bool) {
        self.pixel_snap = on;
    }

    pub fn begin_frame(&mut self) {
        self.tree.clear();
        self.parents.clear();
        self.root = None;
        self.seen_ids.clear();
    }

    /// Run measure + arrange for the recorded tree at the given surface rect.
    /// Call after recording (post `begin_frame` + widget tree) and before
    /// `end_frame`. Reuses the persistent `LayoutEngine` — amortized
    /// zero-alloc after warmup. The tree is read-only; layout output lands
    /// in `LayoutEngine`.
    pub fn layout(&mut self, surface: Rect) {
        let root = self.root();
        self.surface = surface;
        self.layout_engine
            .run(&self.tree, root, surface, &mut self.text);
    }

    /// Surface rect from the most recent [`Ui::layout`] call, in
    /// logical pixels. `Rect::ZERO` before the first `layout()`.
    pub fn surface(&self) -> Rect {
        self.surface
    }

    /// Damage rect for the just-finished frame, in logical pixels —
    /// the value the host should pass to both `Pipeline::build`
    /// (encoder filter) and `WgpuBackend::submit` (LoadOp + scissor).
    /// Pass the *same* value to both: the encoder culls per-node
    /// paint commands, the backend scissors GPU pixels; disagreement
    /// either wastes work or leaves gaps.
    ///
    /// `Some(rect)` → small change, partial repaint.
    /// `None` → full repaint (first frame, post-resize, no diff, or
    /// damage area exceeds the 50% threshold).
    pub fn damage_filter(&self) -> Option<Rect> {
        if self.damage.full_repaint {
            None
        } else {
            self.damage.rect
        }
    }

    /// Rebuild the per-frame cascade table and input's last-frame rect cache
    /// from the just-arranged tree. Call after `layout`. Clears the
    /// repaint-requested gate so the next [`Ui::should_repaint`] returns
    /// `false` until something new happens (input, animation tick,
    /// explicit `request_repaint`).
    ///
    /// Also computes per-node authoring hashes (`Tree.hashes`) and
    /// snapshots `(rect, hash)` per `WidgetId` into `prev_frame` for
    /// the next frame's damage-rect diff (Stage 3). The snapshot is
    /// rebuilt *after* hashes are computed and before the repaint
    /// gate clears.
    pub fn end_frame(&mut self) {
        self.cascades
            .rebuild(&self.tree, self.layout_engine.result());
        self.input
            .end_frame(&self.tree, self.layout_engine.result(), &self.cascades);
        self.tree.compute_hashes();
        self.damage.compute(
            &self.tree,
            self.layout_engine.result(),
            &self.prev_frame,
            &self.seen_ids,
            self.surface,
        );
        self.rebuild_prev_frame();
        self.repaint_requested = false;
    }

    /// Repopulate `prev_frame` from the just-finished frame. `clear()`
    /// keeps the underlying capacity so steady-state frames don't
    /// allocate. Called at the tail of `end_frame()` — after layout,
    /// cascades, and `compute_hashes` have all run.
    fn rebuild_prev_frame(&mut self) {
        self.prev_frame.clear();
        let result = self.layout_engine.result();
        let n = self.tree.node_count();
        self.prev_frame
            .reserve(n.saturating_sub(self.prev_frame.len()));
        for i in 0..n {
            let id = NodeId(i as u32);
            let wid = self.tree.widget_ids[i];
            let snap = NodeSnapshot {
                rect: result.rect(id),
                hash: self.tree.hashes[i],
            };
            self.prev_frame.insert(wid, snap);
        }
    }

    /// Borrow the per-frame cascade table. Pass to the renderer pipeline
    /// alongside `tree()` and `layout_result()`.
    pub fn cascades(&self) -> &Cascades {
        &self.cascades
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.layout_engine.rect(id)
    }

    pub fn layout_result(&self) -> &LayoutResult {
        self.layout_engine.result()
    }

    /// Feed a palantir-native input event. Backend-agnostic. Schedules a
    /// repaint conservatively: every input event sets the repaint gate
    /// because hover/press indices, hit-test rects, or response state
    /// may shift even when the event itself looks like a no-op.
    /// Refining this would require running a hit-test inside `on_input`
    /// (Stage 3 territory).
    pub fn on_input(&mut self, event: InputEvent) {
        self.input.on_input(event);
        self.repaint_requested = true;
    }

    /// True if the UI has changed since the last successful
    /// `end_frame()`. Hosts gate their `request_redraw()` /
    /// pipeline-run on this so idle frames cost nothing.
    ///
    /// Set by `on_input`, `set_scale_factor`, `request_repaint`,
    /// initial construction. Cleared by `end_frame()`.
    pub fn should_repaint(&self) -> bool {
        self.repaint_requested
    }

    /// Schedule a repaint on the next host tick. Idempotent and cheap.
    /// Use for animations, async state landing, theme changes, or any
    /// state mutation that doesn't flow through `on_input` /
    /// `set_scale_factor`.
    pub fn request_repaint(&mut self) {
        self.repaint_requested = true;
    }

    pub fn pointer(&self) -> PointerState {
        self.input.pointer()
    }

    pub fn root(&self) -> NodeId {
        self.root
            .expect("no root pushed yet — open a node before any other ops")
    }

    /// Borrow the recorded tree. Pass to the renderer pipeline or any other
    /// consumer that needs read access.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        self.input.response_for(id)
    }

    pub(crate) fn node(&mut self, element: Element, f: impl FnOnce(&mut Ui)) -> NodeId {
        let parent = self.parents.last().copied();
        let id = element.id;
        assert!(
            self.seen_ids.insert(id),
            "WidgetId collision — id {id:?} recorded twice this frame. Use `with_id(key)` (or `WidgetId::with`) to disambiguate widgets at the same call site, e.g. inside a loop. Duplicate ids silently corrupt focus, scroll, click capture, and hit-testing.",
        );
        let node = self.tree.push_node(element, parent);
        if self.root.is_none() {
            self.root = Some(node);
        }
        self.parents.push(node);
        f(self);
        self.parents.pop();
        node
    }

    pub(crate) fn add_shape(&mut self, shape: Shape) {
        if shape.is_noop() {
            return;
        }
        let node = *self
            .parents
            .last()
            .expect("add_shape called outside any open node");
        self.tree.add_shape(node, shape);
    }
}

#[cfg(test)]
mod tests;
