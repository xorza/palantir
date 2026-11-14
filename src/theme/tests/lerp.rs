//! A theme's lerp: what blends, what switches halfway, and the text axes one end leaves to the
//! ambient style.

use std::time::Duration;

use glam::Vec2;

use crate::animation::animation_spec::AnimationSpec;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::ShadowKind;
use crate::text::font_family::FontFamily;
use crate::text::font_slant::FontSlant;
use crate::text::font_weight::FontWeight;
use crate::theme::close_shown::CloseShown;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::text_style::TextStyle;
use crate::theme::text_style_overrides::TextStyleOverrides;
use crate::theme::{Theme, ThemeText, ThemeWalk};

const RED: RgbaF32 = RgbaF32::new(1.0, 0.0, 0.0, 1.0);

/// Every text slot resolved over its theme's ambient style, in walk order.
fn resolved_text(theme: &mut Theme) -> Vec<TextStyle> {
    let ambient = theme.text;
    let mut styles = Vec::new();
    theme.for_each_text(None, &mut |text, _| {
        styles.push(match text {
            ThemeText::Style(style) => *style,
            ThemeText::Overrides(o) => o.apply(&ambient),
        });
    });
    styles
}

/// The stock themes come back exactly at either end, the light one's every color included. Each
/// step between makes valid faces, between the stock pair and toward a light theme at half and
/// at twice the text size.
#[test]
fn a_lerp_ends_on_either_theme() {
    let (dark, light) = (
        Theme::stock(ColorScheme::Dark),
        Theme::stock(ColorScheme::Light),
    );
    assert_eq!(Theme::lerp(&dark.clone(), &light, 0.0), dark);
    assert_eq!(Theme::lerp(&dark, &light.clone(), 1.0), light);
    assert_ne!(Theme::lerp(&dark.clone(), &light, 0.5), dark);

    for factor in [1.0, 0.5, 2.0] {
        let mut scaled = light.clone();
        scaled.scale_text(factor);
        for t in [0.25, 0.5, 0.75] {
            let step = Theme::lerp(&dark.clone(), &scaled.clone(), t);
            assert!(step.faces_valid(1.0), "{factor}× at {t}");
        }
    }
}

/// Numbers, spacings, radii, arrays, sizes and colors blend; a field the ends share stays
/// exactly itself.
#[test]
fn the_midpoint_of_a_built_pair_blends_every_value() {
    let mut a = Theme::stock(ColorScheme::Dark);
    let mut b = a.clone();
    a.focus_ring.width = 1.0;
    b.focus_ring.width = 3.0;
    a.button.defaults.padding = Spacing::all(4.0);
    b.button.defaults.padding = Spacing::xy(8.0, 12.0);
    a.checkbox.check_points = [Vec2::ZERO, Vec2::new(2.0, 2.0), Vec2::new(4.0, 0.0)];
    b.checkbox.check_points = [
        Vec2::new(2.0, 0.0),
        Vec2::new(2.0, 6.0),
        Vec2::new(8.0, 4.0),
    ];
    a.text.font_size = 14.0;
    b.text.font_size = 18.0;
    a.text.line_height_factor = 1.0;
    b.text.line_height_factor = 2.0;
    a.menu.panel.corners = Corners::all(4.0);
    b.menu.panel.corners = Corners::all(8.0);
    a.tooltip.max_size = Size::new(280.0, f32::INFINITY);
    b.tooltip.max_size = Size::new(320.0, 200.0);
    b.window_bg = RED;

    let mid = Theme::lerp(&a, &b, 0.5);
    // 1 + 2·½ = 2.
    assert_eq!(mid.focus_ring.width, 2.0);
    // (4, 4, 4, 4) + ((8, 12, 8, 12) − (4, 4, 4, 4))·½.
    assert_eq!(mid.button.defaults.padding, Spacing::xy(6.0, 8.0));
    assert_eq!(
        mid.checkbox.check_points,
        [
            Vec2::new(1.0, 0.0),
            Vec2::new(2.0, 4.0),
            Vec2::new(6.0, 2.0)
        ]
    );
    // 14 + 4·½ = 16; 1 + 1·½ = 1.5.
    assert_eq!(mid.text.font_size, 16.0);
    assert_eq!(mid.text.line_height_factor, 1.5);
    assert_eq!(mid.menu.panel.corners, Corners::all(6.0));
    // 280 + 40·½ = 300; the unbounded height switches to `b`'s at ½.
    assert_eq!(mid.tooltip.max_size, Size::new(300.0, 200.0));
    assert_eq!(mid.window_bg, RgbaF32::lerp(&a.window_bg, &RED, 0.5),);
    assert_ne!(mid.window_bg, a.window_bg);
    assert_eq!(mid.slider, a.slider);
    assert_eq!(mid.tabs, a.tabs);
}

/// A value with no blend shows `a`'s below `t = 0.5` and `b`'s from it.
#[test]
fn values_with_no_blend_switch_halfway() {
    let mut a = Theme::stock(ColorScheme::Dark);
    let mut b = a.clone();
    a.panel_clip = ClipMode::None;
    b.panel_clip = ClipMode::Rect;
    a.tabs.close_shown = CloseShown::Always;
    b.tabs.close_shown = CloseShown::SelectedOrHovered;
    a.spinner.period = Duration::from_secs(1);
    b.spinner.period = Duration::from_secs(2);
    a.tooltip.delay = Duration::from_millis(500);
    b.tooltip.delay = Duration::from_millis(900);
    a.button.defaults.transition = AnimationSpec::FAST;
    b.button.defaults.transition = AnimationSpec::SNAP;
    b.text.family = FontFamily::MONO;
    b.text.weight = FontWeight::BOLD;
    b.text.slant = FontSlant::Italic;
    b.button.unselected.normal.chrome.paint.shadow.kind =
        match a.button.unselected.normal.chrome.paint.shadow.kind {
            ShadowKind::Drop => ShadowKind::Inset,
            ShadowKind::Inset => ShadowKind::Drop,
        };

    for (t, want) in [(0.25, &a), (0.49, &a), (0.5, &b), (0.75, &b)] {
        let mix = Theme::lerp(&a.clone(), &b.clone(), t);
        assert_eq!(mix.panel_clip, want.panel_clip, "{t}");
        assert_eq!(mix.tabs.close_shown, want.tabs.close_shown, "{t}");
        assert_eq!(mix.spinner.period, want.spinner.period, "{t}");
        assert_eq!(mix.tooltip.delay, want.tooltip.delay, "{t}");
        assert_eq!(
            mix.button.defaults.transition, want.button.defaults.transition,
            "{t}"
        );
        assert_eq!(mix.text.family, want.text.family, "{t}");
        assert_eq!(mix.text.weight, want.text.weight, "{t}");
        assert_eq!(mix.text.slant, want.text.slant, "{t}");
        assert_eq!(
            mix.button.unselected.normal.chrome.paint.shadow.kind,
            want.button.unselected.normal.chrome.paint.shadow.kind,
            "{t}"
        );
    }
}

/// A panel chrome that appears fades in through `Chrome::NONE`: halfway its fill is at half
/// alpha.
#[test]
fn an_appearing_panel_chrome_is_half_there_halfway() {
    let mut a = Theme::stock(ColorScheme::Dark);
    a.panel_chrome = None;
    let mut b = a.clone();
    b.panel_chrome = Some(Chrome::new(RED));
    let mid = Theme::lerp(&a, &b, 0.5)
        .panel_chrome
        .expect("a blend, not an absence");
    assert_eq!(
        mid.paint.fill,
        RgbaF32::lerp(&RgbaF32::TRANSPARENT, &RED, 0.5).into()
    );
    let Brush::Solid(fill) = mid.paint.fill else {
        panic!("a solid fill blends to a solid fill");
    };
    assert_eq!(fill.a, 0.5);
}

/// An override axis one end names and the other leaves to its ambient style blends from what
/// each end shows: the absent side's ambient value. Halfway a slot shows the blend of what the
/// ends show; a face, with no blend, switches from the ambient one. On its own a set of overrides
/// has no ambient, so the axis switches halfway.
#[test]
fn a_text_axis_one_end_leaves_to_the_ambient_blends_from_it() {
    let mut a = Theme::stock(ColorScheme::Dark);
    let mut b = a.clone();
    a.text.color = RgbaF32::WHITE;
    b.text.color = RgbaF32::BLACK;
    a.text.font_size = 14.0;
    b.text.font_size = 18.0;
    let named = TextStyleOverrides {
        color: Some(RED),
        font_size: Some(20.0),
        family: Some(FontFamily::MONO),
        ..TextStyleOverrides::NONE
    };
    a.button.unselected.normal.text = TextStyleOverrides::NONE;
    b.button.unselected.normal.text = named;
    a.button.unselected.hovered.text = named;
    b.button.unselected.hovered.text = TextStyleOverrides::NONE;

    let mid = Theme::lerp(&a, &b, 0.5);
    let appearing = mid.button.unselected.normal.text;
    assert_eq!(
        appearing.color,
        Some(RgbaF32::lerp(&RgbaF32::WHITE, &RED, 0.5))
    );
    // 14 + (20 − 14)·½ = 17.
    assert_eq!(appearing.font_size, Some(17.0));
    assert_eq!(appearing.family, Some(FontFamily::MONO));
    let leaving = mid.button.unselected.hovered.text;
    assert_eq!(
        leaving.color,
        Some(RgbaF32::lerp(&RED, &RgbaF32::BLACK, 0.5))
    );
    // 20 + (18 − 20)·½ = 19.
    assert_eq!(leaving.font_size, Some(19.0));
    let early = Theme::lerp(&a, &b, 0.49);
    assert_eq!(
        early.button.unselected.normal.text.family,
        Some(FontFamily::SANS)
    );

    assert_eq!(
        TextStyleOverrides::lerp(&TextStyleOverrides::NONE, &named, 0.49),
        TextStyleOverrides::NONE
    );
    assert_eq!(
        TextStyleOverrides::lerp(&TextStyleOverrides::NONE, &named, 0.5),
        named
    );
}

/// Sharing the axes changes what no slot of either end shows.
#[test]
fn sharing_text_axes_keeps_what_each_end_shows() {
    let mut a = Theme::stock(ColorScheme::Dark);
    let mut b = Theme::stock(ColorScheme::Light);
    a.button.unselected.normal.text = TextStyleOverrides::NONE.with_font_size(20.0);
    b.tooltip.text = TextStyleOverrides::NONE;
    let (shown_a, shown_b) = (resolved_text(&mut a), resolved_text(&mut b));
    let before = (a.clone(), b.clone());
    Theme::share_text_axes(&mut a, &mut b);
    assert_ne!(
        (&a, &b),
        (&before.0, &before.1),
        "the pair names axes only one end had"
    );
    assert_eq!(resolved_text(&mut a), shown_a);
    assert_eq!(resolved_text(&mut b), shown_b);
}

/// A blend in place is the blend, bit for bit: `Lerp::lerp_into` of borrowed ends, and
/// `Theme::lerp_ends_into` of ends whose text axes were shared once, as a theme transition blends
/// them, match `Theme::lerp` at every phase, between the stock pair and between ends whose
/// overrides name different text axes. `out` starts as a third theme, so nothing it held stays.
#[test]
fn a_blend_in_place_is_the_blend() {
    let mut a = Theme::stock(ColorScheme::Dark);
    let mut b = Theme::stock(ColorScheme::Light);
    b.scale_text(1.25);
    let named = TextStyleOverrides {
        color: Some(RED),
        font_size: Some(20.0),
        family: Some(FontFamily::MONO),
        ..TextStyleOverrides::NONE
    };
    a.button.unselected.normal.text = TextStyleOverrides::NONE;
    b.button.unselected.normal.text = named;
    a.button.unselected.hovered.text = named;
    b.button.unselected.hovered.text = TextStyleOverrides::NONE;
    let exact = |theme: &Theme| ron::to_string(theme).unwrap();
    let pairs = vec![
        (
            Theme::stock(ColorScheme::Dark),
            Theme::stock(ColorScheme::Light),
        ),
        (a, b),
    ];
    for (from, to) in pairs {
        let (mut shared_from, mut shared_to) = (from.clone(), to.clone());
        Theme::share_text_axes(&mut shared_from, &mut shared_to);
        for t in [0.0, 0.1, 0.37, 0.49, 0.5, 0.9, 1.0] {
            let want = exact(&Theme::lerp(&from, &to, t));
            let mut out = Theme::stock(ColorScheme::Light);
            out.scale_text(0.5);
            Theme::lerp_into(&from, &to, t, &mut out);
            assert_eq!(exact(&out), want, "lerp_into at {t}");
            if t != 0.0 && t != 1.0 {
                Theme::lerp_ends_into(&shared_from, &shared_to, t, &mut out);
                assert_eq!(exact(&out), want, "lerp_ends_into at {t}");
            }
        }
    }
}
