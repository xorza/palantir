mod damage;
mod theme;
pub use theme::ButtonTheme;

pub(crate) use damage::Damage;

use crate::cascade::Cascades;
use crate::element::Element;
use crate::input::{InputEvent, InputState, ResponseState};
use crate::layout::LayoutEngine;
use crate::primitives::{Display, WidgetId};
use crate::renderer::{FrameOutput, Painter};
use crate::shape::Shape;
use crate::text::{SharedCosmic, TextMeasurer};
use crate::tree::{NodeId, Tree};
use rustc_hash::FxHashSet;

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

    /// Per-frame collision set: every `WidgetId` that has been recorded this
    /// frame. Cleared in `begin_frame`. Used to enforce id uniqueness — a
    /// repeat insertion in `Ui::node` is a release-`assert!` panic, not a
    /// warning, because duplicate ids silently corrupt every per-id store
    /// (focus, scroll, click capture, hit-test rect lookup).
    seen_ids: FxHashSet<WidgetId>,

    input: InputState,
    /// Persistent layout engine: holds reusable scratch buffers across frames.
    /// Run inside `end_frame`.
    pub(crate) layout_engine: LayoutEngine,
    /// Per-frame cascade resolution shared by the renderer encoder (skip
    /// invisible subtrees) and the input hit index (screen rects + sense).
    /// Rebuilt in `end_frame`.
    pub(crate) cascades: Cascades,

    /// Display config (physical surface size + DPR + pixel-snap) for
    /// the current frame, written by [`Ui::begin_frame`]. Read by
    /// `end_frame` (layout, damage, painter) and by hosts converting
    /// input coords. Defaults to `Display::default()` until the
    /// first `begin_frame`.
    pub(crate) display: Display,

    /// Text shaping & measurement, with the `CosmicMeasure` / mono dispatch
    /// hidden inside. Install a real shaper with [`Ui::install_text_system`]
    /// to get shaping + rendering; otherwise runs use the mono placeholder.
    pub(crate) text: TextMeasurer,

    /// Frame-skipping gate. `true` when something has changed since the
    /// last successful `end_frame()`; the host calls
    /// [`Ui::should_repaint`] to decide whether to run the pipeline this
    /// tick. Set conservatively by [`Ui::on_input`], or explicitly by
    /// [`Ui::request_repaint`] (animations, async, surface/scale
    /// changes). Cleared at the end of `end_frame()` so the next
    /// idle check returns `false`. Defaults to `true` so the first
    /// frame always renders.
    repaint_requested: bool,

    /// Per-frame damage output (`dirty`, `rect`) plus the
    /// cross-frame snapshot map it diffs against (`damage.prev`).
    /// Computed in `end_frame()` after `compute_hashes()`. The
    /// partial-vs-full repaint decision is made lazily by
    /// `Damage::filter(surface)` at submit time.
    pub(crate) damage: Damage,

    /// Encode + compose stage. Owns the `RenderCmd` vec, composer
    /// scratch, and `RenderBuffer`. Run by `end_frame()` after
    /// damage is computed; output read via [`Ui::frame`].
    painter: Painter,
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
            seen_ids: FxHashSet::default(),
            input: InputState::new(),
            layout_engine: LayoutEngine::new(),
            cascades: Cascades::new(),
            display: Display::default(),
            text: TextMeasurer::new(),
            // First frame must render so the host has something to
            // present. Subsequent idle frames flip back to `false`.
            repaint_requested: true,
            damage: Damage::default(),
            painter: Painter::new(),
        }
    }

    /// Install a shared shaper handle. Apps construct one
    /// [`SharedCosmic`] at startup and clone it into both `Ui` and the wgpu
    /// backend so they see the same buffer cache. Tests usually leave this
    /// unset and run on the deterministic mono fallback.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.text.set_cosmic(cosmic);
    }

    /// Start recording a frame. `display` is the canvas size + DPR
    /// for this frame; stashed and read by `end_frame` for layout,
    /// damage, and painter. Asserts `display.scale_factor >=
    /// f32::EPSILON` — a stray `0.0` from winit would collapse the
    /// UI to a single physical pixel.
    pub fn begin_frame(&mut self, display: Display) {
        assert!(
            display.scale_factor >= f32::EPSILON,
            "Display::scale_factor must be ≥ f32::EPSILON; got {}",
            display.scale_factor,
        );
        self.display = display;
        self.tree.clear();
        self.parents.clear();
        self.seen_ids.clear();
    }

    /// Borrow the recorded tree. Read-only access for benchmarks and
    /// inspection tools that need to walk the recorded structure
    /// (`node_count`, `widget_ids`, etc.) outside the layout/render
    /// pipeline.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Run measure + arrange in isolation. Used by the
    /// `layout_only` benchmark to time the layout pass without the
    /// surrounding cascade/damage/encode work; `end_frame` does the
    /// equivalent step internally and apps don't normally need to
    /// call this.
    pub fn layout(&mut self) {
        if let Some(root) = self.tree.root() {
            self.layout_engine.run(
                &self.tree,
                root,
                self.display.logical_rect(),
                &mut self.text,
            );
        }
    }

    /// Finalize the just-recorded frame: run measure + arrange,
    /// rebuild cascades and hit-index, compute hashes and damage,
    /// and encode + compose into the painter's `RenderBuffer`.
    /// Returns the painted output ready for `WgpuBackend::submit`.
    ///
    /// All passes use the surface from
    /// `display.logical_rect()` as set by [`Ui::begin_frame`].
    ///
    /// Clears the repaint gate so the next [`Ui::should_repaint`]
    /// returns `false` until something new happens (input, animation
    /// tick, explicit `request_repaint`).
    pub fn end_frame(&mut self) -> FrameOutput<'_> {
        let surface = self.display.logical_rect();
        self.layout();
        self.cascades
            .rebuild(&self.tree, self.layout_engine.result());
        self.input.end_frame(&self.tree, &self.cascades);
        self.tree.compute_hashes();
        self.damage
            .compute(&self.tree, &self.cascades, &self.seen_ids);
        let damage = self.damage.filter(surface);
        self.painter.build(
            &self.tree,
            self.layout_engine.result(),
            &self.cascades,
            self.theme.disabled_dim,
            damage,
            &self.display,
        );
        self.repaint_requested = false;
        FrameOutput {
            buffer: self.painter.buffer(),
            damage,
        }
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
    /// Set by `on_input`, `begin_frame`, `request_repaint`,
    /// initial construction. Cleared by `end_frame()`.
    pub fn should_repaint(&self) -> bool {
        self.repaint_requested
    }

    /// Schedule a repaint on the next host tick. Idempotent and cheap.
    /// Use for animations, async state landing, theme changes, or any
    /// state mutation that doesn't flow through `on_input` /
    /// `begin_frame`.
    pub fn request_repaint(&mut self) {
        self.repaint_requested = true;
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
