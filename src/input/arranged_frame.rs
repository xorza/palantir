//! The tables a widget's response reads its geometry from.

use crate::cascade::Cascade;
use crate::layout::Layout;

/// Last frame's cascade and layout, read together: the cascade locates a widget and holds its transform, the layout its arranged rect.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ArrangedFrame<'a> {
    pub(crate) cascade: &'a Cascade,
    pub(crate) layout: &'a Layout,
}
