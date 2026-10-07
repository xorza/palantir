//! Whether a node paints, and whether it still takes space.

/// WPF-style three-state visibility.
///
/// Encoder and input early-return at a non-`Visible` node, so descendants are never visited whatever their own `Visibility`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Visibility {
    /// Laid out, painted, hit-tested. The default.
    #[default]
    Visible = 0,
    /// Laid out, so it occupies space, but neither painted nor hit-tested.
    Hidden = 1,
    /// Treated as absent: zero size, skipped by stack and grid parents (no gap, no fill weight).
    Collapsed = 2,
}

impl Visibility {
    /// Whether this node paints.
    pub const fn is_visible(self) -> bool {
        matches!(self, Visibility::Visible)
    }
    /// Whether this node is skipped by layout entirely.
    pub const fn is_collapsed(self) -> bool {
        matches!(self, Visibility::Collapsed)
    }
}

#[cfg(test)]
mod tests;
