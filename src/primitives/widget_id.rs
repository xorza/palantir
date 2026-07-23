use rustc_hash::FxHasher;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::hash::{Hash, Hasher};
use std::panic::Location;

#[derive(Debug, Default)]
pub(crate) struct IdHasher(u64);

impl Hasher for IdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }

    fn write(&mut self, _bytes: &[u8]) {
        unreachable!("IdHasher only sees write_u64 from WidgetId's derived Hash impl");
    }
}

pub(crate) type WidgetIdMap<V> = HashMap<WidgetId, V, BuildHasherDefault<IdHasher>>;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WidgetId(pub(crate) u64);

impl WidgetId {
    /// Stable id for the `Layer::Main` synthetic viewport root.
    /// Hard-coded (rather than derived from `auto_stable()` at the
    /// viewport construction site) so refactors to `ui/mod.rs` don't
    /// shift it. Treated like any other parent by
    /// `Ui::widget` — top-level `id_salt("k")` resolves to
    /// `VIEWPORT.with(from_hash("k").0)`.
    pub(crate) const VIEWPORT: Self = Self(u64::MAX);

    pub fn from_hash(h: impl Hash) -> Self {
        let mut hasher = FxHasher::default();
        h.hash(&mut hasher);
        Self::nonzero(hasher.finish())
    }

    /// Derive a child id by mixing `h` into this id. Useful for nested widgets
    /// where the parent already has a stable id — widget authors use this to
    /// key the child nodes they open inside their `show` body.
    pub fn with(self, h: impl Hash) -> Self {
        let mut hasher = FxHasher::default();
        self.0.hash(&mut hasher);
        h.hash(&mut hasher);
        Self::nonzero(hasher.finish())
    }

    /// Avoid the all-zero hash that would collide with [`Self::default`]
    /// (used as the "unset" sentinel by [`crate::scene::node::Node::new`]).
    /// FxHasher returns 0 for all-zero input (e.g. `(0u16, 0u16)`); a plain
    /// rotate-and-bias replaces it with `1` without skewing other outputs.
    const fn nonzero(h: u64) -> Self {
        Self(if h == 0 { 1 } else { h })
    }

    /// Stable across frames as long as the call site is unchanged.
    ///
    /// Hashes the caller's `(file, line, column)` through `FxHasher`.
    /// `Location::caller()` resolves at *runtime*, so this runs on every
    /// widget constructor call — with a byte-serial FNV-1a over the file
    /// path it was the single largest record-pass cost in the frame
    /// profile (~90% of `Button::new` self-time); FxHasher walks the
    /// path a word at a time. The [`Self::from_hash`] space can't alias
    /// this one: `str`'s `Hash` impl appends a `0xff` terminator that the
    /// raw byte-slice write here never produces.
    ///
    /// Repeated calls from the same source location (a loop or a closure
    /// helper) all produce the same id; `Ui::node` silently disambiguates by
    /// mixing in a per-id occurrence counter. Override with
    /// [`crate::scene::node::Configure::id_salt`] when call order isn't
    /// stable across frames.
    #[track_caller]
    pub fn auto_stable() -> Self {
        let l = Location::caller();
        let mut hasher = FxHasher::default();
        hasher.write(l.file().as_bytes());
        hasher.write_u32(l.line());
        hasher.write_u32(l.column());
        Self::nonzero(hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::widget_id::WidgetId;
    use rustc_hash::FxHasher;
    use std::hash::Hasher;

    /// Both calls resolve to the *same* caller location, letting the test
    /// below rebuild the exact hash `auto_stable` must produce.
    #[track_caller]
    fn id_and_loc() -> (WidgetId, &'static std::panic::Location<'static>) {
        (WidgetId::auto_stable(), std::panic::Location::caller())
    }

    #[test]
    fn auto_stable_hashes_location_via_fx() {
        let (id, l) = id_and_loc();
        let mut hasher = FxHasher::default();
        hasher.write(l.file().as_bytes());
        hasher.write_u32(l.line());
        hasher.write_u32(l.column());
        let h = hasher.finish();
        assert_eq!(id, WidgetId(if h == 0 { 1 } else { h }));

        // Same call site (loop) → identical ids; a different call line →
        // a different id.
        let repeated: Vec<WidgetId> = (0..2).map(|_| id_and_loc().0).collect();
        assert_eq!(repeated[0], repeated[1]);
        assert_ne!(repeated[0], id);
    }
}
