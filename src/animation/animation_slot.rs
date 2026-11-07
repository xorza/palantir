//! The per-widget tag that lets one widget animate several things at once.

use std::hash;
/// Slot tag for stacking animations on one widget; widgets declare their own consts (`AnimationSlot::new("hover")`). Identity is per widget: the row key is `(WidgetId, AnimationSlot)`.
///
/// Hashed in the `const` constructor by FNV-1a of its name, so a map op hashes one `u64`. Equality
/// compares the names, so two names that share a hash stay two slots in every build. Refused: the
/// hash alone, 8 bytes, as FNV-64 is deterministic and colliding names would alias on every run; an
/// assert on a collision, which panics on a name an app chose.
#[derive(Clone, Copy, Debug)]
pub struct AnimationSlot {
    hash: u64,
    name: &'static str,
}

impl AnimationSlot {
    /// A slot named `name`, hashed at compile time. One name on one widget is one slot.
    pub const fn new(name: &'static str) -> Self {
        let bytes = name.as_bytes();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
            i += 1;
        }
        Self { hash, name }
    }
}

impl PartialEq for AnimationSlot {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.name == other.name
    }
}

impl Eq for AnimationSlot {}

impl hash::Hash for AnimationSlot {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

impl From<&'static str> for AnimationSlot {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use crate::animation::animation_slot::AnimationSlot;

    /// Pins the identity contract: the hash is FNV-1a of the name bytes (published 64-bit test vectors), both construction routes agree, equality is by contents, distinct names give distinct slots.
    #[test]
    fn anim_slot_hash_is_const_fnv1a_of_name() {
        const A: AnimationSlot = AnimationSlot::new("a");
        assert_eq!(A.hash, 0xaf63_dc4c_8601_ec8c);
        assert_eq!(AnimationSlot::new("foobar").hash, 0x8594_4171_f739_67e8);

        let from: AnimationSlot = "a".into();
        assert_eq!(from, A, "From<&str> and const ctor must agree");
        assert_eq!(from.hash, A.hash);
        assert_ne!(AnimationSlot::new("a"), AnimationSlot::new("b"));

        let same_hash = AnimationSlot {
            hash: A.hash,
            name: "b",
        };
        assert_ne!(same_hash, A, "a shared hash leaves two names two slots");
    }
}
