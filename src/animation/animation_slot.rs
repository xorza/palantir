//! The per-widget tag that lets one widget animate several things at once.

use std::hash;
/// Slot tag for stacking multiple animations on one widget. Widgets
/// declare their own slot consts (e.g. `const HOVER: AnimationSlot =
/// AnimationSlot::new("hover"); const PRESS: AnimationSlot =
/// AnimationSlot::new("press");`). Cross-widget slot identity is
/// meaningless — `AnimationSlot::new("hover")` on widget A is unrelated to
/// the same slot on widget B (the hash key is `(WidgetId, AnimationSlot)`).
///
/// Named by `&'static str` so the slot reads as a name at the call
/// site instead of a magic number, but *stored* as the name's FNV-1a
/// hash, computed once in the `const` constructor. Per-frame map ops
/// hash a single precomputed `u64` instead of re-walking the string
/// bytes, and the release slot is 8 bytes so the `(WidgetId,
/// AnimationSlot)` row key stays lean. Identity is a pure function of the
/// name bytes (same literal from any call site compares equal
/// regardless of interning). FNV-64 is deterministic, so two distinct
/// names colliding would alias on every run — debug builds keep the
/// name and assert on exactly that at the aliasing map probe.
#[derive(Clone, Copy, Debug)]
pub struct AnimationSlot {
    hash: u64,
    #[cfg(debug_assertions)]
    name: &'static str,
}

impl AnimationSlot {
    /// A slot named `name`, hashed at compile time.
    ///
    /// One widget animates several values by giving each its own slot. Two
    /// slots on one widget must not share a name — debug builds assert on
    /// exactly that.
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

    /// Pins the `AnimationSlot` identity contract: the cached hash is FNV-1a
    /// of the name bytes (checked against the published 64-bit test
    /// vectors), both construction routes agree, equality is by contents,
    /// and distinct names make distinct slots.
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
