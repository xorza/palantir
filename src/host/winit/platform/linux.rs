//! The windowed host on Linux and the BSDs, under Wayland or X11.

use std::collections::HashMap;
use std::future;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::platform::wayland::WindowAttributesExtWayland as _;
use winit::window::WindowAttributes;
use zbus::export::futures_core::Stream;
use zbus::proxy::{Builder, CacheProperties};
use zbus::zvariant::OwnedValue;
use zbus::{Connection, Proxy};

use crate::common::executor;
use crate::host::winit::input::wheel_steps::WheelSteps;
use crate::host::winit::platform::stop::Stop;
use crate::primitives::reduction::Reduction;

mod stop;

/// Nothing to configure.
pub(super) const fn configure_event_loop<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}

/// Applies [`WindowConfig::with_app_id`](crate::WindowConfig::with_app_id). Wayland's `app_id` and
/// X11's `WM_CLASS` are one winit attribute; the instance name repeats the general one. Wayland has
/// no fallback, so an unnamed window cannot match its `.desktop` entry.
pub(super) fn with_app_id(attrs: WindowAttributes, app_id: Option<&str>) -> WindowAttributes {
    match app_id {
        Some(app_id) => attrs.with_name(app_id, app_id),
        None => attrs,
    }
}

/// GNOME's and KDE's default double-click time, 400 ms, kept where the portal gives none. GNOME's
/// setting reads through the portal's `org.gnome.desktop.peripherals.mouse double-click`; KDE keeps
/// its own in `kdeglobals`, which no portal key holds, so a KDE desktop answers the GSettings value
/// (`.notes/research/2026-10-31-system-preferences.md`).
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = Some(Duration::from_millis(400));

/// One line per notch: neither the X11 nor the Wayland protocol states a scroll setting.
pub(super) fn wheel_steps() -> WheelSteps {
    WheelSteps::default()
}

/// The portal that reports the power-saver profile, as GLib's `GPowerProfileMonitor` reads it
/// (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.PowerProfileMonitor.html>).
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const POWER_PROFILE_MONITOR: &str = "org.freedesktop.portal.PowerProfileMonitor";
const POWER_SAVER_ENABLED: &str = "power-saver-enabled";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Follows the power-saver profile through the portal's `power-saver-enabled`, on a
/// [`PortalThread`]. The thread reads the first value itself, as GLib's portal monitor once did not
/// and missed a mode set before the app started (<https://gitlab.gnome.org/GNOME/glib/-/issues/2481>).
/// A desktop with no session bus or no such portal reports nothing, and power stays full. Refused:
/// `ashpd`, a package for one boolean; GLib's split, power-profiles-daemon's `ActiveProfile` on
/// the system bus outside a sandbox and the portal inside one
/// (<https://gitlab.gnome.org/GNOME/glib/-/blob/main/gio/gpowerprofilemonitordbus.c>), two routes
/// where `mundy` reads every other setting through the portal alone.
#[derive(Debug)]
pub(super) struct PowerSaverWatch {
    _thread: PortalThread,
}

impl PowerSaverWatch {
    /// Follow the profile: `on_change` runs on the watch's thread with it once the portal answers,
    /// then with each change.
    pub(super) fn follow(on_change: impl Fn(Reduction) + Send + Sync + 'static) -> Self {
        Self {
            _thread: PortalThread::spawn("palantir-power-saver", async move |connection| {
                follow_power_profile(connection, &on_change).await
            }),
        }
    }
}

/// The portal that reports low memory, as GLib's `GMemoryMonitor` reads it
/// (<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.MemoryMonitor.html>).
const MEMORY_MONITOR: &str = "org.freedesktop.portal.MemoryMonitor";
const LOW_MEMORY_WARNING: &str = "LowMemoryWarning";

/// Follows the system's memory pressure through the portal's `LowMemoryWarning`, on a
/// [`PortalThread`]: each signal, at any of its levels (50 low, 100 medium, 255 critical), reports
/// a warning, as GLib's `GMemoryMonitor` documents freeing caches at each. A desktop with no
/// session bus or no such portal reports nothing, which leaves the pool to its own decay.
#[derive(Debug)]
pub(super) struct MemoryPressureWatch {
    _thread: PortalThread,
}

impl MemoryPressureWatch {
    /// Follow the pressure: `on_warning` runs on the watch's thread at each warning.
    pub(super) fn follow(on_warning: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            _thread: PortalThread::spawn("palantir-memory-pressure", async move |connection| {
                let warnings = portal_proxy(connection, MEMORY_MONITOR)
                    .await?
                    .receive_signal(LOW_MEMORY_WARNING)
                    .await?;
                let mut warnings = pin!(warnings);
                while next(warnings.as_mut()).await.is_some() {
                    on_warning();
                }
                Ok(())
            }),
        }
    }
}

/// A thread that follows a portal over a session-bus connection of its own until its owner's drop
/// raises a [`Stop`]. The stop drops the follow unfinished while the connection is still open:
/// each signal stream's drop queues its `RemoveMatch` on the connection, which stays open until
/// that is sent. Refused: closing the connection under the thread, which ends a stream but sends
/// its `RemoveMatch` to the closed socket, a warning per subscription.
#[derive(Debug)]
struct PortalThread {
    stop: Arc<Stop>,
}

impl PortalThread {
    /// Run `follow` over a new session-bus connection on a thread named `name`, until the drop;
    /// a failure to open the bus or in `follow` ends the thread with a note.
    fn spawn(
        name: &str,
        follow: impl AsyncFnOnce(&Connection) -> zbus::Result<()> + Send + 'static,
    ) -> Self {
        let stop = Arc::new(Stop::default());
        let held = Arc::clone(&stop);
        let spawned = thread::Builder::new().name(name.to_owned()).spawn(move || {
            let work = async {
                let connection = Connection::session().await?;
                follow(&connection).await
            };
            if let Some(Err(error)) = executor::block_on(held.until(work)) {
                log::debug!("a portal is not followed: {error}");
            }
        });
        if let Err(error) = spawned {
            log::warn!("no thread follows the portal {name}: {error}");
        }
        Self { stop }
    }
}

impl Drop for PortalThread {
    fn drop(&mut self) {
        self.stop.raise();
    }
}

/// A proxy of the portal's `interface` over `connection`, reading no property from a cache.
async fn portal_proxy<'c>(
    connection: &'c Connection,
    interface: &'static str,
) -> zbus::Result<Proxy<'c>> {
    Builder::<Proxy<'_>>::new(connection)
        .destination(PORTAL)?
        .path(PORTAL_PATH)?
        .interface(interface)?
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

/// The next item of `stream`, `None` once it ends.
async fn next<S: Stream>(mut stream: Pin<&mut S>) -> Option<S::Item> {
    future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
}

/// Report the profile and each change until the bus closes the connection or the owner stops the
/// thread. Subscribed to `PropertiesChanged` before the first read, so no change falls between the
/// two. Refused: zbus's property stream, which reads through the proxy's property cache.
async fn follow_power_profile(
    connection: &Connection,
    on_change: &impl Fn(Reduction),
) -> zbus::Result<()> {
    let monitor = portal_proxy(connection, POWER_PROFILE_MONITOR).await?;
    let changes = portal_proxy(connection, PROPERTIES)
        .await?
        .receive_signal("PropertiesChanged")
        .await?;
    let mut changes = pin!(changes);
    let read = async || {
        monitor
            .get_property(POWER_SAVER_ENABLED)
            .await
            .map(Reduction::of)
    };
    on_change(read().await?);
    while let Some(message) = next(changes.as_mut()).await {
        let change = message.body().deserialize::<PropertiesChanged>().and_then(
            |(interface, changed, invalidated)| portal_change(&interface, &changed, &invalidated),
        );
        // A malformed signal or a failed read skips one report, as GLib's monitor ignores a value
        // of the wrong type, and the next change reports again.
        let power = match change {
            Ok(PortalChange::Value(power)) => Ok(power),
            Ok(PortalChange::Invalidated) => read().await,
            Ok(PortalChange::Unrelated) => continue,
            Err(error) => Err(error),
        };
        match power {
            Ok(power) => on_change(power),
            Err(error) => log::debug!("a power-saver change is not read: {error}"),
        }
    }
    Ok(())
}

/// The body of `org.freedesktop.DBus.Properties.PropertiesChanged`: the interface, the changed
/// properties with their values, and the invalidated ones by name.
type PropertiesChanged = (String, HashMap<String, OwnedValue>, Vec<String>);

/// What one `PropertiesChanged` signal says of `power-saver-enabled`.
#[derive(Debug, PartialEq, Eq)]
enum PortalChange {
    /// The signal carries the new value.
    Value(Reduction),
    /// The signal names the property without a value, which a read gets.
    Invalidated,
    /// Another interface or another property changed.
    Unrelated,
}

fn portal_change(
    interface: &str,
    changed: &HashMap<String, OwnedValue>,
    invalidated: &[String],
) -> zbus::Result<PortalChange> {
    if interface != POWER_PROFILE_MONITOR {
        return Ok(PortalChange::Unrelated);
    }
    if let Some(value) = changed.get(POWER_SAVER_ENABLED) {
        return Ok(PortalChange::Value(Reduction::of(bool::try_from(value)?)));
    }
    Ok(
        if invalidated.iter().any(|name| name == POWER_SAVER_ENABLED) {
            PortalChange::Invalidated
        } else {
            PortalChange::Unrelated
        },
    )
}

#[cfg(test)]
pub(super) mod internals {
    use winit::event_loop::EventLoopBuilder;
    use winit::platform::wayland::EventLoopBuilderExtWayland;
    use winit::platform::x11::EventLoopBuilderExtX11;

    /// Let a test, which runs off the main thread, build an event loop, under Wayland or X11.
    pub(crate) fn any_thread<T: 'static>(builder: &mut EventLoopBuilder<T>) {
        EventLoopBuilderExtWayland::with_any_thread(builder, true);
        EventLoopBuilderExtX11::with_any_thread(builder, true);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use zbus::zvariant::{OwnedValue, Str};

    use crate::host::winit::platform::{
        POWER_PROFILE_MONITOR, POWER_SAVER_ENABLED, PortalChange, portal_change,
    };
    use crate::primitives::reduction::Reduction;

    /// A signal with the value reports it, on and off; one that only invalidates the property asks
    /// for a read; another interface or another property is not the mode's; a value of the wrong
    /// type is an error.
    #[test]
    fn a_properties_changed_signal_maps_to_the_mode() {
        let changed = |value: OwnedValue| HashMap::from([(POWER_SAVER_ENABLED.to_owned(), value)]);
        let invalidated = [POWER_SAVER_ENABLED.to_owned()];
        let other = ["other".to_owned()];
        let table = [
            (
                POWER_PROFILE_MONITOR,
                changed(OwnedValue::from(true)),
                &[][..],
                PortalChange::Value(Reduction::Reduced),
            ),
            (
                POWER_PROFILE_MONITOR,
                changed(OwnedValue::from(false)),
                &[][..],
                PortalChange::Value(Reduction::Unreduced),
            ),
            (
                POWER_PROFILE_MONITOR,
                HashMap::new(),
                &invalidated[..],
                PortalChange::Invalidated,
            ),
            (
                POWER_PROFILE_MONITOR,
                HashMap::new(),
                &other[..],
                PortalChange::Unrelated,
            ),
            (
                "org.freedesktop.portal.Settings",
                changed(OwnedValue::from(true)),
                &invalidated[..],
                PortalChange::Unrelated,
            ),
        ];
        for (interface, changed, invalidated, want) in table {
            assert_eq!(
                portal_change(interface, &changed, invalidated).unwrap(),
                want,
                "{interface}, {changed:?}, {invalidated:?}"
            );
        }
        let text = changed(OwnedValue::from(Str::from_static("on")));
        assert!(portal_change(POWER_PROFILE_MONITOR, &text, &[]).is_err());
    }
}
