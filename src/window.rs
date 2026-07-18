//! Backend-agnostic window vocabulary shared by the recorder
//! ([`Ui`](crate::Ui)) and the windowing host
//! ([`WinitHost`](crate::WinitHost)). Both depend *into* this module and
//! neither back out, so the recorder never reaches up into the winit
//! backend — [`WindowRequests`], [`WindowFrameState`], and `WindowConfig`
//! deliberately carry no
//! winit/wgpu types.

use glam::{IVec2, UVec2};

/// Caller-chosen opaque identity for a window. Supplied at
/// [`Ui::open_window`](crate::Ui::open_window) (and
/// [`WinitHost::builder`](crate::WinitHost::builder) or
/// [`OffscreenHost::builder`](crate::OffscreenHost::builder) for a host's
/// first window),
/// handed back to [`App::update`](crate::App::update) and
/// [`App::record`](crate::App::record), and used
/// to address a window in [`Ui::close_window`](crate::Ui::close_window) /
/// [`HostHandle::request_repaint`](crate::HostHandle::request_repaint).
/// The app owns the semantics — use it as an enum discriminant, an index,
/// a document-id hash, whatever. Aperture only stores and compares it;
/// winit's `WindowId` never reaches the app.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowToken(pub u64);

/// Per-window options — what [`Ui::open_window`](crate::Ui::open_window)
/// takes (and what the first window's options live in inside
/// [`WinitHostConfig`](crate::WinitHostConfig)). Backend-agnostic by
/// design: no winit or wgpu types, so opening a window from app code
/// doesn't pull the windowing backend into the `Ui` API. Sizes are
/// `UVec2` logical pixels (DPI-independent), `.x` = width, `.y` = height
/// — the same integer-extent vocabulary as [`Display`](crate::Display).
#[derive(Clone, Debug, Default)]
pub struct WindowConfig {
    pub(crate) title: String,
    /// Initial inner size in logical pixels. `None` lets the platform
    /// pick.
    pub(crate) inner_size: Option<UVec2>,
    /// Minimum inner size in logical pixels. `None` = no floor.
    pub(crate) min_inner_size: Option<UVec2>,
    /// Initial outer position in **physical** pixels (top-left of the
    /// window frame). `None` lets the platform place it. Physical, not
    /// logical, because a saved position is only unambiguous across
    /// mixed-DPI monitors in device pixels. The host drops it at creation
    /// if it no longer lands on any connected monitor, so a window saved
    /// on a since-disconnected display doesn't reopen off-screen.
    pub(crate) position: Option<IVec2>,
    /// Start maximized. Restored alongside `inner_size` — winit applies
    /// the maximized state and holds `inner_size` as the size to return to
    /// when the user un-maximizes.
    pub(crate) maximized: bool,
    /// Title-bar / taskbar icon. `None` = platform default. Honored on
    /// Windows and Linux (X11/Wayland); **macOS ignores per-window icons**
    /// (its Dock icon comes from the `.app` bundle's `.icns`, set at
    /// packaging time). Backend-agnostic raw pixels — see [`WindowIcon`].
    pub(crate) icon: Option<WindowIcon>,
}

/// A window icon as straight-alpha **RGBA8** pixels: row-major, top row
/// first, exactly `width * height * 4` bytes. Backend-agnostic (carries no
/// winit type); [`WinitHost`](crate::WinitHost) converts it to the platform
/// icon at window creation. Decode a PNG (or any source) to RGBA on the app
/// side and hand the raw buffer here.
#[derive(Clone, Debug)]
pub struct WindowIcon {
    /// `width * height * 4` bytes, straight-alpha RGBA8, row-major.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl WindowIcon {
    /// Build from raw RGBA8. Panics if `rgba.len() != width * height * 4`
    /// — a length mismatch means the caller passed the wrong stride or
    /// dimensions, which is a logic bug, not a runtime condition.
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Self {
        assert_eq!(
            rgba.len(),
            width as usize * height as usize * 4,
            "WindowIcon requires width*height*4 RGBA8 bytes",
        );
        Self {
            rgba,
            width,
            height,
        }
    }
}

/// A window's live geometry, assembled on demand by
/// [`Ui::window_geometry`](crate::Ui::window_geometry) so the app can
/// persist and restore size / position across launches. A computed view,
/// not stored state: the size comes from the frame's `Display` (the single
/// source of truth for surface size), the position + maximized flag from
/// the host's window-manager facts. Backend-agnostic (no winit types),
/// matching [`WindowConfig`]'s vocabulary: logical size, physical position.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowGeometry {
    /// Inner (content) size in logical pixels — DPI-independent, so it
    /// round-trips through [`WindowConfig::inner_size()`] unchanged across
    /// monitors of different scale.
    pub inner_size: UVec2,
    /// Outer position in physical pixels, or `None` when the platform
    /// doesn't report it (Wayland clients can't know their absolute
    /// position). Feeds [`WindowConfig::position()`] on restore.
    pub outer_position: Option<IVec2>,
    /// Whether the window is currently maximized.
    pub maximized: bool,
}

impl WindowConfig {
    /// A config for a window titled `title`; every other option defaults
    /// (platform-picked size/position, not maximized, default icon). Chain
    /// the setters below to override.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Initial inner size in logical pixels (`.x` = width, `.y` = height).
    pub fn inner_size(mut self, size: UVec2) -> Self {
        self.inner_size = Some(size);
        self
    }

    /// Minimum inner size in logical pixels — the window can't shrink below
    /// it.
    pub fn min_inner_size(mut self, size: UVec2) -> Self {
        self.min_inner_size = Some(size);
        self
    }

    /// Initial outer position in physical pixels (top-left of the frame).
    /// Dropped at creation if it no longer lands on any connected monitor.
    pub fn position(mut self, position: IVec2) -> Self {
        self.position = Some(position);
        self
    }

    /// Start the window maximized (holding [`Self::inner_size`] as the
    /// un-maximize size).
    pub fn maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    /// Title-bar / taskbar icon (ignored on macOS — see [`WindowIcon`]).
    pub fn icon(mut self, icon: WindowIcon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// The mouse cursor a widget wants shown this frame, requested through
/// [`Ui::set_cursor`](crate::Ui::set_cursor). Backend-agnostic subset of
/// the platform cursors (the winit mapping lives in the host); grows
/// variants as widgets need them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    /// The platform arrow — what every frame resets to.
    #[default]
    Default,
    /// Clickable affordance (hand).
    Pointer,
    /// Text caret (I-beam).
    Text,
    /// Open hand: a grabbable surface.
    Grab,
    /// Closed hand: a grab in progress.
    Grabbing,
    Move,
    Crosshair,
    /// Horizontal resize (a vertical divider).
    EwResize,
    /// Vertical resize (a horizontal divider).
    NsResize,
    NotAllowed,
}

/// A window-open request enqueued by
/// [`Ui::open_window`](crate::Ui::open_window), drained by
/// [`WinitHost`](crate::WinitHost) in `about_to_wait` once it holds
/// `&ActiveEventLoop`.
#[derive(Debug)]
pub(crate) struct PendingWindow {
    pub(crate) token: WindowToken,
    pub(crate) config: WindowConfig,
}

/// Deferred window lifecycle commands transferred from recorders to the host.
#[derive(Debug, Default)]
pub(crate) struct WindowCommands {
    pub(crate) opens: Vec<PendingWindow>,
    pub(crate) closes: Vec<WindowToken>,
}

impl WindowCommands {
    pub(crate) fn append(&mut self, source: &mut Self) {
        self.opens.append(&mut source.opens);
        self.closes.append(&mut source.closes);
    }
}

/// Deferred recorder output consumed by the window host after a frame.
#[derive(Debug, Default)]
pub(crate) struct WindowRequests {
    pub(crate) commands: WindowCommands,
    /// Whether app code vetoed the current close request.
    pub(crate) close_vetoed: bool,
    /// Last cursor requested during a record pass; retained across PaintOnly.
    pub(crate) cursor: CursorIcon,
}

/// Host-owned facts copied into `Ui` at the start of a window frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WindowFrameState {
    pub(crate) close_requested: bool,
    pub(crate) position: Option<IVec2>,
    pub(crate) maximized: bool,
}

#[cfg(test)]
mod tests {
    use crate::window::{PendingWindow, WindowCommands, WindowConfig, WindowIcon, WindowToken};

    #[test]
    fn window_icon_from_rgba_keeps_pixels_and_dims() {
        // 2×1 image → 2*1*4 = 8 bytes: opaque red then opaque green.
        let px = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let icon = WindowIcon::from_rgba(px.clone(), 2, 1);
        assert_eq!(icon.width, 2);
        assert_eq!(icon.height, 1);
        assert_eq!(icon.rgba, px, "pixels pass through unchanged");
    }

    #[test]
    #[should_panic(expected = "width*height*4")]
    fn window_icon_rejects_wrong_length() {
        // 2×2 needs 16 bytes; 12 is a stride/dimension bug.
        WindowIcon::from_rgba(vec![0; 12], 2, 2);
    }

    #[test]
    fn window_config_default_has_no_icon() {
        assert!(WindowConfig::default().icon.is_none());
        assert!(WindowConfig::new("t").icon.is_none());
    }

    #[test]
    fn window_commands_append_preserves_order_and_drains_source() {
        let mut commands = WindowCommands {
            opens: vec![PendingWindow {
                token: WindowToken(1),
                config: WindowConfig::default(),
            }],
            closes: vec![WindowToken(4)],
        };
        let mut source = WindowCommands {
            opens: vec![
                PendingWindow {
                    token: WindowToken(2),
                    config: WindowConfig::default(),
                },
                PendingWindow {
                    token: WindowToken(3),
                    config: WindowConfig::default(),
                },
            ],
            closes: vec![WindowToken(5), WindowToken(6)],
        };
        let source_opens_capacity = source.opens.capacity();
        let source_closes_capacity = source.closes.capacity();

        commands.append(&mut source);

        assert_eq!(
            commands
                .opens
                .iter()
                .map(|pending| pending.token)
                .collect::<Vec<_>>(),
            [WindowToken(1), WindowToken(2), WindowToken(3)]
        );
        assert_eq!(
            commands.closes,
            [WindowToken(4), WindowToken(5), WindowToken(6)]
        );
        assert!(source.opens.is_empty());
        assert!(source.closes.is_empty());
        assert_eq!(source.opens.capacity(), source_opens_capacity);
        assert_eq!(source.closes.capacity(), source_closes_capacity);
    }
}
