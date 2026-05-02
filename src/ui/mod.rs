mod damage;
mod seen_ids;
mod theme;
pub use theme::Theme;

pub(crate) use damage::Damage;
pub(crate) use seen_ids::SeenIds;

use crate::cascade::Cascades;
use crate::element::Element;
use crate::input::{InputEvent, InputState, ResponseState};
use crate::layout::LayoutEngine;
use crate::primitives::{Display, WidgetId};
use crate::renderer::{FrameOutput, Painter};
use crate::shape::Shape;
use crate::text::{SharedCosmic, TextMeasurer};
use crate::tree::{NodeId, Tree};

/// Recorder + input/response broker. Lives across frames; rebuilds the tree each frame
/// while persisting input state via [`InputState`].
///
/// All public coordinate inputs and recorded rects are in **logical pixels** (DIPs).
/// `scale_factor` is the conversion to physical pixels; the renderer applies it at
/// upload time. Pointer events from winit are converted at the boundary
/// (`handle_event` / `InputEvent::from_winit`).
pub struct Ui {
    pub(crate) tree: Tree,
    pub theme: Theme,

    /// Per-frame `WidgetId` tracker — collision detection,
    /// removed-widget diff, and frame rollover. See [`SeenIds`].
    pub(crate) ids: SeenIds,

    input: InputState,
    pub(crate) layout_engine: LayoutEngine,
    pub(crate) cascades: Cascades,
    pub(crate) display: Display,
    pub(crate) text: TextMeasurer,

    /// Defaults to `true` so the first frame always renders. Cleared by
    /// `end_frame`, set by `on_input` / `request_repaint`.
    repaint_requested: bool,

    /// Per-frame damage state. `Damage::compute` returns the filtered
    /// damage rect (`None` ⇒ full repaint, `Some(r)` ⇒ partial).
    pub(crate) damage: Damage,

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
            theme: Theme::default(),
            ids: SeenIds::default(),
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

    /// Install a shared shaper handle. Apps construct one [`SharedCosmic`]
    /// at startup and clone it into both `Ui` and the wgpu backend so they
    /// see the same buffer cache. Tests leave this unset and run on the
    /// deterministic mono fallback.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.text.set_cosmic(cosmic);
    }

    /// Start recording a frame. A stray `scale_factor` of `0.0` from winit
    /// would collapse the UI to a single physical pixel — assert against it.
    pub fn begin_frame(&mut self, display: Display) {
        assert!(
            display.scale_factor >= f32::EPSILON,
            "Display::scale_factor must be ≥ f32::EPSILON; got {}",
            display.scale_factor,
        );
        self.display = display;
        self.tree.clear();
        self.ids.begin_frame();
    }

    /// Read-only access to the recorded tree for benchmarks and
    /// inspection tools.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Finalize the just-recorded frame: measure + arrange, rebuild cascades
    /// and hit-index, compute hashes and damage, and encode + compose into
    /// the painter's `RenderBuffer`. Returns the painted output ready for
    /// `WgpuBackend::submit`. Clears the repaint gate.
    pub fn end_frame(&mut self) -> FrameOutput<'_> {
        let surface = self.display.logical_rect();
        // Hashes are pure functions of recorded inputs and don't depend on
        // layout output, so we compute them up front. Layout reads them to
        // skip text reshape for unchanged Text nodes (see
        // `docs/text-reshape-skip.md`); damage reads them after.
        self.tree.compute_hashes();
        self.ids.end_frame();
        self.text.sweep_removed(self.ids.removed());

        if let Some(root) = self.tree.root() {
            self.layout_engine
                .run(&self.tree, root, surface, &mut self.text);
        }
        self.cascades
            .rebuild(&self.tree, self.layout_engine.result());
        self.input.end_frame(&self.tree, &self.cascades);
        let damage = self
            .damage
            .compute(&self.tree, &self.cascades, self.ids.removed(), surface);
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

    /// Feed a palantir-native input event. Schedules a repaint
    /// conservatively: every event sets the gate because hover/press
    /// indices, hit-test rects, or response state may shift even when the
    /// event itself looks like a no-op. Refining this needs a hit-test
    /// inside `on_input`.
    pub fn on_input(&mut self, event: InputEvent) {
        self.input.on_input(event);
        self.repaint_requested = true;
    }

    /// True if the UI has changed since the last `end_frame`. Hosts gate
    /// `request_redraw` on this so idle frames cost nothing.
    pub fn should_repaint(&self) -> bool {
        self.repaint_requested
    }

    /// Schedule a repaint on the next host tick. Use for animations, async
    /// state landing, theme changes, or any mutation that doesn't flow
    /// through `on_input`.
    pub fn request_repaint(&mut self) {
        self.repaint_requested = true;
    }

    pub(crate) fn response_for(&self, id: WidgetId) -> ResponseState {
        self.input.response_for(id)
    }

    pub(crate) fn node(&mut self, element: Element, f: impl FnOnce(&mut Ui)) -> NodeId {
        let id = element.id;
        assert!(
            self.ids.record(id),
            "WidgetId collision — id {id:?} recorded twice this frame. Use `with_id(key)` (or `WidgetId::with`) to disambiguate widgets at the same call site, e.g. inside a loop. Duplicate ids silently corrupt focus, scroll, click capture, and hit-testing.",
        );
        let node = self.tree.open_node(element);
        f(self);
        self.tree.close_node();
        node
    }

    pub(crate) fn add_shape(&mut self, shape: Shape) {
        if shape.is_noop() {
            return;
        }
        let node = self
            .tree
            .current_open()
            .expect("add_shape called outside any open node");
        self.tree.add_shape(node, shape);
    }
}

#[cfg(test)]
mod tests;
