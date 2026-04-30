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
}
