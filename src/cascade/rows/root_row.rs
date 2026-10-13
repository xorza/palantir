//! One recorded layer root.

use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;

/// One root a layer recorded, in record order across every layer, for Tab
/// traversal to find the topmost open modal when it holds no stop. Same
/// lifecycle as [`ScopeRow`](crate::cascade::rows::scope_row::ScopeRow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RootRow {
    pub(crate) layer: Layer,
    pub(crate) id: WidgetId,
}
