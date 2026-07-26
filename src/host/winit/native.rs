//! Conversions between Aperture's backend-agnostic window vocabulary and
//! winit's, plus native window creation. Together with
//! [`input`](crate::host::winit::input) this is the whole of what the
//! windowed host knows about winit *types* — the rest of the module deals
//! only in winit's *lifecycle* (the event loop and its callbacks).

use std::sync::Arc;

use glam::IVec2;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Icon, Window as WinitWindow};

use crate::host::winit::error::WinitHostError;
use crate::primitives::image::Image;
use crate::window::{CursorIcon, WindowConfig, WindowToken};

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
        .with_maximized(cfg.maximized);
    if let Some(s) = cfg.inner_size {
        attrs = attrs.with_inner_size(LogicalSize::new(s.x, s.y));
    }
    if let Some(s) = cfg.min_inner_size {
        attrs = attrs.with_min_inner_size(LogicalSize::new(s.x, s.y));
    }
    if let Some(image) = &cfg.icon {
        attrs = attrs.with_window_icon(Some(icon(image)));
    }
    // Restore a saved position only if it still lands on a connected
    // monitor — winit does no such clamping, so a window saved on a
    // since-disconnected display would otherwise reopen off-screen and
    // unreachable.
    if let Some(p) = cfg.position
        && position_on_monitor(event_loop, p)
    {
        attrs = attrs.with_position(PhysicalPosition::new(p.x, p.y));
    }
    event_loop
        .create_window(attrs)
        .map(Arc::new)
        .map_err(|source| WinitHostError::CreateWindow { token, source })
}

/// Whether `pos` (physical, window top-left) falls inside any currently
/// connected monitor's bounds — the guard that keeps a restored position
/// from placing the window off every screen.
fn position_on_monitor(event_loop: &ActiveEventLoop, pos: IVec2) -> bool {
    event_loop.available_monitors().any(|m| {
        let mp = m.position();
        let ms = m.size();
        pos.x >= mp.x
            && pos.y >= mp.y
            && pos.x < mp.x + ms.width as i32
            && pos.y < mp.y + ms.height as i32
    })
}

#[cfg(test)]
mod tests {
    use crate::host::winit::native;
    use crate::primitives::image::Image;

    #[test]
    fn validated_window_icon_converts_to_the_platform_type() {
        let image = Image::from_rgba8(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 128]);
        let _ = native::icon(&image);
    }
}
