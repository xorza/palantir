use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct WidgetId(pub u64);

impl WidgetId {
    pub fn from_hash(h: impl Hash) -> Self {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        h.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Derive a child id by mixing `h` into this id. Useful for nested widgets
    /// where the parent already has a stable id.
    pub fn with(self, h: impl Hash) -> Self {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        h.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Stable across frames as long as the call site is unchanged.
    /// Collides for widgets created at the same call site (e.g. inside a `for` loop) —
    /// in that case build an id explicitly with `from_hash`.
    #[track_caller]
    pub fn auto_stable() -> Self {
        let l = std::panic::Location::caller();
        Self::from_hash((l.file(), l.line(), l.column()))
    }
}
