//! The windowed host on Linux and the BSDs, under Wayland or X11.

use std::collections::HashMap;
use std::mem;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use winit::event_loop::EventLoopBuilder;
use winit::platform::wayland::WindowAttributesExtWayland as _;
use winit::window::WindowAttributes;
use zbus::blocking::proxy::Builder;
use zbus::blocking::{Connection, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedValue;

use crate::common::sync::MutexExt as _;
use crate::host::winit::input::wheel_steps::WheelSteps;
use crate::primitives::reduction::Reduction;

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

/// Follows the power-saver profile through the portal's `power-saver-enabled`, on a thread of its
/// own over a session-bus connection the watch closes when it drops, which ends the thread. The
/// thread reads the first value itself, as GLib's portal monitor once did not and missed a mode
/// set before the app started (<https://gitlab.gnome.org/GNOME/glib/-/issues/2481>). A desktop
/// with no session bus or no such portal reports nothing, and power stays full. Refused: `ashpd`,
/// a package for one boolean; GLib's split, power-profiles-daemon's `ActiveProfile` on the system
/// bus outside a sandbox and the portal inside one
/// (<https://gitlab.gnome.org/GNOME/glib/-/blob/main/gio/gpowerprofilemonitordbus.c>), two routes
/// where `mundy` reads every other setting through the portal alone.
#[derive(Debug)]
pub(super) struct PowerSaverWatch {
    connection: Arc<Mutex<WatchedBus>>,
}

/// The watch's session-bus connection, as its thread opens it and the watch's drop closes it.
#[derive(Debug)]
enum WatchedBus {
    Opening,
    Open(Connection),
    Closed,
}

impl PowerSaverWatch {
    /// Follow the profile: `on_change` runs on the watch's thread with it once the portal answers,
    /// then with each change.
    pub(super) fn follow(on_change: impl Fn(Reduction) + Send + Sync + 'static) -> Self {
        let connection = Arc::new(Mutex::new(WatchedBus::Opening));
        let held = Arc::clone(&connection);
        let spawned = thread::Builder::new()
            .name("palantir-power-saver".to_owned())
            .spawn(move || {
                if let Err(error) = follow_portal(&held, &on_change) {
                    tracing::debug!(%error, "the power-saver profile is not followed");
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "no thread follows the power-saver profile");
        }
        Self { connection }
    }
}

impl Drop for PowerSaverWatch {
    fn drop(&mut self) {
        let held = mem::replace(&mut *self.connection.lock_unpoisoned(), WatchedBus::Closed);
        if let WatchedBus::Open(connection) = held {
            connection.close().ok();
        }
    }
}

/// Report the profile and each change until the connection closes. Subscribed to
/// `PropertiesChanged` before the first read, so no change falls between the two. The signals come
/// off the connection itself, which its close ends. Refused: zbus's property stream, which waits
/// on the proxy's cache and outlives a close.
fn follow_portal(held: &Mutex<WatchedBus>, on_change: &impl Fn(Reduction)) -> zbus::Result<()> {
    let connection = Connection::session()?;
    {
        let mut held = held.lock_unpoisoned();
        if matches!(*held, WatchedBus::Closed) {
            return Ok(());
        }
        *held = WatchedBus::Open(connection.clone());
    }
    let proxy = |interface| {
        Builder::<Proxy<'_>>::new(&connection)
            .destination(PORTAL)?
            .path(PORTAL_PATH)?
            .interface(interface)?
            .cache_properties(CacheProperties::No)
            .build()
    };
    let monitor = proxy(POWER_PROFILE_MONITOR)?;
    let changes = proxy(PROPERTIES)?.receive_signal("PropertiesChanged")?;
    let read = || monitor.get_property(POWER_SAVER_ENABLED).map(Reduction::of);
    on_change(read()?);
    for message in changes {
        let change = message.body().deserialize::<PropertiesChanged>().and_then(
            |(interface, changed, invalidated)| portal_change(&interface, &changed, &invalidated),
        );
        // A malformed signal or a failed read skips one report, as GLib's monitor ignores a value
        // of the wrong type, and the next change reports again.
        let power = match change {
            Ok(PortalChange::Value(power)) => Ok(power),
            Ok(PortalChange::Invalidated) => read(),
            Ok(PortalChange::Unrelated) => continue,
            Err(error) => Err(error),
        };
        match power {
            Ok(power) => on_change(power),
            Err(error) => tracing::debug!(%error, "a power-saver change is not read"),
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
                PortalChange::Value(Reduction::Full),
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
