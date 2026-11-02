//! [`LayoutRig`]: layout alone. Nodes go into a forest with no widget, so no widget's default
//! moves an expectation, and the engine lays them out on the mono shaper.

use crate::layout::Layout;
use crate::layout::engine::LayoutEngine;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::layout_mode::{LayoutMode, ScrollbarsDefId};
use crate::primitives::paint::stroke::Stroke;
use crate::scene::forest::{Forest, NodeChrome};
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::node::node_mode::NodeMode;
use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::text::shaper::TextShaper;
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
        self.forest.pre_record();
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
        let endpoint = self
            .forest
            .ids
            .endpoint(WidgetId::from_hash(id))
            .unwrap_or_else(|| panic!("no node {id:?} was recorded"));
        self.layout[endpoint.plane].rect[endpoint.node.idx()]
    }
}

impl NodeRecorder<'_> {
    /// Record `node` under the id `id`, its children recorded by `body`.
    pub(crate) fn node(&mut self, id: &str, node: Node, body: impl FnOnce(&mut Self)) {
        self.open(WidgetId::from_hash(id), &node);
        body(self);
        self.forest.close_node();
    }

    /// Record `node`, with no children, under the id `id`.
    pub(crate) fn leaf(&mut self, id: &str, node: Node) {
        self.node(id, node, |_| {});
    }

    /// Intern `def`, whose viewport this run recorded, for a scrollbar overlay to name.
    pub(crate) fn scrollbars(&mut self, def: ScrollbarsDef) -> ScrollbarsDefId {
        self.forest.push_scrollbars_def(def)
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

/// A node of `mode`, every other field unset.
pub(crate) fn node(mode: LayoutMode) -> Node {
    Node::new(NodeMode::Resolved(mode))
}
