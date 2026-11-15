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
    /// Every tenant, in discriminant order, which indexes the pool's eviction queues.
    pub(crate) const ALL: [Self; 4] = [
        Self::Glyphs,
        Self::Icons,
        Self::CutoutTables,
        Self::Backdrops,
    ];

    /// How many tenants there are.
    pub(crate) const COUNT: usize = Self::ALL.len();
}

/// Who holds an item of the pool: its tenant, and the tag the tenant names its own entry for the
/// item by, which comes back with the item's eviction
/// ([`Evicted`](crate::gpu::atlas::atlas_handle::Evicted)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ItemOwner {
    pub(crate) tenant: Tenant,
    pub(crate) tag: u32,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::atlas::tenant::{ItemOwner, Tenant};

    impl Tenant {
        /// The owner of an item this tenant files under tag 0, as a test that names no entry of
        /// its own does.
        pub(crate) const fn owner(self) -> ItemOwner {
            ItemOwner {
                tenant: self,
                tag: 0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::atlas::tenant::Tenant;

    /// `ALL` lists each tenant at its discriminant, which indexes the eviction queues.
    #[test]
    fn every_tenant_sits_at_its_discriminant() {
        for (at, tenant) in Tenant::ALL.into_iter().enumerate() {
            assert_eq!(tenant as usize, at);
        }
    }
}
