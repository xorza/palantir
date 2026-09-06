//! Declaring a bit-flag set: the macro, and the operations one gets.

/// Declare a `u8` flag set with exactly the operations this crate asks of
/// one, and no more.
///
/// `bitflags` published a wider type than the crate wanted: `from_bits_retain`
/// mints values with bits no arm handles, `iter` hands back a
/// `bitflags::iter::Iter` a caller cannot name, and the `Flags` impl put that
/// crate's `Internal` / `Bits` / `Primitive` into palantir's own surface — so
/// a major bump there was a breaking change here. Nothing in the tree used any
/// of it. What the sets do use is below, and every value of one is a union of
/// its declared bits by construction.
///
/// The one crate-wide macro, and the exception to the per-subtree rule the
/// others follow: a flag set is a shape rather than a fact about a subsystem,
/// and the sets that exist today sit under `input` only because that is where
/// the vocabulary they describe happens to live. `#[macro_use]` on this module
/// in `lib.rs` is what carries it, which is why that declaration comes first —
/// textual scoping reaches only the modules declared after it.
macro_rules! flag_set {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$flag_meta:meta])*
                const $flag:ident = $bit:expr;
            )+
        }
    ) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
        $vis struct $name(u8);

        impl $name {
            $(
                $(#[$flag_meta])*
                $vis const $flag: Self = Self($bit);
            )+

            /// No flags set.
            #[inline]
            $vis const fn empty() -> Self {
                Self(0)
            }

            /// Every declared flag.
            #[inline]
            $vis const fn all() -> Self {
                Self(0 $(| Self::$flag.0)+)
            }

            /// The raw bits, for packing into a wider word.
            #[inline]
            $vis const fn bits(self) -> u8 {
                self.0
            }

            /// Rebuild from packed bits, dropping any that name no flag.
            ///
            /// Truncating rather than retaining: a set is the union of its
            /// declared bits, and a word unpacked from a node's flag field
            /// carries neighbouring fields the mask may not have cleared.
            #[inline]
            $vis const fn from_bits_truncate(bits: u8) -> Self {
                Self(bits & Self::all().0)
            }

            /// True while no flag is set.
            #[inline]
            $vis const fn is_empty(self) -> bool {
                self.0 == 0
            }

            /// Every flag in `other` is set here.
            #[inline]
            $vis const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            /// At least one flag in `other` is set here.
            #[inline]
            $vis const fn intersects(self, other: Self) -> bool {
                self.0 & other.0 != 0
            }

            /// The flags in either set.
            #[inline]
            $vis const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }

            /// The flags here that `other` does not have.
            #[inline]
            $vis const fn difference(self, other: Self) -> Self {
                Self(self.0 & !other.0)
            }

            /// Add every flag in `other`.
            #[inline]
            $vis fn insert(&mut self, other: Self) {
                self.0 |= other.0;
            }

            /// Drop every flag in `other`.
            #[inline]
            $vis fn remove(&mut self, other: Self) {
                self.0 &= !other.0;
            }

            /// Add or drop every flag in `other`, per `on`.
            #[inline]
            $vis fn set(&mut self, other: Self, on: bool) {
                if on {
                    self.insert(other);
                } else {
                    self.remove(other);
                }
            }
        }

        impl std::ops::BitOr for $name {
            type Output = Self;
            #[inline]
            fn bitor(self, other: Self) -> Self {
                self.union(other)
            }
        }

        impl std::fmt::Debug for $name {
            /// Names, not the packed byte: `Sense(CLICK | DRAG)` is what a
            /// failing assertion has to read out.
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "("))?;
                let mut first = true;
                $(
                    if self.contains(Self::$flag) {
                        if !first {
                            f.write_str(" | ")?;
                        }
                        f.write_str(stringify!($flag))?;
                        first = false;
                    }
                )+
                if first {
                    f.write_str("empty")?;
                }
                f.write_str(")")
            }
        }
    };
}

#[cfg(test)]
mod tests {
    flag_set! {
        /// Bit 2 is skipped on purpose: the truncation test needs a gap
        /// inside the declared range as well as the spare bits above it.
        struct Fixture {
            const A = 1 << 0;
            const B = 1 << 1;
            const C = 1 << 3;
        }
    }

    /// Hand-written bit logic, so the set algebra is pinned against values
    /// worked out by hand rather than trusted to read right.
    #[test]
    fn set_algebra_matches_the_bits_it_claims() {
        let ab = Fixture::A.union(Fixture::B);
        assert_eq!(ab.bits(), 0b0011);

        // `contains` is "all of", `intersects` is "any of" — the pair that
        // reads alike and must not.
        assert!(ab.contains(Fixture::A));
        assert!(ab.contains(ab));
        assert!(!ab.contains(Fixture::A.union(Fixture::C)));
        assert!(ab.intersects(Fixture::A.union(Fixture::C)));
        assert!(!ab.intersects(Fixture::C));

        // The empty set is contained by everything and intersects nothing.
        assert!(ab.contains(Fixture::empty()));
        assert!(!ab.intersects(Fixture::empty()));
        assert!(Fixture::empty().is_empty());
        assert!(!ab.is_empty());

        assert_eq!(ab.difference(Fixture::A), Fixture::B);
        assert_eq!(ab.difference(Fixture::C), ab);
        assert_eq!(Fixture::A | Fixture::B, ab);

        assert_eq!(Fixture::all().bits(), 0b1011);
        assert_eq!(Fixture::empty().bits(), 0);
    }

    /// The property the hand-written set buys over a generated one: no value
    /// can hold a bit that names no flag, so every arm matching on a set is
    /// exhaustive by construction.
    #[test]
    fn undeclared_bits_cannot_survive_a_round_trip() {
        // `Fixture` leaves bits 2 and 4..8 unnamed.
        assert_eq!(Fixture::from_bits_truncate(0b1111_0100), Fixture::empty());
        assert_eq!(
            Fixture::from_bits_truncate(0b1111_0110),
            Fixture::B,
            "the undeclared bits go, the declared one stays",
        );
        assert_eq!(Fixture::from_bits_truncate(u8::MAX), Fixture::all());
    }

    #[test]
    fn insert_remove_and_set_move_only_the_named_flags() {
        let mut f = Fixture::A;
        f.insert(Fixture::C);
        assert_eq!(f, Fixture::A.union(Fixture::C));
        f.remove(Fixture::A);
        assert_eq!(f, Fixture::C);
        f.remove(Fixture::A);
        assert_eq!(f, Fixture::C, "removing an absent flag changes nothing");

        f.set(Fixture::B, true);
        assert_eq!(f, Fixture::C.union(Fixture::B));
        f.set(Fixture::B, false);
        assert_eq!(f, Fixture::C);
    }

    /// Debug prints names, because that is what a failing assertion on a
    /// mask has to read out.
    #[test]
    fn debug_names_the_flags_that_are_set() {
        assert_eq!(format!("{:?}", Fixture::empty()), "Fixture(empty)");
        assert_eq!(format!("{:?}", Fixture::A), "Fixture(A)");
        assert_eq!(
            format!("{:?}", Fixture::A.union(Fixture::C)),
            "Fixture(A | C)",
        );
    }
}
