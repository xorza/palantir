//! One recorded plane root.

use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::plane::Plane;

/// One root a plane recorded, in paint order across every plane, for Tab
/// traversal to find the topmost open modal when it holds no stop. Same
/// lifecycle as [`ScopeRow`](crate::cascade::rows::scope_row::ScopeRow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RootRow {
    pub(crate) plane: Plane,
    /// The layer the root asked for, which focus rules read; paint and hit order read
    /// [`Self::plane`]. A dropdown nested in a dialog records on a `Modal` plane and stays a `Popup`.
    pub(crate) layer: Layer,
    pub(crate) id: WidgetId,
}
