//! `cargo atlas-memory`: what each atlas holds over the bench scenes, each arm of
//! `integration/frame` over the scene and then the backdrops' looks and scenes, a host of its own
//! each. Per run, each atlas's peak bytes over its frames, then at its last frame its bytes, the
//! bytes its live items fill and their share, and the same for the total, whose peak is the most
//! one frame held. Under `cargo test`, which passes no `--bench`, two frames a run, as criterion
//! runs each case once there.

use crate::bench::backends::Backends;
use crate::bench::scene::Scene;
use crate::gpu::atlas_memory::{AtlasMemory, AtlasUse, Bytes};
use crate::gpu::backdrop;
use crate::gpu::bench_gpu::Timing;
use crate::host::offscreen::OffscreenHost;
use crate::ui;
use std::io::{self, Write};
use std::process::ExitCode;

/// Frames of a run under `cargo atlas-memory`: four seconds at 60 Hz, past each arm's settling.
const FRAMES: u32 = 240;

/// Frames of a run under `cargo test`.
const TEST_FRAMES: u32 = 2;

/// Every run over `S`, as `args`, the command line past the study's name, asks: `--bench`, which
/// `cargo bench` passes, runs [`FRAMES`] a run, else [`TEST_FRAMES`].
pub(crate) fn study<S: Scene>(args: &[String]) -> ExitCode {
    let timed = args.iter().any(|arg| arg == "--bench");
    let frames = if timed { FRAMES } else { TEST_FRAMES };
    let backends = Backends::probe();
    if backends.gpu().is_none() {
        return ExitCode::SUCCESS;
    }
    let gpu = backends.device(Timing::PassOnly);
    let mut report = Report::new(io::stdout().lock());
    ui::bench::drive_arms::<S>(gpu, frames, |name, host| report.visit("frame", name, host));
    backdrop::bench::drive::<S>(gpu, frames, |name, host| {
        report.visit("backdrop", name, host);
    });
    match report.finish() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_error(&error),
    }
}

/// Print `error` where a person reads it.
#[expect(
    clippy::print_stderr,
    reason = "a study that cannot write its report says why where a person reads it"
)]
fn report_error(error: &io::Error) -> ExitCode {
    eprintln!("atlas memory: {error}");
    ExitCode::FAILURE
}

/// The runs seen so far, each printed once the next begins.
#[derive(Debug)]
struct Report<W> {
    out: W,
    run: Option<Run>,
    /// The first write that failed; the rest of the study writes nothing more.
    error: Option<io::Error>,
}

/// One run's frames.
#[derive(Debug)]
struct Run {
    group: &'static str,
    name: String,
    frames: u32,
    /// The most each atlas held at the end of a frame.
    peak: [u64; AtlasMemory::LEN],
    /// The most every atlas held together at the end of one frame.
    peak_total: u64,
    last: AtlasMemory,
}

impl<W: Write> Report<W> {
    const fn new(out: W) -> Self {
        Self {
            out,
            run: None,
            error: None,
        }
    }

    /// The memory `host` holds after a frame of the run `group/name`.
    fn visit(&mut self, group: &'static str, name: &str, host: &OffscreenHost) {
        let memory = host.atlas_memory();
        let continues = self
            .run
            .as_ref()
            .is_some_and(|run| run.group == group && run.name == name);
        if !continues {
            self.flush();
            self.run = Some(Run {
                group,
                name: name.to_owned(),
                frames: 0,
                peak: [0; AtlasMemory::LEN],
                peak_total: 0,
                last: memory,
            });
        }
        let run = self.run.as_mut().expect("a run was just opened");
        run.frames += 1;
        for (peak, used) in run.peak.iter_mut().zip(&memory.uses) {
            *peak = (*peak).max(used.bytes);
        }
        run.peak_total = run.peak_total.max(total(&memory.uses).bytes);
        run.last = memory;
    }

    /// Print the open run, if any.
    fn flush(&mut self) {
        let Some(run) = self.run.take() else {
            return;
        };
        if self.error.is_none()
            && let Err(error) = write_run(&mut self.out, &run)
        {
            self.error = Some(error);
        }
    }

    /// Print the last run, then report the first write that failed.
    fn finish(&mut self) -> io::Result<()> {
        self.flush();
        self.error.take().map_or(Ok(()), Err)
    }
}

/// The row that sums `uses`: their textures and bytes, and the live bytes of those that hold
/// items.
fn total(uses: &[AtlasUse]) -> AtlasUse {
    AtlasUse {
        name: "total",
        textures: uses.iter().map(|used| used.textures).sum(),
        bytes: uses.iter().map(|used| used.bytes).sum(),
        live_bytes: Some(uses.iter().filter_map(|used| used.live_bytes).sum()),
    }
}

/// One run's table: an atlas a row, then the total.
fn write_run(out: &mut impl Write, run: &Run) -> io::Result<()> {
    writeln!(
        out,
        "\n== {}/{} ({} frames) ==",
        run.group, run.name, run.frames
    )?;
    writeln!(
        out,
        "{:<22} {:>8} {:>11} {:>11} {:>11} {:>7}",
        "atlas", "textures", "peak", "bytes", "live", "share"
    )?;
    for (used, &peak) in run.last.uses.iter().zip(&run.peak) {
        write_row(out, used, peak)?;
    }
    write_row(out, &total(&run.last.uses), run.peak_total)
}

/// One row: `used` at the run's last frame, after the most it held, `peak`.
fn write_row(out: &mut impl Write, used: &AtlasUse, peak: u64) -> io::Result<()> {
    let live = used
        .live_bytes
        .map_or_else(|| "—".to_owned(), |live| Bytes(live).to_string());
    writeln!(
        out,
        "{:<22} {:>8} {:>11} {:>11} {:>11} {:>7}",
        used.name,
        used.textures,
        Bytes(peak).to_string(),
        Bytes(used.bytes).to_string(),
        live,
        share(used)
    )
}

/// The share of `used`'s bytes its live items fill, to a tenth of a percent; `—` where it holds
/// no texture or no items, and `<0.1 %` where a nonzero share rounds to none.
#[expect(
    clippy::cast_precision_loss,
    reason = "a share prints to a tenth of a percent"
)]
fn share(used: &AtlasUse) -> String {
    match used.live_bytes {
        Some(live) if used.bytes > 0 => {
            let percent = live as f64 * 100.0 / used.bytes as f64;
            if live > 0 && percent < 0.05 {
                "<0.1 %".to_owned()
            } else {
                format!("{percent:.1} %")
            }
        }
        _ => "—".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::atlas_memory::AtlasUse;
    use crate::gpu::atlas_memory::bench::{share, total};

    fn used(bytes: u64, live_bytes: Option<u64>) -> AtlasUse {
        AtlasUse {
            name: "test",
            textures: 1,
            bytes,
            live_bytes,
        }
    }

    /// A share is live over bytes to a tenth: 1 of 4 MiB reads `25.0 %`; one byte of 4 MiB,
    /// 2.4e-5 %, reads `<0.1 %` rather than `0.0 %`; none of it `0.0 %`; no texture, or scratch
    /// with no items, `—`.
    #[test]
    fn a_share_never_reads_a_nonzero_as_zero() {
        assert_eq!(share(&used(4 << 20, Some(1 << 20))), "25.0 %");
        assert_eq!(share(&used(4 << 20, Some(1))), "<0.1 %");
        assert_eq!(share(&used(4 << 20, Some(0))), "0.0 %");
        assert_eq!(share(&used(0, Some(0))), "—");
        assert_eq!(share(&used(4 << 20, None)), "—");
    }

    /// The total sums textures and bytes over every row, and live bytes over the rows that hold
    /// items: 1 + 2 textures, 10 + 20 bytes, 3 live from the one row that reports any.
    #[test]
    fn the_total_sums_the_rows() {
        let rows = [
            used(10, Some(3)),
            AtlasUse {
                textures: 2,
                ..used(20, None)
            },
        ];
        let sum = total(&rows);
        assert_eq!((sum.textures, sum.bytes, sum.live_bytes), (3, 30, Some(3)));
    }
}
