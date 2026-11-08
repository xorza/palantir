//! The windowed host on macOS.

use std::fmt;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::{AllocAnyThread as _, DeclaredClass as _, define_class, msg_send, sel};
use objc2_foundation::{
    NSNotificationCenter, NSObject, NSProcessInfo, NSProcessInfoPowerStateDidChangeNotification,
};
use winit::event_loop::EventLoopBuilder;
use winit::platform::macos::EventLoopBuilderExtMacOS;
use winit::window::WindowAttributes;

use crate::host::winit::input::wheel_steps::WheelSteps;
use crate::primitives::reduction::Reduction;

/// Drops winit's default menu, which binds ⌘Q to `terminate:` and kills the process before the app
/// can veto `CloseRequested`; ⌘Q then arrives as an ordinary key event.
pub(super) fn configure_event_loop<T: 'static>(builder: &mut EventLoopBuilder<T>) {
    builder.with_default_menu(false);
}

/// Nothing to apply: the application's identity comes from its `.app` bundle.
pub(super) const fn with_app_id(
    attrs: WindowAttributes,
    _app_id: Option<&str>,
) -> WindowAttributes {
    attrs
}

/// macOS always holds a double-click time, so none is kept here.
pub(super) const DOUBLE_CLICK_FALLBACK: Option<Duration> = None;

/// One line per notch: AppKit applies the person's scroll speed before the event arrives.
pub(super) fn wheel_steps() -> WheelSteps {
    WheelSteps::default()
}

/// Follows Low Power Mode: `NSProcessInfo.isLowPowerModeEnabled`, read again at each
/// `NSProcessInfoPowerStateDidChangeNotification`, as Apple's documentation of the property asks
/// (<https://developer.apple.com/documentation/foundation/processinfo/islowpowermodeenabled>).
/// The observer joins the notification center on a thread of its own, as the first observer of
/// that notification took 218 ms on an Apple laptop, a delay the window's first frame would wait
/// out. It leaves the center when the watch drops.
#[derive(Debug)]
pub(super) struct PowerSaverWatch {
    observer: Retained<PowerSaverObserver>,
    /// The thread that adds the observer and reads the first mode; `None` where no thread
    /// started, which leaves power full.
    joining: Option<JoinHandle<()>>,
}

impl PowerSaverWatch {
    /// Follow the mode: `on_change` runs with it once the observer has joined the center, then on
    /// the thread that posts each change. The observer joins before the first read, so no change
    /// falls between the two.
    pub(super) fn follow(on_change: impl Fn(Reduction) + Send + Sync + 'static) -> Self {
        let observer = PowerSaverObserver::alloc().set_ivars(ObserverIvars {
            on_change: Box::new(on_change),
        });
        // SAFETY: the instance is allocated and its ivars are set.
        let observer: Retained<PowerSaverObserver> = unsafe { msg_send![super(observer), init] };
        let joining = Retained::clone(&observer);
        let joining = thread::Builder::new()
            .name("palantir-power-saver".to_owned())
            .spawn(move || {
                // SAFETY: the observer answers the selector, and leaves the center before the
                // watch drops its share (`Drop`).
                unsafe {
                    NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                        &joining,
                        sel!(powerStateDidChange),
                        Some(NSProcessInfoPowerStateDidChangeNotification),
                        None,
                    );
                }
                joining.report();
            })
            .inspect_err(|error| tracing::warn!(%error, "no thread follows Low Power Mode"))
            .ok();
        Self { observer, joining }
    }
}

impl Drop for PowerSaverWatch {
    fn drop(&mut self) {
        // After the join, so the observer cannot join the center after it left.
        if let Some(joining) = self.joining.take() {
            joining.join().ok();
        }
        // SAFETY: the observer joined this center in `follow`, or never did, which removing
        // allows.
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver(&self.observer);
        }
    }
}

struct ObserverIvars {
    on_change: Box<dyn Fn(Reduction) + Send + Sync>,
}

impl fmt::Debug for ObserverIvars {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObserverIvars").finish_non_exhaustive()
    }
}

// SAFETY: the class holds no state but its ivars, which are `Send` and `Sync`, and the
// notification center calls it from any thread.
unsafe impl Send for PowerSaverObserver {}
// SAFETY: as for `Send`.
unsafe impl Sync for PowerSaverObserver {}

define_class! {
    // SAFETY: `NSObject` has no subclassing requirements, and the class implements no `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "PalantirPowerSaverObserver"]
    #[ivars = ObserverIvars]
    #[derive(Debug)]
    struct PowerSaverObserver;

    impl PowerSaverObserver {
        #[unsafe(method(powerStateDidChange))]
        fn power_state_did_change(&self) {
            self.report();
        }
    }
}

impl PowerSaverObserver {
    fn report(&self) {
        let low_power = NSProcessInfo::processInfo().isLowPowerModeEnabled();
        (self.ivars().on_change)(Reduction::of(low_power));
    }
}

#[cfg(test)]
pub(super) mod internals {
    use winit::event_loop::EventLoopBuilder;

    /// Nothing to allow: AppKit runs an event loop on the main thread alone, so a test that
    /// builds one panics off it.
    pub(crate) const fn any_thread<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}
}
