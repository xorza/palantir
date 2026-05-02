/// WPF-style three-state visibility.
///
/// - `Visible` — laid out, painted, hit-tested.
/// - `Hidden` — laid out (occupies space), but neither painted nor hit-tested.
/// - `Collapsed` — treated as if absent: zero size, skipped by stack/grid
///   parents (no gap contribution, no fill weight), not painted, not hit-tested.
///
/// Cascade is implicit: encoder/input early-return at a non-`Visible` node, so
/// descendants are never visited regardless of their own `Visibility`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapsed,
}

impl Visibility {
    pub const fn is_visible(self) -> bool {
        matches!(self, Visibility::Visible)
    }
    pub const fn is_hidden(self) -> bool {
        matches!(self, Visibility::Hidden)
    }
    pub const fn is_collapsed(self) -> bool {
        matches!(self, Visibility::Collapsed)
    }
}
