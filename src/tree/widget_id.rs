use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WidgetId(pub(crate) u64);

impl WidgetId {
    pub fn from_hash(h: impl Hash) -> Self {
        let mut hasher = FxHasher::default();
        h.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Derive a child id by mixing `h` into this id. Useful for nested widgets
    /// where the parent already has a stable id.
    pub(crate) fn with(self, h: impl Hash) -> Self {
        let mut hasher = FxHasher::default();
        self.0.hash(&mut hasher);
        h.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Stable across frames as long as the call site is unchanged. Const so
    /// the compiler folds the FNV-1a hash to a `u64` literal at every
    /// `Foo::new()` call site — widget construction is a single `mov` rather
    /// than a hasher run.
    ///
    /// Hashes via FNV-1a (not the FxHasher used by [`Self::from_hash`]). The
    /// two hash spaces never alias in practice — auto ids derive from
    /// `(file, line, column)` triples that no sensible explicit key would
    /// match — so we accept the small smell of two functions for the
    /// const-fold win on the hot path.
    ///
    /// Repeated calls from the same source location (a loop or a closure
    /// helper) all produce the same id; `Ui::node` silently disambiguates by
    /// mixing in a per-id occurrence counter. Override with
    /// [`crate::tree::element::Configure::id_salt`] when call order isn't
    /// stable across frames.
    #[track_caller]
    pub(crate) const fn auto_stable() -> Self {
        let l = std::panic::Location::caller();
        let mut h: u64 = FNV_OFFSET;
        h = fnv1a_extend_str(h, l.file());
        h = fnv1a_extend_u32(h, l.line());
        h = fnv1a_extend_u32(h, l.column());
        Self(h)
    }
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

const fn fnv1a_extend_str(mut h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

const fn fnv1a_extend_u32(mut h: u64, v: u32) -> u64 {
    let bytes = v.to_le_bytes();
    let mut i = 0;
    while i < 4 {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}
