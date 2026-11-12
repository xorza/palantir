//! The windowed host on macOS.

use std::ffi::c_void;
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

/// Follows the system's memory pressure through libdispatch's memory-pressure source, as
/// Chromium's macOS memory-pressure monitor does
/// (<https://developer.apple.com/documentation/dispatch/dispatch_source_type_memorypressure>):
/// a warning or a critical level reports a warning. The source runs its handler on a global
/// queue, through libdispatch's C API with a function and a context, no block, so no crate past
/// the system's; the watch's drop cancels the source, whose cancel handler frees the context once
/// no handler runs.
#[derive(Debug)]
pub(super) struct MemoryPressureWatch {
    source: DispatchSource,
}

/// libdispatch's opaque object, a source here.
#[repr(C)]
struct DispatchObject {
    _opaque: [u8; 0],
}

/// A `dispatch_source_t`.
#[derive(Debug)]
struct DispatchSource(*mut DispatchObject);

// SAFETY: libdispatch objects are thread-safe; the watch only cancels and releases it.
unsafe impl Send for DispatchSource {}

/// The context the source's handlers read: what a warning runs.
type OnWarning = Box<dyn Fn() + Send + Sync>;

/// `DISPATCH_MEMORYPRESSURE_WARN` and `DISPATCH_MEMORYPRESSURE_CRITICAL`, `<dispatch/source.h>`.
const MEMORY_PRESSURE_WARN: usize = 0x02;
const MEMORY_PRESSURE_CRITICAL: usize = 0x04;

/// `QOS_CLASS_UTILITY`, `<sys/qos.h>`: a warning is housekeeping, not what a person waits on.
const QOS_CLASS_UTILITY: isize = 0x11;

unsafe extern "C" {
    static _dispatch_source_type_memorypressure: u8;
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut DispatchObject;
    fn dispatch_source_create(
        kind: *const u8,
        handle: usize,
        mask: usize,
        queue: *mut DispatchObject,
    ) -> *mut DispatchObject;
    fn dispatch_set_context(object: *mut DispatchObject, context: *mut c_void);
    fn dispatch_source_set_event_handler_f(
        source: *mut DispatchObject,
        handler: unsafe extern "C" fn(*mut c_void),
    );
    fn dispatch_source_set_cancel_handler_f(
        source: *mut DispatchObject,
        handler: unsafe extern "C" fn(*mut c_void),
    );
    fn dispatch_resume(object: *mut DispatchObject);
    fn dispatch_source_cancel(source: *mut DispatchObject);
    fn dispatch_release(object: *mut DispatchObject);
}

/// The source's event handler: a warning at either level the mask lets through.
unsafe extern "C" fn on_pressure(context: *mut c_void) {
    // SAFETY: the context is the watch's `OnWarning`, which the cancel handler frees only after
    // every event handler has returned.
    let on_warning = unsafe { &*context.cast::<OnWarning>() };
    on_warning();
}

/// The source's cancel handler, which libdispatch runs once, after the last event handler.
unsafe extern "C" fn on_cancel(context: *mut c_void) {
    // SAFETY: the context is the `OnWarning` the watch boxed and leaked, freed here alone.
    drop(unsafe { Box::from_raw(context.cast::<OnWarning>()) });
}

impl MemoryPressureWatch {
    /// Follow the pressure: `on_warning` runs on a libdispatch queue at each warning.
    pub(super) fn follow(on_warning: impl Fn() + Send + Sync + 'static) -> Self {
        let context: Box<OnWarning> = Box::new(Box::new(on_warning));
        // SAFETY: each call follows `<dispatch/source.h>`: a global queue, which is never
        // released; a source of the memory-pressure type over it, its context and both handlers
        // set before its first resume; the context lives until the cancel handler frees it.
        let source = unsafe {
            let queue = dispatch_get_global_queue(QOS_CLASS_UTILITY, 0);
            let source = dispatch_source_create(
                &raw const _dispatch_source_type_memorypressure,
                0,
                MEMORY_PRESSURE_WARN | MEMORY_PRESSURE_CRITICAL,
                queue,
            );
            assert!(
                !source.is_null(),
                "libdispatch made no memory-pressure source"
            );
            dispatch_set_context(source, Box::into_raw(context).cast());
            dispatch_source_set_event_handler_f(source, on_pressure);
            dispatch_source_set_cancel_handler_f(source, on_cancel);
            dispatch_resume(source);
            source
        };
        Self {
            source: DispatchSource(source),
        }
    }
}

impl Drop for MemoryPressureWatch {
    fn drop(&mut self) {
        // SAFETY: the source was resumed in `follow`; a cancel lets its handler finish and then
        // runs the cancel handler, which frees the context, and the release drops the watch's
        // reference, which libdispatch holds past it while a handler is queued.
        unsafe {
            dispatch_source_cancel(self.source.0);
            dispatch_release(self.source.0);
        }
    }
}

#[cfg(test)]
pub(super) mod internals {
    use winit::event_loop::EventLoopBuilder;

    /// Nothing to allow: AppKit runs an event loop on the main thread alone, so a test that
    /// builds one panics off it.
    pub(crate) const fn any_thread<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::host::winit::platform::MemoryPressureWatch;

    /// A watch makes its source and, dropped, cancels it: the cancel handler frees the context, so
    /// the closure's share of what it holds goes back, within a second of libdispatch's queue.
    #[test]
    fn a_dropped_watch_frees_its_handler() {
        let held = Arc::new(());
        let shared = Arc::clone(&held);
        let watch = MemoryPressureWatch::follow(move || {
            let _ = &shared;
        });
        assert_eq!(Arc::strong_count(&held), 2);
        drop(watch);
        let deadline = Instant::now() + Duration::from_secs(1);
        while Arc::strong_count(&held) > 1 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            Arc::strong_count(&held),
            1,
            "the cancel handler freed the context"
        );
    }
}
