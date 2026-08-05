//! Tracy frame sets, and the `profile-with-tracy` gating that keeps them
//! out of every other build.
//!
//! The `profiling` facade this crate uses everywhere else covers zones
//! but models only one frame set — `finish_frame!` marks the main one
//! and has no secondary equivalent. Frame *sets* are the reason
//! `tracy-client` is also a direct dependency, and this module is the
//! only place that names it, so the `#[cfg]`s live here rather than at
//! the call sites.

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
        if let Some(client) = tracy_client::Client::running() {
            client.secondary_frame_mark(NAMES[self.index]);
        }
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
    if let Some(client) = tracy_client::Client::running() {
        client.frame_mark();
    }
}
