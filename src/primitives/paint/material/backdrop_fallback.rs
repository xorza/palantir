//! [`BackdropFallback`]: what a backdrop effect paints in its place while transparency is reduced.

use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::color::RgbaF32;

/// What a backdrop effect paints in its place while transparency is reduced
/// ([`Ui::reductions`](crate::Reductions::transparency)), a property of any
/// backdrop effect: a [`Material`](crate::Material)'s box draws as a plain one, this its fill, its
/// border and shadow as they are, with no capture, blur or effect, and a backdrop
/// [`EffectPaint`](crate::EffectPaint) paints it over its shape alike.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum BackdropFallback {
    /// The theme's surface, [`Theme::material_fallback`](crate::Theme::material_fallback): its
    /// float surface, translucent at the least alpha that keeps its text readable over any page.
    /// Resolved where the box records, so it follows the theme and its scheme.
    #[default]
    ThemeSurface,
    /// A brush of the author's: a scrim's own dim, a float's own panel, or a gradient look.
    Brush(Brush),
}

impl BackdropFallback {
    /// Whether this is [`Self::ThemeSurface`], the default a file leaves out. Takes `&self`
    /// because serde names it in a `skip_serializing_if`.
    pub(crate) const fn is_theme_surface(&self) -> bool {
        matches!(self, Self::ThemeSurface)
    }

    /// The brush it paints, the theme's surface being `theme`.
    pub(crate) fn brush(&self, theme: RgbaF32) -> Brush {
        match self {
            Self::ThemeSurface => Brush::Solid(theme),
            Self::Brush(brush) => brush.clone(),
        }
    }

    /// Whether it paints nothing, the theme's surface being `theme`.
    pub(crate) const fn is_noop(&self, theme: RgbaF32) -> bool {
        match self {
            Self::ThemeSurface => theme.is_noop(),
            Self::Brush(brush) => brush.is_noop(),
        }
    }

    /// Whether its brush holds its kinds; the theme's surface always does, as the theme checks it.
    pub(crate) const fn is_valid(&self) -> bool {
        match self {
            Self::ThemeSurface => true,
            Self::Brush(brush) => brush.is_valid(),
        }
    }

    /// Panics unless its brush holds its kinds.
    #[track_caller]
    pub(crate) const fn validate(&self) {
        if let Self::Brush(brush) = self {
            brush.validate();
        }
    }
}

/// Two brushes blend as a [`Brush`] does; the theme's surface and a brush switch at `t = 0.5`, as
/// the surface resolves only where the box records.
impl Lerp for BackdropFallback {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        match (a, b) {
            (Self::Brush(a), Self::Brush(b)) => Self::Brush(Brush::lerp(a, b, t)),
            _ if t < 0.5 => a.clone(),
            _ => b.clone(),
        }
    }
}

impl NanCheck for BackdropFallback {
    fn has_nan(&self) -> bool {
        match self {
            Self::ThemeSurface => false,
            Self::Brush(brush) => brush.has_nan(),
        }
    }
}
