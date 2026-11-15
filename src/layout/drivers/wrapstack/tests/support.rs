//! The cells and the outer row the wrapstack cases share.

use crate::internals::layout_rig::NodeRecorder;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::SizeSpec;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;

/// A leaf fixed at `size`, under the id `id`.
pub(super) fn cell(rec: &mut NodeRecorder<'_>, id: &str, size: Size) {
    rec.leaf(id, Widget::leaf().size((size.w, size.h)));
}

/// A Fill hstack around `body`, as an app's root row.
pub(super) fn outer(rec: &mut NodeRecorder<'_>, body: impl FnOnce(&mut NodeRecorder<'_>)) {
    rec.node("outer", Widget::hstack().size(SizeSpec::FILL), body);
}
