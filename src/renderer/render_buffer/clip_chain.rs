//! What a draw names of the frame's rounded clips.

use crate::common::span::Span;
use crate::primitives::wgsl;

/// The rounded clips that cut a draw: a run of [`RenderBuffer::rounded_clips`], outer to inner,
/// packed into the one `u32` its instance carries, the depth in the low [`Self::DEPTH_BITS`] and
/// the start above them. [`Self::NONE`] cuts nothing.
///
/// [`RenderBuffer::rounded_clips`]: crate::renderer::render_buffer::RenderBuffer::rounded_clips
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ClipChain(u32);

impl ClipChain {
    /// The bits that hold the depth.
    pub(crate) const DEPTH_BITS: u32 = wgsl::CLIP_DEPTH_BITS;

    /// The deepest chain.
    pub(crate) const MAX_DEPTH: u32 = (1 << Self::DEPTH_BITS) - 1;

    /// The masks a frame's pool holds at most: a chain starts below it.
    pub(crate) const MAX_POOL: u32 = 1 << (u32::BITS - Self::DEPTH_BITS);

    /// No clip.
    pub(crate) const NONE: Self = Self(0);

    /// The chain `span` of the pool holds; an empty span is [`Self::NONE`].
    ///
    /// # Panics
    ///
    /// Panics where `span` is deeper than [`Self::MAX_DEPTH`] or starts at [`Self::MAX_POOL`] or
    /// past it, which the composer refuses where it builds the chain.
    pub(crate) const fn new(span: Span) -> Self {
        if span.len == 0 {
            return Self::NONE;
        }
        assert!(span.len <= Self::MAX_DEPTH, "a chain fits its depth bits");
        assert!(
            span.start < Self::MAX_POOL,
            "a chain starts inside the pool"
        );
        Self((span.start << Self::DEPTH_BITS) | span.len)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::common::span::Span;
    use crate::renderer::render_buffer::clip_chain::ClipChain;

    impl ClipChain {
        /// The run of the pool this chain names, empty for [`ClipChain::NONE`].
        pub(crate) const fn span(self) -> Span {
            Span::new(self.0 >> Self::DEPTH_BITS, self.0 & Self::MAX_DEPTH)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::renderer::render_buffer::clip_chain::ClipChain;

    /// A chain packs its start above its depth: start 5, depth 3 is `5 << 8 | 3 = 1283`; the
    /// deepest chain at the last start packs every bit; an empty span is no clip, whatever its
    /// start, and no clip is zero, what a zeroed instance holds.
    #[test]
    fn a_chain_packs_its_span() {
        assert_eq!(ClipChain::new(Span::new(5, 3)), ClipChain(1283));
        assert_eq!(ClipChain(1283).span(), Span::new(5, 3));
        let last = Span::new(ClipChain::MAX_POOL - 1, ClipChain::MAX_DEPTH);
        assert_eq!(ClipChain::new(last), ClipChain(u32::MAX));
        assert_eq!(ClipChain::new(Span::new(7, 0)), ClipChain::NONE);
        assert_eq!(ClipChain::NONE, ClipChain(0));
    }

    #[test]
    #[should_panic(expected = "a chain fits its depth bits")]
    fn a_chain_past_its_depth_bits_panics() {
        ClipChain::new(Span::new(0, ClipChain::MAX_DEPTH + 1));
    }

    #[test]
    #[should_panic(expected = "a chain starts inside the pool")]
    fn a_chain_past_the_pool_panics() {
        ClipChain::new(Span::new(ClipChain::MAX_POOL, 1));
    }
}
