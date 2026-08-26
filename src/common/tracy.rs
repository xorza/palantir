//! Tracy instrumentation: scoped zones, per-window frame sets, and the
//! `profile-with-tracy` gating that keeps both out of every other build.
//!
//! The only place in the crate that names `tracy_client`, so the
//! `#[cfg]`s live here instead of at every call site.

/// Open a zone covering the rest of the enclosing block.
///
/// `zone!()` takes its name from the enclosing function; `zone!("name")`
/// names it explicitly. A trailing `value =` or `text =` payload rides
/// along in Tracy's zone panel; only a profiling build evaluates it, so
/// a count that costs a `format!` still costs nothing here.
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

/// Names for the per-window frame sets.
///
/// A fixed table because [`tracy_client::FrameName`] must be `'static`
/// and `frame_name!` takes a literal. Windows past the table share the
/// last entry, which says so rather than silently continuing
/// `window 7`'s history.
#[cfg(feature = "profile-with-tracy")]
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

/// One window's Tracy frame set.
///
/// A set per window is the point: windows paint on independent
/// schedules — different monitors, different refresh rates, one idle
/// while another animates — so no single frame spans them, and marking
/// them into one set reports per-window slices as whole frames.
///
/// Zero-sized without the profiler, so a normal build carries no
/// per-window profiling state at all.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameSet {
    /// Index into [`NAMES`], clamped when claimed.
    #[cfg(feature = "profile-with-tracy")]
    index: usize,
}

impl FrameSet {
    /// Claim the next set, in window creation order. Never reused, so a
    /// closed window's frame history stays its own instead of a later
    /// window continuing it.
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

    /// End one frame in this window's set.
    pub(crate) fn mark(self) {
        #[cfg(feature = "profile-with-tracy")]
        tracy_client::Client::running()
            .expect("secondary_frame_mark without a running Client")
            .secondary_frame_mark(NAMES[self.index]);
    }
}

/// End one frame in Tracy's *main* set — the one behind the FPS readout.
///
/// That set is a single global timeline, so it means something only
/// while one window owns the cadence — the caller decides when that
/// holds. The winit host marks it in `WinitRuntime::draw`, where the
/// live window count is already known.
pub(crate) fn mark_main_frame() {
    #[cfg(feature = "profile-with-tracy")]
    tracy_client::frame_mark();
}
