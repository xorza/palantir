//! A chip a drag moved to another slot.

use crate::widgets::tabs::reindex;

/// A chip a drag moved: move `from` to `to` in the caller's collection. `to` addresses it
/// **before** the move (`len` appends), so the item lands at `to - 1` when `to > from`;
/// [`Self::apply`] does the move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabMove {
    /// Where the chip came from.
    pub from: usize,
    /// Where it lands, addressing the collection before the move.
    pub to: usize,
}

impl TabMove {
    /// The slot the moved item sits in after the move: `to`, or one short of it for a move right.
    pub const fn landing(self) -> usize {
        reindex::landing(self.from, self.to)
    }

    /// Move the item at `from` in `items` to [`Self::landing`], the items between closing up
    /// behind it: one rotate of the span between the two slots.
    ///
    /// # Panics
    ///
    /// Panics unless `from` is a slot of `items` and `to` at most its length.
    pub fn apply<T>(self, items: &mut [T]) {
        let landing = self.landing();
        if landing > self.from {
            items[self.from..=landing].rotate_left(1);
        } else {
            items[landing..=self.from].rotate_right(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::widgets::tabs::tab_move::TabMove;

    /// A move right lands one short of its gap, a move left at it, and a move into either gap
    /// beside the chip leaves the order.
    #[test]
    fn apply_rotates_the_span_between_the_slots() {
        for (from, to, want) in [
            (0, 3, ['b', 'c', 'a', 'd']),
            (0, 4, ['b', 'c', 'd', 'a']),
            (3, 0, ['d', 'a', 'b', 'c']),
            (2, 1, ['a', 'c', 'b', 'd']),
            (1, 1, ['a', 'b', 'c', 'd']),
            (1, 2, ['a', 'b', 'c', 'd']),
        ] {
            let mut items = ['a', 'b', 'c', 'd'];
            let moved = TabMove { from, to };
            let item = items[from];
            moved.apply(&mut items);
            assert_eq!(items, want, "{from} → {to}");
            assert_eq!(
                items[moved.landing()],
                item,
                "{from} → {to} lands where it says"
            );
        }
    }
}
