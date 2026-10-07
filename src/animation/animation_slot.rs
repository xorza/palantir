//! The per-widget tag that lets one widget animate several things at once.

use std::hash;
/// Slot tag for stacking animations on one widget; widgets declare their own consts (`AnimationSlot::new("hover")`). Identity is per widget: the row key is `(WidgetId, AnimationSlot)`.
///
/// Named by `&'static str` but stored as its FNV-1a hash, computed in the `const` constructor, so map ops hash one `u64` and the slot is 8 bytes. Equal names compare equal from any call site. FNV-64 is deterministic, so colliding names would alias every run; debug builds keep the name and assert at the aliasing map probe.
#[derive(Clone, Copy, Debug)]
pub struct AnimationSlot {
    hash: u64,
    #[cfg(debug_assertions)]
    name: &'static str,
}

impl AnimationSlot {
    /// A slot named `name`, hashed at compile time. Two slots on one widget must not share a name; debug builds assert.
    pub const fn new(name: &'static str) -> Self {
        let bytes = name.as_bytes();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < bytes.len() {
            hash ^= bytes[i] as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
            i += 1;
        }
        Self {
            hash,
            #[cfg(debug_assertions)]
            name,
        }
    }
}

impl PartialEq for AnimationSlot {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        let eq = self.hash == other.hash;
        #[cfg(debug_assertions)]
        if eq {
            assert_eq!(
                self.name, other.name,
                "AnimationSlot FNV-64 hash collision — rename one of the slots"
            );
        }
        eq
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
    }
}
