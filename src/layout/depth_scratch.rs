//! The flat, depth-shared scratch pool both stack drivers work out of.

/// A flat buffer shared by every nesting depth of one driver.
///
/// Each invocation takes a [`mark`](Self::mark), pushes entries, works on [`since`](Self::since) it, and [`truncate`](Self::truncate)s back on exit, so a nested stack reuses its parent's tail capacity and steady state allocates nothing. One allocation for the tree, not one per level.
#[derive(Debug)]
pub(crate) struct DepthScratch<T> {
    pool: Vec<T>,
}

// Manual: a derived `Default` would demand `T: Default`, which an empty pool doesn't need.
impl<T> Default for DepthScratch<T> {
    fn default() -> Self {
        Self { pool: Vec::new() }
    }
}

impl<T> DepthScratch<T> {
    /// Where this depth's entries begin; hand back to [`Self::since`] and [`Self::truncate`].
    #[inline]
    pub(super) const fn mark(&self) -> usize {
        self.pool.len()
    }

    #[inline]
    pub(super) fn push(&mut self, item: T) {
        self.pool.push(item);
    }

    /// This depth's own entries, as a mutable slice.
    #[inline]
    pub(super) fn since(&mut self, mark: usize) -> &mut [T] {
        &mut self.pool[mark..]
    }

    /// Drop everything pushed since `mark`.
    #[inline]
    pub(super) fn truncate(&mut self, mark: usize) {
        self.pool.truncate(mark);
    }
}

impl<T: Copy> DepthScratch<T> {
    /// One entry by absolute index, copied out because drivers then call into `&mut LayoutEngine`, which a slice borrow would collide with.
    #[inline]
    pub(super) fn at(&self, index: usize) -> T {
        self.pool[index]
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::layout::depth_scratch::DepthScratch;

    impl<T> DepthScratch<T> {
        /// Whether the pool drained, which a driver's exit contract guarantees.
        pub(crate) fn is_empty(&self) -> bool {
            self.pool.is_empty()
        }
    }
}
