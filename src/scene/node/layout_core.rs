//! The per-node sizing column every measure and arrange pass reads.

use crate::layout::types::layout_mode::{LayoutMode, PackedLayoutMeta};
use crate::layout::types::sizing::Sizes;
use crate::primitives::rect::Rect;
use crate::primitives::spacing::Spacing;
use crate::scene::node::Node;
use crate::scene::node::node_flags::NodeFlags;
use std::hash::Hash;

#[derive(Clone, Copy, Debug)]
pub(crate) struct LayoutCore {
    pub(crate) size: Sizes,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,
    pub(crate) meta: PackedLayoutMeta,
}

impl LayoutCore {
    pub(super) fn from_node(node: &Node) -> Self {
        let mode = node.mode.resolved();
        Self {
            size: node.size.unwrap_or_default(),
            padding: node.padding.unwrap_or(Spacing::ZERO),
            margin: node.margin.unwrap_or(Spacing::ZERO),
            meta: PackedLayoutMeta::new(mode, node.align, node.visibility),
        }
    }

    /// The box this node's own content lives in: `rect` less this node's
    /// padding, in whatever space `rect` is given in.
    ///
    /// **Four passes ask it and must agree.** Arrange places children in
    /// it, the container-text pass wraps a run to its width, the cascade
    /// clips direct shapes and descendant damage to it, and the encoder
    /// pushes it as the clip mask. `Tree::open_node` has already folded a
    /// chrome stroke's ring into `padding`, so all four sit inside the
    /// painted ring without any of them knowing about the stroke.
    #[inline]
    pub(crate) fn inner_rect(&self, rect: Rect) -> Rect {
        rect.deflated_by(self.padding)
    }

    /// Fold this column and the node's flags into one hash.
    ///
    /// A method rather than a `Hash` impl — unlike the sibling columns —
    /// because the flags live in a column of their own, and folding them
    /// into this one's tail word is what makes the pair three writes
    /// instead of four on a per-node path.
    #[inline]
    pub(crate) fn hash_with_flags<H: std::hash::Hasher>(&self, flags: NodeFlags, h: &mut H) {
        h.write_u64(self.size.as_u64());
        h.write_u64(self.padding.as_u64());
        h.write_u64(self.margin.as_u64());
        let mode = self.meta.into();
        // Shifted rather than byte-cast, like the sibling
        // [`Gaps::as_u32`](crate::scene::node::gaps::Gaps::as_u32):
        // the key never leaves the process, but a layout-dependent hash
        // is a trap worth not setting.
        let tail = u32::from(self.meta.metadata())
            | (u32::from(self.meta.tag()) << 8)
            | (u32::from(flags.bits()) << 16);
        h.write_u32(tail);
        if let LayoutMode::Scroll(spec) = mode {
            spec.hash(h);
        }
    }
}
