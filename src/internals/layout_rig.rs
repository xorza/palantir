//! [`LayoutRig`]: layout alone. Nodes go into a forest with no widget, so no widget's default
//! moves an expectation, and the engine lays them out on the mono shaper. A bare
//! [`Widget`](crate::widget::Widget) builds a node, as it carries no widget's defaults: its
//! `Configure` setters are the whole node.

use crate::layout::Layout;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::len_req::LenReq;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::track::Track;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::reduce_preference::Reduction;
use crate::scene::endpoint::Endpoint;
use crate::scene::forest::{Forest, NodeChrome};
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::plane::Plane;
use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::text::glyph_font::GlyphFont;
use crate::text::shaped_text::ShapedText;
use crate::text::shaper::TextShaper;
use crate::text::wrap::TextWrap;
use crate::widget_core::widget::Widget;
use glam::Vec2;

/// A forest, the layout engine and the layout it writes, kept across runs as a window keeps them,
/// so a second run meets the first's measure cache.
#[derive(Debug)]
pub(crate) struct LayoutRig {
    forest: Forest,
    engine: LayoutEngine,
    layout: Layout,
}

/// Records nodes into a [`LayoutRig`]'s forest during a run.
#[derive(Debug)]
pub(crate) struct NodeRecorder<'a> {
    forest: &'a mut Forest,
}

impl LayoutRig {
    pub(crate) fn new() -> Self {
        let shaper = TextShaper::test_mono();
        Self {
            forest: Forest::new(shaper.family_keys()),
            engine: LayoutEngine::new(shaper),
            layout: Layout::default(),
        }
    }

    /// Record `body` under the viewport root a frame opens, and lay it out on a surface of
    /// `surface` logical px.
    pub(crate) fn run(&mut self, surface: Size, body: impl FnOnce(&mut NodeRecorder<'_>)) {
        self.forest.pre_record(Reduction::Full);
        let mut rec = NodeRecorder {
            forest: &mut self.forest,
        };
        rec.open(WidgetId::VIEWPORT, &Node::viewport());
        body(&mut rec);
        rec.forest.close_node();
        self.forest.post_record();
        self.engine.run(
            &self.forest,
            Rect {
                min: Vec2::ZERO,
                size: surface,
            },
            &mut self.layout,
        );
    }

    /// Where the node recorded under `id` was arranged.
    ///
    /// # Panics
    ///
    /// When the last run recorded no node under `id`.
    pub(crate) fn rect(&self, id: &str) -> Rect {
        let endpoint = self.endpoint(id);
        self.layout[endpoint.plane].rect[endpoint.node.idx()]
    }

    /// Where each child of the node recorded under `id` was arranged, in record order.
    pub(crate) fn child_rects(&self, id: &str) -> Vec<Rect> {
        let endpoint = self.endpoint(id);
        let rects = &self.layout[endpoint.plane].rect;
        self.forest.trees[endpoint.plane]
            .children(endpoint.node)
            .map(|child| rects[child.id.idx()])
            .collect()
    }

    /// The size the node recorded under `id` measured to in the last run.
    pub(crate) fn desired(&self, id: &str) -> Size {
        self.engine.cache.captured_desired()[self.endpoint(id).node.idx()]
    }

    /// The `req` content size on `axis` of the node recorded under `id`, as a parent's intrinsic
    /// query asks it.
    pub(crate) fn intrinsic(&mut self, id: &str, axis: Axis, req: LenReq) -> f32 {
        let node = self.endpoint(id).node;
        self.engine
            .main_intrinsic(&self.forest, node, IntrinsicQuery::single(axis, req))
            .get(req)
    }

    /// The tree the last run recorded on `plane`; a `Layer` names its base plane.
    pub(crate) fn tree(&self, plane: impl Into<Plane>) -> &Tree {
        &self.forest.trees[plane.into()]
    }

    /// The node recorded under `id`, for what a test reads of the engine's scratch.
    pub(crate) fn node(&self, id: &str) -> NodeId {
        self.endpoint(id).node
    }

    /// The layout engine, for what a test reads of its scratch.
    pub(crate) const fn engine(&self) -> &LayoutEngine {
        &self.engine
    }

    /// The engine beside the forest the last run recorded, for a test that writes the engine's
    /// scratch or runs an intrinsic pass of its own.
    pub(crate) const fn parts(&mut self) -> RigParts<'_> {
        RigParts {
            engine: &mut self.engine,
            forest: &self.forest,
        }
    }

    /// The content extent the scroll node recorded under `id` measured, as its bars read it.
    pub(crate) fn scroll_content(&self, id: &str) -> Size {
        let endpoint = self.endpoint(id);
        self.layout[endpoint.plane].scroll_content[endpoint.node.idx()]
    }

    /// The one text shape the node recorded under `id` holds, as the last run shaped it.
    pub(crate) fn shaped_text(&self, id: &str) -> ShapedText {
        let endpoint = self.endpoint(id);
        let plane = &self.layout[endpoint.plane];
        let span = plane.text_spans[endpoint.node.idx()];
        assert_eq!(span.len, 1, "{id:?} holds one text shape");
        plane.text_shapes[span.start as usize]
    }

    fn endpoint(&self, id: &str) -> Endpoint {
        self.forest
            .ids
            .endpoint(WidgetId::from_hash(id))
            .unwrap_or_else(|| panic!("no node {id:?} was recorded"))
    }
}

impl NodeRecorder<'_> {
    /// Record `widget`'s node under the id `id`, its children recorded by `body`.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a builder chain hands the widget by value, and its node is `Copy`"
    )]
    pub(crate) fn node(&mut self, id: &str, widget: Widget, body: impl FnOnce(&mut Self)) {
        self.open(WidgetId::from_hash(id), &widget.node);
        body(self);
        self.forest.close_node();
    }

    /// Record `widget`'s node, with no children, under the id `id`.
    pub(crate) fn leaf(&mut self, id: &str, widget: Widget) {
        self.node(id, widget, |_| {});
    }

    /// Record `widget`, a [`Widget::grid`], over `tracks` under the id `id`.
    pub(crate) fn grid(
        &mut self,
        id: &str,
        mut widget: Widget,
        tracks: GridTracks<'_>,
        body: impl FnOnce(&mut Self),
    ) {
        let def = self.forest.push_grid_def(tracks.rows, tracks.cols);
        widget.node.set_mode(LayoutMode::Grid(def));
        self.node(id, widget, body);
    }

    /// Record `widget`, a [`Widget::scrollbars`] overlay over the viewport `def` names, which
    /// this run recorded, under the id `id`.
    pub(crate) fn scrollbars(
        &mut self,
        id: &str,
        mut widget: Widget,
        def: ScrollbarsDef,
        body: impl FnOnce(&mut Self),
    ) {
        let def = self.forest.push_scrollbars_def(def);
        widget.node.set_mode(LayoutMode::Scrollbars(def));
        self.node(id, widget, body);
    }

    /// Record `widget`'s node under the id `id`, holding `run` and no children.
    pub(crate) fn text_leaf(&mut self, id: &str, widget: Widget, run: TextRun<'_>) {
        self.node(id, widget, |rec| {
            let text = rec.forest.record_store.intern(run.text);
            let font = GlyphFont::new(run.px);
            rec.forest.add_shape(Shape::text(text, font).wrap(run.wrap));
        });
    }

    fn open(&mut self, id: WidgetId, node: &Node) {
        let resolved = self.forest.widget_id(Ident::Verbatim(id));
        self.forest.open_node(
            resolved,
            node,
            NodeChrome {
                chrome: None,
                ring: Stroke::NONE,
            },
        );
    }
}

/// A run of text in the rig's mono face: `px / 2` a char, led at `px`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextRun<'a> {
    text: &'a str,
    px: f32,
    wrap: TextWrap,
}

impl<'a> TextRun<'a> {
    pub(crate) const fn new(text: &'a str, px: f32, wrap: TextWrap) -> Self {
        Self { text, px, wrap }
    }
}

/// A [`LayoutRig`]'s engine and forest, borrowed apart.
#[derive(Debug)]
pub(crate) struct RigParts<'a> {
    pub(crate) engine: &'a mut LayoutEngine,
    pub(crate) forest: &'a Forest,
}

/// The rows and the columns of a grid [`NodeRecorder::grid`] records.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GridTracks<'a> {
    pub(crate) rows: &'a [Track],
    pub(crate) cols: &'a [Track],
}
