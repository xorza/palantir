//! [`Tenant`]: who holds an atlas item, so an eviction reaches its owner.

/// What allocates from the pool. An eviction takes an item of any tenant of its list, so the pool
/// queues each evicted handle for its tenant, which drops the key it held it under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum Tenant {
    Glyphs = 0,
    Icons = 1,
    CutoutTables = 2,
    Backdrops = 3,
}

impl Tenant {
    /// How many tenants there are, the length of the pool's eviction queues.
    pub(crate) const COUNT: usize = 4;
}
