//! Declaring a bit-flag set: the macro, and the operations one gets.

/// Declare a `u8` flag set with exactly the operations this crate asks of one.
///
/// Every set gets `NONE`, `ALL`, its named flags, the set operations and `|`. `struct Name: packed` also gets crate-private `bits` and `from_bits_truncate`; the bit layout is never public.
///
/// Replaces `bitflags`, whose public `from_bits_retain`, `iter` and `Flags` impl leaked its types into palantir's surface. Every value is a union of declared bits by construction.
///
/// The one crate-wide macro: `#[macro_use]` in `lib.rs` carries it, so its declaration must precede the modules that use it.
macro_rules! flag_set {
    (@packed $name:ident, packed) => {
        impl $name {
            #[inline]
            pub(crate) const fn bits(self) -> u8 {
                self.0
            }

            /// Rebuild from packed bits, dropping any that name no flag (a word unpacked from a node's flag field carries neighbouring fields).
            #[inline]
            pub(crate) const fn from_bits_truncate(bits: u8) -> Self {
                Self(bits & Self::ALL.0)
            }
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident $(: $packed:ident)? {
            $(
                $(#[$flag_meta:meta])*
                const $flag:ident = $bit:expr;
            )+
        }
    ) => {
        $(#[$meta])*
        #[must_use]
        #[repr(transparent)]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
        $vis struct $name(u8);

        impl $name {
            $(
                $(#[$flag_meta])*
                $vis const $flag: Self = Self($bit);
            )+

            /// No flags set. Also the `Default`.
            $vis const NONE: Self = Self(0);

            /// Every declared flag.
            $vis const ALL: Self = Self(0 $(| Self::$flag.0)+);

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
            $vis const fn insert(&mut self, other: Self) {
                self.0 |= other.0;
            }

            /// Drop every flag in `other`.
            #[inline]
            $vis const fn remove(&mut self, other: Self) {
                self.0 &= !other.0;
            }

            /// Add or drop every flag in `other`, per `on`.
            #[inline]
            $vis const fn set(&mut self, other: Self, on: bool) {
                if on {
                    self.insert(other);
                } else {
                    self.remove(other);
                }
            }
        }

        $(flag_set!(@packed $name, $packed);)?

        impl std::ops::BitOr for $name {
            type Output = Self;
            #[inline]
            fn bitor(self, other: Self) -> Self {
                self.union(other)
            }
        }

        impl std::fmt::Debug for $name {
            /// Names, not the packed byte, so a failing assertion reads `Sense(CLICK | DRAG)`.
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
        /// Bit 2 is skipped on purpose: the truncation test needs a gap inside the declared range.
        struct Fixture: packed {
            const A = 1 << 0;
            const B = 1 << 1;
            const C = 1 << 3;
        }
    }

    /// Set algebra is pinned against hand-worked values.
    #[test]
    fn set_algebra_matches_the_bits_it_claims() {
        let ab = Fixture::A.union(Fixture::B);
        assert_eq!(ab.bits(), 0b0011);

        // `contains` is "all of", `intersects` is "any of".
        assert!(ab.contains(Fixture::A));
        assert!(ab.contains(ab));
        assert!(!ab.contains(Fixture::A.union(Fixture::C)));
        assert!(ab.intersects(Fixture::A.union(Fixture::C)));
        assert!(!ab.intersects(Fixture::C));

        // The empty set is contained by everything and intersects nothing.
        assert!(ab.contains(Fixture::NONE));
        assert!(!ab.intersects(Fixture::NONE));
        assert!(Fixture::NONE.is_empty());
        assert!(!ab.is_empty());

        assert_eq!(ab.difference(Fixture::A), Fixture::B);
        assert_eq!(ab.difference(Fixture::C), ab);
        assert_eq!(Fixture::A | Fixture::B, ab);

        assert_eq!(Fixture::ALL.bits(), 0b1011);
        assert_eq!(Fixture::NONE.bits(), 0);
        assert_eq!(Fixture::default(), Fixture::NONE);
    }

    /// No value can hold a bit that names no flag, so matches on a set are exhaustive.
    #[test]
    fn undeclared_bits_cannot_survive_a_round_trip() {
        // `Fixture` leaves bits 2 and 4..8 unnamed.
        assert_eq!(Fixture::from_bits_truncate(0b1111_0100), Fixture::NONE);
        assert_eq!(
            Fixture::from_bits_truncate(0b1111_0110),
            Fixture::B,
            "the undeclared bits go, the declared one stays",
        );
        assert_eq!(Fixture::from_bits_truncate(u8::MAX), Fixture::ALL);
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

    /// Debug prints names, which a failing mask assertion needs.
    #[test]
    fn debug_names_the_flags_that_are_set() {
        assert_eq!(format!("{:?}", Fixture::NONE), "Fixture(empty)");
        assert_eq!(format!("{:?}", Fixture::A), "Fixture(A)");
        assert_eq!(
            format!("{:?}", Fixture::A.union(Fixture::C)),
            "Fixture(A | C)",
        );
    }
}
