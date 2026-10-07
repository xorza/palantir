//! Tracy instrumentation (zones and per-window frame sets) gated on `profile-with-tracy`; the only place that
//! names `tracy_client`.

/// Opens a zone for the rest of the enclosing block, named after the function or by `zone!("name")`; a
/// `value =`/`text =` payload is evaluated only in a profiling build.
macro_rules! zone {
    () => {
        #[cfg(feature = "profile-with-tracy")]
        let _zone = ::tracy_client::span!();
    };
    ($name:literal) => {
        #[cfg(feature = "profile-with-tracy")]
        let _zone = ::tracy_client::span!($name, 0);
    };
    ($name:literal, value = $value:expr) => {
        #[cfg(feature = "profile-with-tracy")]
        let _zone = {
            let zone = ::tracy_client::span!($name, 0);
            zone.emit_value($value);
            zone
        };
    };
    ($name:literal, text = $text:expr) => {
        #[cfg(feature = "profile-with-tracy")]
        let _zone = {
            let zone = ::tracy_client::span!($name, 0);
            zone.emit_text($text);
            zone
        };
    };
}

pub(crate) use zone;

/// Names for the per-window frame sets: a fixed table, as `FrameName` must be `'static`; windows past it share
/// the last entry.
#[cfg(all(feature = "profile-with-tracy", feature = "winit"))]
const NAMES: &[tracy_client::FrameName] = &[
    tracy_client::frame_name!("window 0"),
    tracy_client::frame_name!("window 1"),
    tracy_client::frame_name!("window 2"),
    tracy_client::frame_name!("window 3"),
    tracy_client::frame_name!("window 4"),
    tracy_client::frame_name!("window 5"),
    tracy_client::frame_name!("window 6"),
    tracy_client::frame_name!("window 7"),
    tracy_client::frame_name!("window 8+"),
];

/// One window's Tracy frame set: windows paint on independent schedules, so one shared set would report
/// per-window slices as whole frames. Zero-sized without the profiler.
#[cfg(feature = "winit")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameSet {
    #[cfg(feature = "profile-with-tracy")]
    index: usize,
}

#[cfg(feature = "winit")]
impl FrameSet {
    /// Claims the next set in window creation order; never reused, so a closed window's history stays its own.
    #[cfg_attr(
        not(feature = "profile-with-tracy"),
        expect(
            clippy::missing_const_for_fn,
            reason = "with `profile-with-tracy` the body calls the Tracy client"
        )
    )]
    pub(crate) fn claim() -> Self {
        FrameSet {
            #[cfg(feature = "profile-with-tracy")]
            index: {
                static CLAIMED: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                CLAIMED
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .min(NAMES.len() - 1)
            },
        }
    }

    #[cfg_attr(
        not(feature = "profile-with-tracy"),
        expect(
            clippy::missing_const_for_fn,
            reason = "with `profile-with-tracy` the body calls the Tracy client"
        )
    )]
    pub(crate) fn mark(self) {
        #[cfg(feature = "profile-with-tracy")]
        tracy_client::Client::running()
            .expect("secondary_frame_mark without a running Client")
            .secondary_frame_mark(NAMES[self.index]);
    }
}

/// Ends one frame in Tracy's *main* set (the FPS readout), a global timeline meaningful only while one window
/// owns the cadence; the winit host marks it in `WinitRuntime::draw`.
#[cfg(feature = "winit")]
#[cfg_attr(
    not(feature = "profile-with-tracy"),
    expect(
        clippy::missing_const_for_fn,
        reason = "with `profile-with-tracy` the body calls the Tracy client"
    )
)]
pub(crate) fn mark_main_frame() {
    #[cfg(feature = "profile-with-tracy")]
    tracy_client::frame_mark();
}
