#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    /// `(r, g, b)` in 0..1 sRGB space (the default — matches CSS, Figma, Photoshop).
    /// Linearized internally so blending and SDF AA happen correctly in linear space.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self::rgba(r, g, b, 1.0)
    }
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
            a,
        }
    }

    /// `(r, g, b)` already in linear (scene-referred) space. Use for tween outputs,
    /// physically-derived values, or interop with linear pipelines.
    pub const fn linear_rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }
    pub const fn linear_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Multiply the linear RGB channels by `mul`, preserve alpha. Used by the
    /// encoder to dim disabled subtrees.
    pub const fn dim_rgb(self, mul: f32) -> Self {
        Self {
            r: self.r * mul,
            g: self.g * mul,
            b: self.b * mul,
            a: self.a,
        }
    }

    /// 8-bit sRGB channels (Figma/CSS/Photoshop convention). Linearized
    /// internally, same as `Color::rgb`. `#3366CC` → `Color::rgb_u8(0x33, 0x66, 0xCC)`.
    pub const fn rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }
    /// `rgb_u8` with 8-bit alpha. Alpha is not gamma-encoded — straight `a / 255`.
    pub const fn rgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self::rgba(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    /// Packed 24-bit `0xRRGGBB` sRGB literal, opaque. Matches CSS hex
    /// notation: `#3366CC` → `Color::hex(0x3366CC)`.
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb_u8(
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
        )
    }
    /// Packed 32-bit `0xRRGGBBAA` sRGB+alpha literal. CSS-order (alpha last).
    pub const fn hexa(rgba: u32) -> Self {
        Self::rgba_u8(
            ((rgba >> 24) & 0xff) as u8,
            ((rgba >> 16) & 0xff) as u8,
            ((rgba >> 8) & 0xff) as u8,
            (rgba & 0xff) as u8,
        )
    }
}

/// sRGB→linear via cubic polynomial. Const-friendly (`f32::powf` is not
/// const-stable; see rust-lang/rust#57241). Industry-standard cubic fit
/// (Hejl-Burgess-Dawson and similar) over `[0, 1]`; max abs error ~1.5e-3
/// in linear space — well below 8-bit display precision (1/255 ≈ 4e-3),
/// so the difference is invisible in rendered output. Pinned by
/// `tests::cubic_srgb_max_error_under_two_thousandths`.
const fn srgb_to_linear(c: f32) -> f32 {
    c * (c * (c * 0.305_306_01 + 0.682_171_1) + 0.012_522_878)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference: spec-exact piecewise sRGB→linear (the previous in-tree
    /// implementation). Used as ground truth for the cubic approximation.
    fn srgb_to_linear_exact(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// Pin: the cubic stays within ~1.5e-3 of the spec-exact piecewise
    /// curve across `[0, 1]`. A regression past 2e-3 suggests the
    /// coefficients drifted; revisit before shipping.
    #[test]
    fn cubic_srgb_max_error_under_two_thousandths() {
        let mut max_err: f32 = 0.0;
        // Sweep at 1/1024 resolution — finer than 8-bit display, plenty
        // to catch the worst-case point.
        for i in 0..=1024 {
            let c = i as f32 / 1024.0;
            let approx = srgb_to_linear(c);
            let exact = srgb_to_linear_exact(c);
            let err = (approx - exact).abs();
            if err > max_err {
                max_err = err;
            }
        }
        assert!(
            max_err < 2.0e-3,
            "cubic max error {max_err} exceeded 2e-3 threshold"
        );
    }

    /// Sanity: const-construction works in const context. If `Color::rgb`
    /// regresses to non-const, this fails to compile.
    #[test]
    fn rgb_is_const_constructible() {
        const _LITERAL: Color = Color::rgb(0.2, 0.4, 0.8);
        const _HEX: Color = Color::hex(0x3366CC);
    }
}
