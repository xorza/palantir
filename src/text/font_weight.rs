//! [`FontWeight`] — the numeric weight axis a face is matched on.

use std::fmt;

/// How black a face is, on the CSS 1–1000 scale: 400 is regular, 700 is
/// bold.
///
/// A number rather than an enum, because that is what the axis is
/// everywhere it is matched — CSS `font-weight`, a variable font's `wght`
/// axis, fontdb's own `Weight`. The named constants are the nine CSS
/// steps, and a variable face takes any value between them.
///
/// Ten bits of the shape-cache key hold one, which is what [`Self::new`]
/// checks the range against.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontWeight(u16);

impl FontWeight {
    pub const THIN: Self = Self(100);
    pub const EXTRA_LIGHT: Self = Self(200);
    pub const LIGHT: Self = Self(300);
    pub const REGULAR: Self = Self(400);
    pub const MEDIUM: Self = Self(500);
    pub const SEMI_BOLD: Self = Self(600);
    pub const BOLD: Self = Self(700);
    pub const EXTRA_BOLD: Self = Self(800);
    pub const BLACK: Self = Self(900);

    /// The widest value the axis holds, and the width of the key field
    /// that carries it. Stated here rather than in the key, because it is
    /// the type that decides what a weight can be.
    pub(crate) const MAX: u16 = 1000;

    /// A weight anywhere on the axis, including between the named steps.
    ///
    /// # Panics
    ///
    /// Panics outside `1..=1000`, which is the whole CSS range and the
    /// whole `wght` range a variable face registers. A weight is authored
    /// in a theme or a builder, never taken from a frame, so this is a
    /// cold check on public-API misuse.
    pub const fn new(weight: u16) -> Self {
        assert!(Self::in_range(weight), "a font weight is 1..=1000");
        Self(weight)
    }

    /// The axis, stated once — three callers check against it, and a
    /// range spelled at each is one that can move in only some.
    const fn in_range(weight: u16) -> bool {
        weight >= 1 && weight <= Self::MAX
    }

    pub const fn value(self) -> u16 {
        self.0
    }

    /// The key's spelling of a weight, decoded.
    ///
    /// Unchecked in release, unlike [`Self::new`]: the ten bits it reads
    /// were written by this crate from a weight that had already passed
    /// that check, so a bad value here is a logic error — and this runs
    /// per shape.
    pub(crate) const fn from_raw(raw: u16) -> Self {
        debug_assert!(Self::in_range(raw));
        Self(raw)
    }
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::REGULAR
    }
}

/// The number alone: `FontWeight(700)` reads as the axis value it is,
/// where a derived `Debug` would print the tuple wrapper twice over in a
/// nested style dump.
impl fmt::Debug for FontWeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FontWeight({})", self.0)
    }
}

/// The bare number, not a newtype wrapper: a theme file says
/// `weight: 700`, which is how every other font system spells the axis.
impl serde::Serialize for FontWeight {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u16(self.0)
    }
}

/// Validated on the way in, like [`Self::new`]: a theme file is untrusted
/// input, and an out-of-range weight would truncate inside the shape key
/// rather than fail.
impl<'de> serde::Deserialize<'de> for FontWeight {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let weight = u16::deserialize(deserializer)?;
        if !Self::in_range(weight) {
            return Err(serde::de::Error::custom("a font weight is 1..=1000"));
        }
        Ok(Self(weight))
    }
}

#[cfg(test)]
mod tests {
    use crate::text::font_weight::FontWeight;

    #[test]
    fn the_named_steps_are_the_css_scale() {
        assert_eq!(FontWeight::REGULAR.value(), 400);
        assert_eq!(FontWeight::BOLD.value(), 700);
        assert_eq!(FontWeight::default(), FontWeight::REGULAR);
        assert!(FontWeight::LIGHT < FontWeight::REGULAR);
        assert!(FontWeight::REGULAR < FontWeight::BOLD);
        assert_eq!(FontWeight::new(550).value(), 550);
    }

    #[test]
    #[should_panic(expected = "a font weight is 1..=1000")]
    fn a_weight_past_the_axis_is_rejected() {
        let _ = FontWeight::new(1001);
    }

    #[test]
    fn serde_carries_the_number_and_checks_the_range() {
        let encoded = ron::ser::to_string(&FontWeight::SEMI_BOLD).expect("serialize");
        assert_eq!(encoded, "600");
        assert_eq!(
            ron::from_str::<FontWeight>(&encoded).expect("parse"),
            FontWeight::SEMI_BOLD
        );
        assert!(ron::from_str::<FontWeight>("0").is_err());
        assert!(ron::from_str::<FontWeight>("1001").is_err());
    }
}
