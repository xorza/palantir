//! The per-node sizing column every measure and arrange pass reads.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::packed_layout_meta::PackedLayoutMeta;
use crate::primitives::layout::sizing::SizeSpec;
use crate::scene::node::Node;
use crate::scene::node::node_flags::NodeFlags;
use std::hash;
use std::hash::Hash;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayoutCore {
    pub(crate) size: SizeSpec,
    pub(crate) padding: Spacing,
    pub(crate) margin: Spacing,
    pub(crate) meta: PackedLayoutMeta,
}

// SAFETY: `repr(C)` over four plain-integer fields (`SizeSpec`'s two `u32`, two `Spacing`s of `u16` lanes, `PackedLayoutMeta`'s `u32`) whose sizes sum to the struct's: no padding.
unsafe impl bytemuck::NoUninit for LayoutCore {}

const _: () = assert!(
    size_of::<LayoutCore>() == 2 * size_of::<u32>() + 2 * size_of::<Spacing>() + size_of::<u32>()
);

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

    /// The box this node's content lives in: `rect` less padding, in `rect`'s space.
    ///
    /// **Four passes must agree**: arrange places children in it, container-text wraps to its width, cascade clips shapes and descendant damage to it, and the encoder pushes it as the clip mask. `Tree::open_node` already folds a chrome stroke's ring into `padding`.
    #[inline]
    pub(crate) fn inner_rect(&self, rect: Rect) -> Rect {
        rect.deflated_by(self.padding)
    }

    /// Fold this column and the node's flags into one hash. A method, not `Hash`, because the flags live in their own column and folding them into this tail word saves a write on a per-node path.
    #[inline]
    pub(crate) fn hash_with_flags<H: hash::Hasher>(&self, flags: NodeFlags, h: &mut H) {
        h.write_u64(self.size.as_u64());
        h.write_u64(self.padding.as_u64());
        h.write_u64(self.margin.as_u64());
        let mode = self.meta.into();
        // Shifted, not byte-cast, like [`Gaps::as_u32`](crate::scene::node::gaps::Gaps::as_u32): avoids a layout-dependent hash.
        let tail = u64::from(self.meta.metadata())
            | (u64::from(self.meta.tag()) << 8)
            | (u64::from(flags.bits()) << 16);
        h.write_u64(tail);
        if let LayoutMode::Scroll(spec) = mode {
            spec.hash(h);
        }
    }
}
