//! [`FontWeight`] — the numeric weight axis a face is matched on.

use serde::de;
use std::fmt;

/// How black a face is, on the CSS 1–1000 scale (400 regular, 700 bold); the named constants are the nine CSS steps.
///
/// Ten bits of the shape-cache key hold one; [`Self::new`] checks the range.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontWeight(u16);

impl FontWeight {
    /// Weight 100.
    pub const THIN: Self = Self(100);
    /// Weight 200.
    pub const EXTRA_LIGHT: Self = Self(200);
    /// Weight 300.
    pub const LIGHT: Self = Self(300);
    /// CSS 400 — the default a [`TextStyle`](crate::TextStyle) starts at.
    pub const REGULAR: Self = Self(400);
    /// Weight 500.
    pub const MEDIUM: Self = Self(500);
    /// Weight 600.
    pub const SEMI_BOLD: Self = Self(600);
    /// CSS 700 — what [`Text::bold`](crate::Text::bold) selects.
    pub const BOLD: Self = Self(700);
    /// Weight 800.
    pub const EXTRA_BOLD: Self = Self(800);
    /// Weight 900.
    pub const BLACK: Self = Self(900);

    /// The widest value the axis holds, and the width of the key field that carries it.
    pub(crate) const MAX: u16 = 1000;

    /// A weight anywhere on the axis, including between the named steps.
    ///
    /// # Panics
    ///
    /// Panics outside `1..=1000`, the whole CSS and `wght` range. Cold check on public-API misuse.
    #[track_caller]
    pub const fn new(weight: u16) -> Self {
        assert!(Self::in_range(weight), "a font weight is 1..=1000");
        Self(weight)
    }

    /// The axis, stated once; three callers check against it.
    const fn in_range(weight: u16) -> bool {
        weight >= 1 && weight <= Self::MAX
    }

    /// This weight as a bare number on the 1–1000 scale.
    pub const fn get(self) -> u16 {
        self.0
    }

    /// The key's spelling of a weight, decoded; unchecked in release, since the bits came from a checked weight and this runs per shape.
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

/// The number alone (`FontWeight(700)`), not the tuple wrapper in nested style dumps.
impl fmt::Debug for FontWeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FontWeight({})", self.0)
    }
}

/// The bare number: a theme file says `weight: 700`.
impl serde::Serialize for FontWeight {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u16(self.0)
    }
}

/// Validated like [`Self::new`]: a theme file is untrusted and an out-of-range weight would truncate inside the shape key.
impl<'de> serde::Deserialize<'de> for FontWeight {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let weight = u16::deserialize(deserializer)?;
        if !Self::in_range(weight) {
            return Err(de::Error::custom("a font weight is 1..=1000"));
        }
        Ok(Self(weight))
    }
}

#[cfg(test)]
mod tests {
    use crate::text::font_weight::FontWeight;
    use ron::ser;

    #[test]
    fn the_named_steps_are_the_css_scale() {
        assert_eq!(FontWeight::REGULAR.get(), 400);
        assert_eq!(FontWeight::BOLD.get(), 700);
        assert_eq!(FontWeight::default(), FontWeight::REGULAR);
        assert!(FontWeight::LIGHT < FontWeight::REGULAR);
        assert!(FontWeight::REGULAR < FontWeight::BOLD);
        assert_eq!(FontWeight::new(550).get(), 550);
    }

    #[test]
    #[should_panic(expected = "a font weight is 1..=1000")]
    fn a_weight_past_the_axis_is_rejected() {
        let _ = FontWeight::new(1001);
    }

    #[test]
    fn serde_carries_the_number_and_checks_the_range() {
        let encoded = ser::to_string(&FontWeight::SEMI_BOLD).expect("serialize");
        assert_eq!(encoded, "600");
        assert_eq!(
            ron::from_str::<FontWeight>(&encoded).expect("parse"),
            FontWeight::SEMI_BOLD
        );
        assert!(ron::from_str::<FontWeight>("0").is_err());
        assert!(ron::from_str::<FontWeight>("1001").is_err());
    }
}
