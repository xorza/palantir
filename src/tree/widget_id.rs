use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WidgetId(pub u64);

impl WidgetId {
    pub fn from_hash(h: impl Hash) -> Self {
        let mut hasher = FxHasher::default();
        h.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Derive a child id by mixing `h` into this id. Useful for nested widgets
    /// where the parent already has a stable id.
    pub fn with(self, h: impl Hash) -> Self {
        let mut hasher = FxHasher::default();
        self.0.hash(&mut hasher);
        h.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Stable across frames as long as the call site is unchanged.
    ///
    /// **Footgun: collides on repeated calls from the same source location.**
    /// All widgets recorded inside a `for` / `while` / `iter::map` loop will
    /// share one id, which corrupts persistent per-widget state (focus,
    /// scroll, animation) and breaks click capture. Use `with_id(key)` (or
    /// `WidgetId::from_hash` / `WidgetId::with`) whenever the call site can
    /// fire more than once per frame, where `key` is something stable like
    /// the item's index or a domain key. `Ui::node` warns at debug time when
    /// it sees the same id twice in one frame.
    #[track_caller]
    pub fn auto_stable() -> Self {
        let l = std::panic::Location::caller();
        Self::from_hash((l.file(), l.line(), l.column()))
    }
}
