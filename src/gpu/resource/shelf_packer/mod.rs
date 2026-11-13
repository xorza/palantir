//! The rectangle packer of one square atlas texture: shelves of one height class each, their free
//! spans kept exact, so a freed rectangle is room again at once, and a grow keeps every placement.

use glam::U16Vec2;
use std::ops::Range;

#[cfg(feature = "internals")]
pub(crate) mod bench;

/// Shelves of rectangles packed left to right, stacked from the top of a square side.
///
/// Reclaim is per rectangle: freeing one returns its span to its shelf, merged with its neighbors,
/// so an evicted rectangle is room for the next one its shelf takes. A bucketed packer reclaims
/// only whole buckets, which made an atlas evict raster after raster before any space came back.
/// A shelf that empties merges with the empty shelves beside it, and a rectangle too tall for
/// every shelf class takes the shortest empty shelf that holds it, split to its height, as
/// étagère's shelf allocator does: the rows of emptied shelves serve any height again.
#[derive(Debug)]
pub(crate) struct ShelfPacker {
    /// The side's edge in texels.
    edge: u16,
    /// Sorted by `y`; together they cover `0..top`.
    shelves: Vec<Shelf>,
    /// The first row no shelf holds: everything below is unclaimed.
    top: u16,
    /// The span lists of shelves that went, cleared and kept for the next shelf to open, so a
    /// shelf allocates nothing once the packer has warmed up.
    spare: Vec<Vec<Range<u16>>>,
}

/// One shelf: where it starts, how tall it is, and its free spans.
///
/// Each shelf keeps a list of its own, as a free or a carve moves only the spans after it in that
/// list. Refused: one list of every shelf's spans sorted by row, which moves every span after it
/// in the atlas, about 2,500 at 8,000 glyphs, and took the churn of `atomic/shelf_packer` from
/// 65 / 128 ns to 98 / 243 ns (1,000 / 8,000 live rectangles, an Apple laptop).
#[derive(Debug)]
pub(crate) struct Shelf {
    pub(crate) y: u16,
    pub(crate) height: u16,
    /// The width of the widest free span, so a search passes a shelf with no room with no look at
    /// its spans; the side's edge only on an empty shelf.
    widest: u16,
    /// Free spans of the shelf's row, sorted, disjoint and never touching: a free next to a free
    /// span merges with it.
    free: Vec<Range<u16>>,
}

impl ShelfPacker {
    /// The largest edge a packer counts, in texels.
    pub(crate) const MAX_EDGE: u32 = u16::MAX as u32;

    pub(crate) const fn new(edge: u16) -> Self {
        Self {
            edge,
            shelves: Vec::new(),
            top: 0,
            spare: Vec::new(),
        }
    }

    /// A packer of `edge` texels, held to [`Self::MAX_EDGE`].
    pub(crate) const fn within(edge: u32) -> Self {
        Self::new(if edge < Self::MAX_EDGE {
            edge as u16
        } else {
            u16::MAX
        })
    }

    /// The side's edge in texels.
    pub(crate) const fn edge(&self) -> u16 {
        self.edge
    }

    /// The top-left of a free rectangle of `size`, now taken; `None` when no shelf and no
    /// unclaimed row has room. The lowest shelf that takes the rectangle and has a span wide
    /// enough wins, then a new shelf, then the shortest empty shelf that holds it, split to its
    /// height.
    pub(crate) fn allocate(&mut self, size: U16Vec2) -> Option<U16Vec2> {
        let (w, h) = (size.x, size.y);
        debug_assert!(w > 0 && h > 0, "a rectangle with no texels owns no room");
        if w > self.edge || h > self.edge {
            return None;
        }
        let best = self
            .shelves
            .iter()
            .enumerate()
            .filter(|(_, shelf)| shelf.widest >= w && shelf.takes(h))
            .map(|(i, shelf)| (shelf.height, i))
            .min();
        if let Some((_, shelf)) = best {
            return Some(self.carve(shelf, w));
        }
        let height = shelf_height(h).min(self.edge - self.top);
        if h <= height {
            let y = self.top;
            self.top += height;
            let shelf = self.open(y, height);
            self.shelves.push(shelf);
            return Some(self.carve(self.shelves.len() - 1, w));
        }
        let (_, empty) = self
            .shelves
            .iter()
            .enumerate()
            .filter(|(_, shelf)| h <= shelf.height && self.is_empty(shelf))
            .map(|(i, shelf)| (shelf.height, i))
            .min()?;
        self.split(empty, shelf_height(h));
        Some(self.carve(empty, w))
    }

    /// A shelf at row `y` of `height`, its whole row free, on a span list kept from a shelf
    /// that went.
    fn open(&mut self, y: u16, height: u16) -> Shelf {
        let mut free = self.spare.pop().unwrap_or_default();
        free.push(0..self.edge);
        Shelf {
            y,
            height,
            widest: self.edge,
            free,
        }
    }

    /// Keep `shelf`'s span list for the next shelf to open.
    fn retire(&mut self, shelf: Shelf) {
        let mut free = shelf.free;
        free.clear();
        self.spare.push(free);
    }

    /// Cut empty shelf `i` to `height` rows, its rows past them an empty shelf of their own, when
    /// it is taller.
    fn split(&mut self, i: usize, height: u16) {
        let Shelf {
            y, height: whole, ..
        } = self.shelves[i];
        if whole <= height {
            return;
        }
        let rest = self.open(y + height, whole - height);
        self.shelves[i].height = height;
        self.shelves.insert(i + 1, rest);
    }

    /// Take `w` texels off the front of shelf `i`'s first span that wide.
    fn carve(&mut self, i: usize, w: u16) -> U16Vec2 {
        let shelf = &mut self.shelves[i];
        let at = shelf
            .free
            .iter()
            .position(|span| width_of(span) >= w)
            .expect("a shelf has a span as wide as its widest");
        let span = &mut shelf.free[at];
        let x = span.start;
        let was = width_of(span);
        span.start += w;
        if span.start == span.end {
            shelf.free.remove(at);
        }
        if was == shelf.widest {
            shelf.widest = shelf.free.iter().map(width_of).max().unwrap_or(0);
        }
        U16Vec2::new(x, shelf.y)
    }

    /// Return the rectangle of `width` at `origin`, as [`Self::allocate`] placed it. A shelf it
    /// leaves empty merges with the empty shelves beside it, and empty shelves at the bottom go
    /// back to the unclaimed rows.
    pub(crate) fn deallocate(&mut self, origin: U16Vec2, width: u16) {
        let shelf = self.shelf_at(origin.y);
        debug_assert_eq!(
            self.shelves[shelf].y, origin.y,
            "a rectangle sits at its shelf's row"
        );
        let freed = origin.x..origin.x + width;
        let free = &mut self.shelves[shelf].free;
        let at = free.partition_point(|span| span.start < freed.start);
        debug_assert!(
            free.get(at).is_none_or(|next| freed.end <= next.start)
                && at
                    .checked_sub(1)
                    .is_none_or(|prev| free[prev].end <= freed.start),
            "a freed rectangle {freed:?} overlaps a free span",
        );
        let joins_prev = at > 0 && free[at - 1].end == freed.start;
        let joins_next = free.get(at).is_some_and(|next| next.start == freed.end);
        let merged = match (joins_prev, joins_next) {
            (true, true) => {
                free[at - 1].end = free[at].end;
                free.remove(at);
                at - 1
            }
            (true, false) => {
                free[at - 1].end = freed.end;
                at - 1
            }
            (false, true) => {
                free[at].start = freed.start;
                at
            }
            (false, false) => {
                free.insert(at, freed);
                at
            }
        };
        let merged = width_of(&free[merged]);
        let widest = &mut self.shelves[shelf].widest;
        *widest = (*widest).max(merged);
        if self.is_empty(&self.shelves[shelf]) {
            self.merge_empty(shelf);
        }
        while self
            .shelves
            .last()
            .is_some_and(|shelf| self.is_empty(shelf))
        {
            let shelf = self.shelves.pop().expect("a last shelf");
            self.top = shelf.y;
            self.retire(shelf);
        }
    }

    /// Whether nothing sits on `shelf`: only an empty shelf has a span as wide as the side.
    const fn is_empty(&self, shelf: &Shelf) -> bool {
        shelf.widest == self.edge
    }

    /// Merge empty shelf `i` with the empty shelves just above and below it.
    fn merge_empty(&mut self, i: usize) {
        let mut i = i;
        if i > 0 && self.is_empty(&self.shelves[i - 1]) {
            let shelf = self.shelves.remove(i);
            self.shelves[i - 1].height += shelf.height;
            self.retire(shelf);
            i -= 1;
        }
        if self
            .shelves
            .get(i + 1)
            .is_some_and(|next| self.is_empty(next))
        {
            let next = self.shelves.remove(i + 1);
            self.shelves[i].height += next.height;
            self.retire(next);
        }
    }

    /// The shelf a rectangle at row `y` sits on.
    pub(crate) fn shelf(&self, y: u16) -> &Shelf {
        &self.shelves[self.shelf_at(y)]
    }

    /// Every shelf, top to bottom.
    pub(crate) fn shelves(&self) -> &[Shelf] {
        &self.shelves
    }

    /// The number of the shelf a rectangle at row `y` sits on.
    pub(crate) fn shelf_at(&self, y: u16) -> usize {
        let at = self.shelves.partition_point(|shelf| shelf.y <= y);
        debug_assert!(at > 0, "row {y} sits on a shelf");
        at - 1
    }

    /// Free every rectangle at once, keeping each shelf's span list for the next shelf to open.
    pub(crate) fn clear(&mut self) {
        while let Some(shelf) = self.shelves.pop() {
            self.retire(shelf);
        }
        self.top = 0;
    }

    /// Free every rectangle and take an edge of `edge`, keeping the span lists, so a packer reused
    /// for trials allocates nothing once warm.
    pub(crate) fn reset(&mut self, edge: u16) {
        self.clear();
        self.edge = edge;
    }

    /// Widen and deepen the side to `edge`, keeping every placement: each shelf gains the new
    /// columns, and the new rows join the unclaimed ones.
    pub(crate) fn grow(&mut self, edge: u16) {
        debug_assert!(edge > self.edge, "a grow widens the side");
        let old = self.edge;
        for shelf in &mut self.shelves {
            match shelf.free.last_mut() {
                Some(last) if last.end == old => last.end = edge,
                _ => shelf.free.push(old..edge),
            }
            let last = shelf.free.last().expect("a span pushed or widened");
            shelf.widest = shelf.widest.max(width_of(last));
        }
        self.edge = edge;
    }
}

impl Shelf {
    /// Whether a rectangle of height `h` belongs on the shelf, as [`takes`] its height.
    pub(crate) const fn takes(&self, h: u16) -> bool {
        takes(self.height, h)
    }
}

/// Whether a rectangle of height `h` belongs on a shelf of `height`: no lower, and within a quarter
/// of its own height above it (four texels at least), so short rectangles do not strand the room
/// of a tall shelf.
const fn takes(height: u16, h: u16) -> bool {
    h <= height && height - h <= max_slack(h)
}

/// The texels `span` holds.
const fn width_of(span: &Range<u16>) -> u16 {
    span.end - span.start
}

/// The room a shelf may leave above a rectangle of height `h`.
const fn max_slack(h: u16) -> u16 {
    let quarter = h / 4;
    if quarter > 4 { quarter } else { 4 }
}

/// The height a new shelf for a rectangle of height `h` opens at: rounded up to four texels, so
/// rectangles a few texels apart share it, and held to the packer's largest edge.
const fn shelf_height(h: u16) -> u16 {
    match h.checked_next_multiple_of(4) {
        Some(height) => height,
        None => u16::MAX,
    }
}

#[cfg(test)]
mod tests;
