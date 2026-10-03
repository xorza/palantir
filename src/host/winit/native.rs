//! Conversions between Palantir's backend-agnostic window vocabulary and
//! winit's, plus native window creation. Together with
//! [`input`](crate::host::winit::input) this is the whole of what the
//! windowed host knows about winit *types* — the rest of the module deals
//! only in winit's *lifecycle* (the event loop and its callbacks).

use std::sync::Arc;

use glam::{IVec2, UVec2};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Icon, Window as WinitWindow, WindowAttributes};

use crate::host::winit::error::WinitHostError;
use crate::primitives::paint::image::Image;
use crate::window::cursor_icon::CursorIcon;
use crate::window::window_config::WindowConfig;
use crate::window::window_token::WindowToken;

/// Map the backend-agnostic cursor vocabulary onto winit's.
pub(super) fn cursor(cursor: CursorIcon) -> winit::window::CursorIcon {
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

pub(super) fn icon(icon: &Image) -> Icon {
    Icon::from_rgba(icon.pixels.clone(), icon.size.x, icon.size.y)
        .expect("validated Image rejected by winit")
}

/// Build a winit `Window` from a [`WindowConfig`]. Converts the
/// backend-agnostic logical `UVec2` sizes into winit `LogicalSize` here so the
/// winit type stays inside this module.
pub(super) fn create_window(
    event_loop: &ActiveEventLoop,
    token: WindowToken,
    cfg: &WindowConfig,
) -> Result<Arc<WinitWindow>, WinitHostError> {
    let mut attrs = WinitWindow::default_attributes()
        .with_title(cfg.title.clone())
        .with_maximized(cfg.placement.maximized);
    if let Some(s) = cfg.inner_size {
        attrs = attrs.with_inner_size(LogicalSize::new(s.x, s.y));
    }
    if let Some(s) = cfg.min_inner_size {
        attrs = attrs.with_min_inner_size(LogicalSize::new(s.x, s.y));
    }
    if let Some(image) = &cfg.icon {
        attrs = attrs.with_window_icon(Some(icon(image)));
    }
    attrs = with_app_id(attrs, cfg);
    // Restore a saved position only if it still lands on a connected
    // monitor — winit does no such clamping, so a window saved on a
    // since-disconnected display would otherwise reopen off-screen and
    // unreachable.
    if let Some(p) = cfg.placement.position
        && position_on_monitor(event_loop, p)
    {
        attrs = attrs.with_position(PhysicalPosition::new(p.x, p.y));
    }
    event_loop
        .create_window(attrs)
        .map(Arc::new)
        .map_err(|source| WinitHostError::CreateWindow { token, source })
}

/// Apply [`WindowConfig::app_id`] on the platforms that have one.
///
/// Wayland's `app_id` and X11's `WM_CLASS` are the *same* winit attribute,
/// reached through one extension trait per backend, so writing it through
/// either covers both and whichever session is running reads it. The instance
/// name repeats the general one: Wayland ignores the instance outright, and
/// X11's `WM_CLASS(STRING) = "instance", "general"` conventionally carries the
/// application name twice.
///
/// X11 would survive without this — winit falls back to `argv[0]`'s file name
/// — but Wayland has no fallback at all, so an unnamed window reaches the
/// shell with nothing to match against its `.desktop` entry.
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
))]
fn with_app_id(attrs: WindowAttributes, cfg: &WindowConfig) -> WindowAttributes {
    // Inside the fn rather than at the top of the file: the trait is only
    // reachable on these targets.
    use winit::platform::wayland::WindowAttributesExtWayland as _;

    match &cfg.app_id {
        Some(app_id) => attrs.with_name(app_id.clone(), app_id.clone()),
        None => attrs,
    }
}

/// Nothing to apply: application identity comes from the `.app` bundle on
/// macOS and from the executable on Windows, neither of which is a per-window
/// hint.
#[cfg(not(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
)))]
fn with_app_id(attrs: WindowAttributes, _cfg: &WindowConfig) -> WindowAttributes {
    attrs
}

/// Whether `pos` (physical, window top-left) falls inside any currently
/// connected monitor's bounds — the guard that keeps a restored position
/// from placing the window off every screen.
fn position_on_monitor(event_loop: &ActiveEventLoop, pos: IVec2) -> bool {
    within_any(
        pos,
        event_loop.available_monitors().map(|m| {
            let (at, size) = (m.position(), m.size());
            (IVec2::new(at.x, at.y), UVec2::new(size.width, size.height))
        }),
    )
}

/// Whether `pos` lies in any of `monitors`, each a physical top-left and
/// extent: the top-left edges inside, the far edges out.
fn within_any(pos: IVec2, monitors: impl IntoIterator<Item = (IVec2, UVec2)>) -> bool {
    monitors.into_iter().any(|(at, size)| {
        let end = at + size.as_ivec2();
        pos.x >= at.x && pos.y >= at.y && pos.x < end.x && pos.y < end.y
    })
}

/// The window's physical extent, as the graphics layer wants it.
pub(super) fn physical_size(window: &WinitWindow) -> UVec2 {
    let size = window.inner_size();
    UVec2::new(size.width, size.height)
}

#[cfg(test)]
mod tests {
    use crate::host::winit::native;
    use crate::primitives::paint::image::Image;
    use glam::{IVec2, UVec2};

    /// A restored position must land on a screen: inside one of two
    /// side-by-side monitors counts, their top-left edges included and
    /// their far edges not, and no monitor at all places nothing.
    #[test]
    fn a_position_is_on_a_monitor_only_inside_its_bounds() {
        let left = (IVec2::new(0, 0), UVec2::new(1920, 1080));
        let right = (IVec2::new(1920, 0), UVec2::new(2560, 1440));
        for (pos, on) in [
            (IVec2::new(100, 100), true),
            (IVec2::new(0, 0), true),
            (IVec2::new(1919, 1079), true),
            (IVec2::new(1920, 1200), true),
            (IVec2::new(1919, 1080), false),
            (IVec2::new(4480, 0), false),
            (IVec2::new(-1, 10), false),
        ] {
            assert_eq!(native::within_any(pos, [left, right]), on, "{pos}");
        }
        assert!(!native::within_any(IVec2::ZERO, []), "no monitor, no place");
    }

    #[test]
    fn validated_window_icon_converts_to_the_platform_type() {
        let image = Image::from_srgba8(UVec2::new(2, 1), vec![255, 0, 0, 255, 0, 255, 0, 128]);
        let _ = native::icon(&image);
    }
}
