//! The rectangle packer of one square atlas texture: shelves of one height class each, their free
//! spans kept exact, so a freed rectangle is room again at once, and a grow keeps every placement.

use glam::U16Vec2;
use std::ops::Range;

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
    size: u16,
    /// Sorted by `y`; together they cover `0..top`.
    shelves: Vec<Shelf>,
    /// The first row no shelf holds: everything below is unclaimed.
    top: u16,
    /// The span lists of shelves that went, cleared and kept for the next shelf to open, so a
    /// shelf allocates nothing once the packer has warmed up.
    spare: Vec<Vec<Range<u16>>>,
}

/// Where a shelf starts and how tall it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShelfRow {
    pub(crate) y: u16,
    pub(crate) height: u16,
}

#[derive(Debug)]
struct Shelf {
    y: u16,
    height: u16,
    /// Free spans of the shelf's row, sorted, disjoint and never touching: a free next to a free
    /// span merges with it.
    free: Vec<Range<u16>>,
}

impl ShelfPacker {
    pub(crate) const fn new(size: u16) -> Self {
        Self {
            size,
            shelves: Vec::new(),
            top: 0,
            spare: Vec::new(),
        }
    }

    /// Whether a rectangle of height `h` belongs on a shelf of `height`: no lower, and within a
    /// quarter of its own height above it (four texels at least), so short rectangles do not
    /// strand the room of a tall shelf.
    pub(crate) const fn takes(height: u16, h: u16) -> bool {
        h <= height && height - h <= max_slack(h)
    }

    /// The top-left of a free rectangle of `size`, now taken; `None` when no shelf and no
    /// unclaimed row has room. The lowest shelf that takes the rectangle and has a span wide
    /// enough wins, then a new shelf, then the shortest empty shelf that holds it, split to its
    /// height.
    pub(crate) fn allocate(&mut self, size: U16Vec2) -> Option<U16Vec2> {
        let (w, h) = (size.x, size.y);
        debug_assert!(w > 0 && h > 0, "a rectangle with no texels owns no room");
        if w > self.size || h > self.size {
            return None;
        }
        let fitting = |shelf: &Shelf| {
            shelf
                .free
                .iter()
                .position(|span| span.end - span.start >= w)
        };
        let best = self
            .shelves
            .iter()
            .enumerate()
            .filter(|(_, shelf)| Self::takes(shelf.height, h))
            .filter_map(|(i, shelf)| fitting(shelf).map(|span| (shelf.height, i, span)))
            .min();
        if let Some((_, shelf, span)) = best {
            return Some(self.carve(shelf, span, w));
        }
        let height = shelf_height(h).min(self.size - self.top);
        if h <= height {
            let y = self.top;
            self.top += height;
            let shelf = self.open(y, height);
            self.shelves.push(shelf);
            return Some(self.carve(self.shelves.len() - 1, 0, w));
        }
        let size = self.size;
        let (_, empty) = self
            .shelves
            .iter()
            .enumerate()
            .filter(|(_, shelf)| h <= shelf.height && shelf.is_empty(size))
            .map(|(i, shelf)| (shelf.height, i))
            .min()?;
        self.split(empty, shelf_height(h));
        Some(self.carve(empty, 0, w))
    }

    /// A shelf at row `y` of `height`, its whole row free, on a span list kept from a shelf
    /// that went.
    fn open(&mut self, y: u16, height: u16) -> Shelf {
        let mut free = self.spare.pop().unwrap_or_default();
        free.push(0..self.size);
        Shelf { y, height, free }
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
        let ShelfRow { y, height: whole } = self.row(i);
        if whole <= height {
            return;
        }
        let rest = self.open(y + height, whole - height);
        self.shelves[i].height = height;
        self.shelves.insert(i + 1, rest);
    }

    /// Take `w` texels off the front of free span `span` of shelf `shelf`.
    fn carve(&mut self, shelf: usize, span: usize, w: u16) -> U16Vec2 {
        let shelf = &mut self.shelves[shelf];
        let free = &mut shelf.free[span];
        let x = free.start;
        free.start += w;
        if free.start == free.end {
            shelf.free.remove(span);
        }
        U16Vec2::new(x, shelf.y)
    }

    /// Return the rectangle of `width` at `origin`, as [`Self::allocate`] placed it. A shelf it
    /// leaves empty merges with the empty shelves beside it, and empty shelves at the bottom go
    /// back to the unclaimed rows.
    pub(crate) fn deallocate(&mut self, origin: U16Vec2, width: u16) {
        let shelf = self.shelf_at(origin.y);
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
        match (joins_prev, joins_next) {
            (true, true) => {
                free[at - 1].end = free[at].end;
                free.remove(at);
            }
            (true, false) => free[at - 1].end = freed.end,
            (false, true) => free[at].start = freed.start,
            (false, false) => free.insert(at, freed),
        }
        if self.shelves[shelf].is_empty(self.size) {
            self.merge_empty(shelf);
        }
        let size = self.size;
        while self
            .shelves
            .last()
            .is_some_and(|shelf| shelf.is_empty(size))
        {
            let shelf = self.shelves.pop().expect("a last shelf");
            self.top = shelf.y;
            self.retire(shelf);
        }
    }

    /// Merge empty shelf `i` with the empty shelves just above and below it.
    fn merge_empty(&mut self, i: usize) {
        let size = self.size;
        let mut i = i;
        if i > 0 && self.shelves[i - 1].is_empty(size) {
            let shelf = self.shelves.remove(i);
            self.shelves[i - 1].height += shelf.height;
            self.retire(shelf);
            i -= 1;
        }
        if self
            .shelves
            .get(i + 1)
            .is_some_and(|next| next.is_empty(size))
        {
            let next = self.shelves.remove(i + 1);
            self.shelves[i].height += next.height;
            self.retire(next);
        }
    }

    /// The height of the shelf a rectangle at row `y` sits on.
    pub(crate) fn shelf_height(&self, y: u16) -> u16 {
        self.shelves[self.shelf_at(y)].height
    }

    /// How many shelves there are, numbered top to bottom.
    pub(crate) const fn shelf_count(&self) -> usize {
        self.shelves.len()
    }

    /// Shelf `i`'s row.
    pub(crate) fn row(&self, i: usize) -> ShelfRow {
        let shelf = &self.shelves[i];
        ShelfRow {
            y: shelf.y,
            height: shelf.height,
        }
    }

    /// The number of the shelf a rectangle at row `y` sits on.
    pub(crate) fn shelf_at(&self, y: u16) -> usize {
        let at = self.shelves.partition_point(|shelf| shelf.y <= y);
        debug_assert!(at > 0, "row {y} sits on a shelf");
        at - 1
    }

    /// Widen and deepen the side to `size`, keeping every placement: each shelf gains the new
    /// columns, and the new rows join the unclaimed ones.
    pub(crate) fn grow(&mut self, size: u16) {
        debug_assert!(size >= self.size, "a side only grows");
        let old = self.size;
        for shelf in &mut self.shelves {
            match shelf.free.last_mut() {
                Some(last) if last.end == old => last.end = size,
                _ => shelf.free.push(old..size),
            }
        }
        self.size = size;
    }
}

impl Shelf {
    /// Whether nothing sits on the shelf: its one free span is the whole `size`-wide row.
    const fn is_empty(&self, size: u16) -> bool {
        matches!(self.free.as_slice(), [span] if span.start == 0 && span.end == size)
    }
}

/// The room a shelf may leave above a rectangle of height `h`.
const fn max_slack(h: u16) -> u16 {
    let quarter = h / 4;
    if quarter > 4 { quarter } else { 4 }
}

/// The height a new shelf for a rectangle of height `h` opens at: rounded up to four texels, so
/// rectangles a few texels apart share it.
const fn shelf_height(h: u16) -> u16 {
    h.next_multiple_of(4)
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::shelf_packer::ShelfPacker;
    use glam::U16Vec2;

    fn size(w: u16, h: u16) -> U16Vec2 {
        U16Vec2::new(w, h)
    }

    /// Rasters fill a shelf left to right and open the next one under it, rounded up to four rows;
    /// a raster too tall for a shelf's slack opens its own; one past the edge never fits.
    #[test]
    fn shelves_fill_left_to_right_and_stack() {
        let mut packer = ShelfPacker::new(64);
        assert_eq!(packer.allocate(size(30, 10)), Some(U16Vec2::new(0, 0)));
        assert_eq!(packer.allocate(size(30, 9)), Some(U16Vec2::new(30, 0)));
        assert_eq!(packer.shelf_height(0), 12, "10 rounds up to 12");
        assert_eq!(
            packer.allocate(size(10, 12)),
            Some(U16Vec2::new(0, 12)),
            "4 columns are left on the first shelf, too few for 10: a second opens"
        );
        // 2 rows above a 10 is within its slack of 4; 12 above a 20 is not.
        assert!(ShelfPacker::takes(12, 10));
        assert!(!ShelfPacker::takes(32, 20));
        assert_eq!(packer.allocate(size(10, 30)), Some(U16Vec2::new(0, 24)));
        assert_eq!(packer.allocate(size(65, 1)), None, "wider than the side");
        assert_eq!(
            packer.allocate(size(10, 16)),
            None,
            "no row left for a new shelf"
        );
    }

    /// A freed rectangle is room again at once, merged with the free spans beside it; the last
    /// shelf emptied gives its rows back, so another height can open there.
    #[test]
    fn a_freed_rectangle_is_room_at_once() {
        let mut packer = ShelfPacker::new(64);
        let a = packer.allocate(size(20, 8)).unwrap();
        let b = packer.allocate(size(20, 8)).unwrap();
        assert_eq!(packer.allocate(size(20, 8)), Some(U16Vec2::new(40, 0)));
        assert_eq!(
            packer.allocate(size(40, 8)),
            Some(U16Vec2::new(0, 8)),
            "4 columns are left on the first shelf"
        );
        packer.deallocate(b, 20);
        assert_eq!(
            packer.allocate(size(24, 8)),
            Some(U16Vec2::new(40, 8)),
            "b's 20 columns are too few for 24; the second shelf's 24 are not"
        );
        packer.deallocate(a, 20);
        assert_eq!(
            packer.allocate(size(40, 8)),
            Some(U16Vec2::new(0, 0)),
            "a and b merged into one 40-wide span"
        );
        packer.deallocate(U16Vec2::new(0, 8), 40);
        packer.deallocate(U16Vec2::new(40, 8), 24);
        assert_eq!(
            packer.allocate(size(10, 30)),
            Some(U16Vec2::new(0, 8)),
            "the emptied last shelf gave its rows back"
        );
    }

    /// A grow keeps every placement and widens every shelf.
    #[test]
    fn a_grow_widens_each_shelf_and_keeps_placements() {
        let mut packer = ShelfPacker::new(32);
        assert_eq!(packer.allocate(size(32, 8)), Some(U16Vec2::new(0, 0)));
        assert_eq!(packer.allocate(size(8, 8)), Some(U16Vec2::new(0, 8)));
        packer.grow(64);
        assert_eq!(packer.allocate(size(32, 8)), Some(U16Vec2::new(32, 0)));
        assert_eq!(packer.allocate(size(56, 8)), Some(U16Vec2::new(8, 8)));
        packer.deallocate(U16Vec2::new(0, 0), 32);
        packer.deallocate(U16Vec2::new(32, 0), 32);
        assert_eq!(
            packer.allocate(size(64, 6)),
            Some(U16Vec2::new(0, 0)),
            "the emptied first shelf takes a raster within its slack"
        );
    }

    /// Two shelves emptied side by side merge into one, which a rectangle taller than either takes
    /// once no unclaimed row is left, split to its height: the rows past it stay an empty shelf.
    /// Without the merge the 16-row rectangle finds no room; the 24 merged rows are past its slack,
    /// so it takes them by the split. A span list of a shelf that went is
    /// kept and taken by the next shelf to open.
    #[test]
    fn emptied_shelves_merge_and_split_to_a_taller_rectangle() {
        let mut packer = ShelfPacker::new(64);
        let a = packer.allocate(size(64, 12)).unwrap();
        let b = packer.allocate(size(64, 12)).unwrap();
        assert_eq!(packer.allocate(size(64, 40)), Some(U16Vec2::new(0, 24)));
        assert_eq!(packer.allocate(size(8, 16)), None, "no row left");
        packer.deallocate(a, 64);
        packer.deallocate(b, 64);
        assert_eq!(packer.shelf_count(), 2, "the two 12-row shelves merged");
        assert_eq!(packer.row(0).height, 24);
        assert_eq!(packer.allocate(size(8, 16)), Some(U16Vec2::new(0, 0)));
        assert_eq!(
            (packer.row(0).height, packer.row(1).height),
            (16, 8),
            "split to the rectangle's 16 rows, 8 left empty"
        );
        packer.deallocate(U16Vec2::new(0, 24), 64);
        assert_eq!(
            packer.spare.len(),
            2,
            "the 40-row shelf merged with the 8 empty rows above it, then went back to the unclaimed rows"
        );
        assert_eq!(packer.allocate(size(10, 30)), Some(U16Vec2::new(0, 16)));
        assert_eq!(packer.spare.len(), 1, "the new shelf took a kept span list");
    }

    /// With no unclaimed row, a rectangle takes the shortest empty shelf that holds it, not the
    /// first.
    #[test]
    fn a_rectangle_takes_the_shortest_empty_shelf_that_holds_it() {
        let mut packer = ShelfPacker::new(64);
        let tall = packer.allocate(size(64, 32)).unwrap();
        packer.allocate(size(64, 8)).unwrap();
        let short = packer.allocate(size(64, 20)).unwrap();
        packer.allocate(size(64, 4)).unwrap();
        packer.deallocate(tall, 64);
        packer.deallocate(short, 64);
        assert_eq!(
            packer.allocate(size(8, 18)),
            Some(short),
            "the 20-row shelf"
        );
        assert_eq!(packer.row(0).height, 32, "the 32-row one kept whole");
    }
}
