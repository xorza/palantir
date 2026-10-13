//! Where a keyboard read happens, as scope routing answers it.

use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;

/// The record position a chord is read from: the layer an overlay's scope can silence, and the parent whose enclosing scope the read speaks for (`None` outside every node, where the layer's outermost scope answers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyReader {
    pub(crate) layer: Layer,
    pub(crate) parent: Option<WidgetId>,
}
