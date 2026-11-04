//! Conversions between Palantir's window vocabulary and winit's, plus native window creation.

use std::sync::Arc;

use glam::{IVec2, UVec2};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Icon, Window as WinitWindow};

use crate::host::winit::error::WinitHostError;
use crate::host::winit::platform;
use crate::primitives::paint::image::Image;
use crate::theme::color_scheme::ColorScheme;
use crate::window::cursor_icon::CursorIcon;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;
use winit::window;

pub(super) const fn cursor(cursor: CursorIcon) -> window::CursorIcon {
    use winit::window::CursorIcon as W;
    match cursor {
        CursorIcon::Default => W::Default,
        CursorIcon::Pointer => W::Pointer,
        CursorIcon::Text => W::Text,
        CursorIcon::Grab => W::Grab,
        CursorIcon::Grabbing => W::Grabbing,
        CursorIcon::Move => W::Move,
        CursorIcon::Crosshair => W::Crosshair,
        CursorIcon::EwResize => W::EwResize,
        CursorIcon::NsResize => W::NsResize,
        CursorIcon::NotAllowed => W::NotAllowed,
    }
}

pub(super) const fn color_scheme(scheme: ColorScheme) -> window::Theme {
    match scheme {
        ColorScheme::Light => window::Theme::Light,
        ColorScheme::Dark => window::Theme::Dark,
    }
}

pub(super) fn icon(icon: &Image) -> Icon {
    Icon::from_rgba(icon.pixels.clone(), icon.size.x, icon.size.y)
        .expect("validated Image rejected by winit")
}

/// Builds a winit `Window` from a [`WindowConfig`], its decorations in `scheme`; winit types stay
/// inside this module.
pub(super) fn create_window(
    event_loop: &ActiveEventLoop,
    token: WindowToken,
    cfg: &WindowConfig,
    scheme: ColorScheme,
) -> Result<Arc<WinitWindow>, WinitHostError> {
    let mut attrs = WinitWindow::default_attributes()
        .with_title(cfg.title.clone())
        .with_theme(Some(color_scheme(scheme)))
        .with_maximized(cfg.placement.maximized);
    if let Some(s) = cfg.inner_size {
        attrs = attrs.with_inner_size(LogicalSize::new(s.w, s.h));
    }
    if let Some(s) = cfg.min_inner_size {
        attrs = attrs.with_min_inner_size(LogicalSize::new(s.w, s.h));
    }
    if let Some(image) = &cfg.icon {
        attrs = attrs.with_window_icon(Some(icon(image)));
    }
    attrs = platform::with_app_id(attrs, cfg.app_id.as_deref());
    // Restore a saved position only where the title bar reaches a connected monitor; winit does
    // not clamp.
    if let Some(p) = cfg.placement.physical_pos
        && title_bar_on_monitor(event_loop, p, cfg.inner_size.map(|size| size.w))
    {
        attrs = attrs.with_position(PhysicalPosition::new(p.x, p.y));
    }
    event_loop
        .create_window(attrs)
        .map(Arc::new)
        .map_err(|source| WinitHostError::CreateWindow { token, source })
}

/// The logical rows of a window's top that its title bar takes, and its width where the window's
/// is unknown: what must reach a monitor for the user to grab the window.
const TITLE_BAND: f64 = 32.0;

/// A connected monitor: its top-left and extent in physical px, and its scale.
#[derive(Clone, Copy, Debug)]
struct MonitorArea {
    at: IVec2,
    size: UVec2,
    scale: f64,
}

/// Whether the title bar of a window whose frame starts at `pos` (physical) and whose content is
/// `inner_width` logical px wide reaches any connected monitor.
fn title_bar_on_monitor(
    event_loop: &ActiveEventLoop,
    pos: IVec2,
    inner_width: Option<f32>,
) -> bool {
    title_bar_reaches(
        pos,
        inner_width,
        event_loop.available_monitors().map(|m| {
            let (at, size) = (m.position(), m.size());
            MonitorArea {
                at: IVec2::new(at.x, at.y),
                size: UVec2::new(size.width, size.height),
                scale: m.scale_factor(),
            }
        }),
    )
}

/// Whether the band a title bar takes, [`TITLE_BAND`] logical rows from `pos` and `inner_width`
/// logical px wide, both at a monitor's own scale, overlaps that monitor for any of `monitors`.
/// Qt's `restoreGeometry` keeps a window whose title bar shows. Refused: the frame's top-left on
/// a monitor, which a window snapped to the left edge on Windows misses, as its invisible resize
/// border starts 7 px left of the screen.
fn title_bar_reaches(
    pos: IVec2,
    inner_width: Option<f32>,
    monitors: impl IntoIterator<Item = MonitorArea>,
) -> bool {
    let (x, y) = (i64::from(pos.x), i64::from(pos.y));
    monitors.into_iter().any(|monitor| {
        let width = inner_width.map_or(TITLE_BAND, f64::from) * monitor.scale;
        let band_end_x = x + width.ceil() as i64;
        let band_end_y = y + (TITLE_BAND * monitor.scale).ceil() as i64;
        let (at_x, at_y) = (i64::from(monitor.at.x), i64::from(monitor.at.y));
        let end_x = at_x + i64::from(monitor.size.x);
        let end_y = at_y + i64::from(monitor.size.y);
        x < end_x && at_x < band_end_x && y < end_y && at_y < band_end_y
    })
}

pub(super) fn physical_size(window: &WinitWindow) -> UVec2 {
    let size = window.inner_size();
    UVec2::new(size.width, size.height)
}

#[cfg(test)]
mod tests {
    use crate::host::winit::native;
    use crate::primitives::paint::image::Image;
    use glam::{IVec2, UVec2};

    /// A restored position stands where its title bar, 32 logical rows as wide as the window,
    /// reaches a screen. Beside a 1920 × 1080 monitor at scale 1 sits a 2560 × 1440 one at scale
    /// 2, whose band is 64 rows tall. A window snapped to the left edge on Windows starts 7 px off
    /// the screen and stands; one whose title bar lies past every edge, above the top or wholly
    /// left of the screen, falls back to the platform's placement. A window of unknown width
    /// takes a 32-px band: its corner one texel off a monitor's far edge still reaches it.
    #[test]
    fn a_position_stands_where_its_title_bar_reaches_a_monitor() {
        let left = native::MonitorArea {
            at: IVec2::new(0, 0),
            size: UVec2::new(1920, 1080),
            scale: 1.0,
        };
        let right = native::MonitorArea {
            at: IVec2::new(1920, 0),
            size: UVec2::new(2560, 1440),
            scale: 2.0,
        };
        for (pos, inner_width, on, why) in [
            (IVec2::new(100, 100), Some(800.0), true, "inside"),
            (
                IVec2::new(-7, 0),
                Some(960.0),
                true,
                "snapped to the left edge",
            ),
            (
                IVec2::new(-2000, 0),
                Some(960.0),
                false,
                "wholly left of the screen",
            ),
            (
                IVec2::new(-2000, 0),
                Some(2100.0),
                true,
                "its title bar's right end shows",
            ),
            (
                IVec2::new(100, -40),
                Some(800.0),
                false,
                "above the top of the left screen",
            ),
            (
                IVec2::new(100, -20),
                Some(800.0),
                true,
                "its title bar's lower rows show",
            ),
            (
                IVec2::new(2000, -40),
                Some(800.0),
                true,
                "a 64-row band at scale 2 shows",
            ),
            (IVec2::new(4480, 0), Some(800.0), false, "past the far edge"),
            (
                IVec2::new(1919, 1079),
                None,
                true,
                "a corner texel on the left screen",
            ),
            (
                IVec2::new(-31, -31),
                None,
                true,
                "a 32-px band's last texel",
            ),
            (
                IVec2::new(-32, 0),
                None,
                false,
                "a 32-px band ending at the edge",
            ),
        ] {
            assert_eq!(
                native::title_bar_reaches(pos, inner_width, [left, right]),
                on,
                "{pos}, {why}"
            );
        }
        assert!(
            !native::title_bar_reaches(IVec2::ZERO, Some(800.0), []),
            "no monitor, no place"
        );
    }

    #[test]
    fn validated_window_icon_converts_to_the_platform_type() {
        let image =
            Image::from_srgba8(UVec2::new(2, 1), vec![255, 0, 0, 255, 0, 255, 0, 128]).unwrap();
        let _ = native::icon(&image);
    }
}
