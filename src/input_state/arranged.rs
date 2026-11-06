//! [`Arranged`]: the latest arranged frame, which an input event means something against.

use crate::cascade::Cascade;
use crate::layout::Layout;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;

/// The latest record pass's cascade and layout, read together: the cascade locates a widget and
/// holds its transform, the layout its arranged rect. The engines write them after each pass.
#[derive(Debug, Default)]
pub(crate) struct Arranged {
    pub(crate) cascade: Cascade,
    pub(crate) layout: Layout,
}

impl Arranged {
    /// The content extent the scroll viewport `id` measured in that pass, before zoom. `Size::ZERO`
    /// for a non-viewport or a widget not yet laid out.
    #[inline]
    pub(crate) fn scroll_content(&self, id: WidgetId) -> Size {
        // Bridge: `Layout` keys by `(layer, node)`, callers hold a `WidgetId`.
        self.cascade
            .endpoint(id)
            .map_or(Size::ZERO, |endpoint| self.layout.scroll_content(endpoint))
    }
}
