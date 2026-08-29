//! The flat, depth-shared scratch pool both stack drivers work out of.

/// A flat buffer shared by every nesting depth of one driver.
///
/// Each invocation takes a [`mark`](Self::mark) on entry, pushes its own
/// entries, works on [`since`](Self::since) that mark, and
/// [`truncate`](Self::truncate)s back to it on exit — so a nested stack
/// reuses the tail capacity its parent left, and steady state allocates
/// nothing.
///
/// Flat rather than a `Vec` per depth: one allocation for the whole
/// tree instead of one per level, which is the shape both drivers
/// needed and each had written out.
#[derive(Debug)]
pub(crate) struct DepthScratch<T> {
    pool: Vec<T>,
}

// Manual: a derived `Default` would demand `T: Default`, which neither
// element type owes anyone — an empty pool needs no element at all.
impl<T> Default for DepthScratch<T> {
    fn default() -> Self {
        Self { pool: Vec::new() }
    }
}

impl<T> DepthScratch<T> {
    /// Where this depth's entries begin — hand it back to
    /// [`Self::since`] and [`Self::truncate`].
    #[inline]
    pub(super) fn mark(&self) -> usize {
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
    /// One entry by absolute index.
    ///
    /// Copied out rather than borrowed: both drivers read an entry and
    /// then call into `&mut LayoutEngine`, which a live slice borrow of
    /// this pool would collide with.
    #[inline]
    pub(super) fn at(&self, index: usize) -> T {
        self.pool[index]
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::layout::depth_scratch::DepthScratch;

    impl<T> DepthScratch<T> {
        /// Whether the pool drained — what a driver's exit contract
        /// leaves behind, and the only thing a test asks of it.
        pub(crate) fn is_empty(&self) -> bool {
            self.pool.is_empty()
        }
    }
}
