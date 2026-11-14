//! The palantir reel: a motion-graphics tour of its features, every frame drawn by palantir on a
//! headless window and streamed to ffmpeg.
//!
//! ```text
//! reel preview [--out FILE]        1280 × 720 at 60 fps, the preview
//! reel final [--out-dir DIR]       4K at 120 sub-frames a second, 1080p60 and 1440p60
//! reel png DIR [--every N]         the preview's frames as PNGs, every Nth
//!     [--slide K]                  one slide alone, from its own start
//!     [--audio FILE]               a track under the encodes, faded out at the end
//! ```
//!
//! - **Slides on one canvas.** Each slide is a live subtree on one pan-zoom canvas, and the
//!   camera glides between them through the canvas scroll, mostly as a pan. Refused:
//!   shared-element morphs between every pair (Keynote's Magic Move), a design per pair;
//!   crossfades; cuts.
//! - **Choreography is Rust**, a list of steps a slide. Refused: the showcase's shot script,
//!   whose timing, easing and overlap would grow a language (Motion Canvas times in code too).
//! - **The cursor feeds real input**, so widgets animate their own states. Refused: a cursor
//!   composited on the frames afterwards.
//! - **Frames stream to ffmpeg**, raw on its stdin. Refused: a PNG of every frame, tens of GB
//!   written and read again; PNGs stay for a look at a frame or a range.
//! - **The track cuts on its bars**: slides round up to whole 3 s bars, 4/4 at 80 BPM, and the
//!   reel tells its story muted, as muted autoplay is the common case. Refused: the YouTube
//!   Audio Library, whose standard tracks are for YouTube alone.
//! - **The claim is "every frame drawn by palantir, in Rust"**: ffmpeg encodes and the music comes
//!   from outside, so "made only with palantir" would be false. Slide 9 credits the MCP server
//!   to Darkroom, which serves its window on palantir's `Harness` and `Remote`.

mod code;
mod cube;
mod cursor;
mod deck;
mod dock;
mod downscale;
mod motion;
mod script;
mod slides;
mod timeline;

use std::env;
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::Duration;

use palantir::glam::{UVec2, Vec2};
use palantir::{Harness, HarnessDepth, RequestedGpu, Scroll, SystemScale};

use crate::deck::{Deck, SLIDE, SLIDES};
use crate::downscale::{Linear, Shrink};
use crate::motion::View;
use crate::script::Runner;
use crate::timeline::{frame_time, whole_bars};

/// What a run renders.
#[derive(Clone, Debug)]
enum Mode {
    Preview { out: PathBuf },
    Final { dir: PathBuf },
    Png { dir: PathBuf, every: u64 },
}

#[derive(Clone, Debug)]
struct Args {
    mode: Mode,
    slide: Option<usize>,
    audio: Option<PathBuf>,
}

fn parse() -> Result<Args, String> {
    let mut words = env::args().skip(1);
    let mut mode = match words.next().as_deref() {
        Some("preview") => Mode::Preview {
            out: PathBuf::from("reel-preview.mp4"),
        },
        Some("final") => Mode::Final {
            dir: PathBuf::from("reel"),
        },
        Some("png") => Mode::Png {
            dir: words.next().ok_or("png takes a folder")?.into(),
            every: 1,
        },
        _ => return Err("usage: reel preview|final|png DIR [--slide K] [--audio FILE]".into()),
    };
    let (mut slide, mut audio) = (None, None);
    while let Some(word) = words.next() {
        let mut value = || words.next().ok_or_else(|| format!("{word} takes a value"));
        match (word.as_str(), &mut mode) {
            ("--out", Mode::Preview { out }) => *out = value()?.into(),
            ("--out-dir", Mode::Final { dir }) => *dir = value()?.into(),
            ("--every", Mode::Png { every, .. }) => {
                *every = value()?.parse().map_err(|_| "--every takes a count")?;
            }
            ("--slide", _) => {
                let k: usize = value()?.parse().map_err(|_| "--slide takes a number")?;
                if k >= SLIDES {
                    return Err(format!("there are {SLIDES} slides"));
                }
                slide = Some(k);
            }
            ("--audio", _) => audio = Some(value()?.into()),
            _ => return Err(format!("unknown option {word}")),
        }
    }
    Ok(Args { mode, slide, audio })
}

fn main() -> ExitCode {
    match parse().and_then(|args| run(&args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            writeln!(io::stderr(), "reel: {error}").ok();
            ExitCode::FAILURE
        }
    }
}

/// How the window renders: its physical size, scale and format, and the sub-frames a second.
#[derive(Clone, Copy, Debug)]
struct Render {
    scale: f32,
    fps: u32,
    float: bool,
}

impl Render {
    const PREVIEW: Self = Self {
        scale: 1.0,
        fps: 60,
        float: false,
    };
    /// 4K, the logical canvas at an integer scale of 3, so no edge lands between two pixels; 120
    /// sub-frames a second, a pair blended in linear light into each 60 fps frame, a 180°
    /// shutter. Refused: a 120 fps delivery, whose 4K H.264 needs level 6, which few players
    /// decode, and which YouTube does not show past 60 fps
    /// (<https://support.google.com/youtube/answer/1722171>).
    const FINAL: Self = Self {
        scale: 3.0,
        fps: 120,
        float: true,
    };

    fn size(self) -> UVec2 {
        (SLIDE * self.scale).as_uvec2()
    }
}

fn run(args: &Args) -> Result<(), String> {
    let render = match args.mode {
        Mode::Final { .. } => Render::FINAL,
        Mode::Preview { .. } | Mode::Png { .. } => Render::PREVIEW,
    };
    let gpu = RequestedGpu::headless(palantir::PowerPreference::HighPerformance)
        .map_err(|error| error.to_string())?
        .gpu;
    let depth = if render.float {
        HarnessDepth::gpu_float(gpu)
    } else {
        HarnessDepth::gpu(gpu)
    };
    let mut h = Harness::builder(render.size())
        .system_scale(SystemScale::new(render.scale).unwrap())
        .depth(depth)
        .build();
    let mut sink = Sink::open(args)?;
    let slides = args.slide.map_or(0..SLIDES, |k| k..k + 1);
    let mut deck = Deck::new();
    deck::dress(h.ui_mut());
    let mut pixels = Vec::new();
    let frames = play(&mut h, &mut deck, slides, render.fps, |beat, h, deck| {
        match beat {
            Beat::Settle => h
                .capture(deck, &mut Vec::new())
                .map_err(|e| e.to_string())?,
            Beat::Frame(n) => {
                h.capture(deck, &mut pixels).map_err(|e| e.to_string())?;
                sink.frame(n, &pixels)?;
            }
            Beat::SlideEnd { k, length } => {
                writeln!(io::stderr(), "slide {k}: {:.1} s", length.as_secs_f32()).ok();
            }
        }
        Ok(())
    })?;
    sink.finish(frame_time(frames, render.fps))
}

/// What [`play`] asks of its caller.
#[derive(Clone, Copy, Debug)]
enum Beat {
    /// Draw a frame no one shows, so a step that reaches for a widget finds it laid out.
    Settle,
    /// Draw frame `n` of the reel.
    Frame(u64),
    /// Slide `k` ended, after `length`.
    SlideEnd { k: usize, length: Duration },
}

/// Play `slides` at `fps`, scripted input and camera included, and answer how many frames it took.
fn play(
    h: &mut Harness,
    deck: &mut Deck,
    slides: Range<usize>,
    fps: u32,
    mut beat: impl FnMut(Beat, &mut Harness, &mut Deck) -> Result<(), String>,
) -> Result<u64, String> {
    let mut camera = deck::slide_view(slides.start);
    let mut cursor = SLIDE / 2.0 + Vec2::new(180.0, 120.0);
    deck.cursor = cursor;
    place_camera(h, camera);
    beat(Beat::Settle, h, deck)?;
    let mut n = 0_u64;
    for k in slides {
        let start = frame_time(n, fps);
        deck.now = start;
        slides::start(deck);
        let mut runner = Runner::new(slides::steps(k), cursor, camera);
        let mut end = None;
        loop {
            let now = frame_time(n, fps);
            if end.is_some_and(|end| now >= end) {
                break;
            }
            deck.now = now;
            runner.feed(now.saturating_sub(start), h, deck);
            if end.is_none()
                && let Some(done) = runner.finished()
            {
                end = Some(start + whole_bars(done));
            }
            deck.cursor = runner.cursor();
            deck.click = runner.last_click().map(|(at, place)| (start + at, place));
            place_camera(h, runner.camera());
            h.at(now);
            beat(Beat::Frame(n), h, deck)?;
            n += 1;
        }
        cursor = runner.cursor();
        camera = runner.camera();
        let length = frame_time(n, fps) - start;
        beat(Beat::SlideEnd { k, length }, h, deck)?;
    }
    Ok(n)
}

/// Point the canvas scroll at `view`, from the next frame.
fn place_camera(h: &mut Harness, view: View) {
    let zoom = (f64::from(SLIDE.x) / view.width) as f32;
    let size = Vec2::new(view.width as f32, view.width as f32 * SLIDE.y / SLIDE.x);
    let corner = Vec2::new(view.center.x as f32, view.center.y as f32) - size / 2.0;
    let id = deck::canvas_id();
    Scroll::set_zoom(h.ui_mut(), id, zoom);
    Scroll::set_offset(h.ui_mut(), id, corner * zoom);
}

/// Where frames go.
#[derive(Debug)]
enum Sink {
    Preview(Encoder),
    Png {
        dir: PathBuf,
        every: u64,
    },
    Final {
        sub: Linear,
        pair: Linear,
        deliveries: Vec<(Shrink, Encoder, Vec<u8>)>,
    },
}

impl Sink {
    fn open(args: &Args) -> Result<Self, String> {
        let audio = args.audio.as_deref();
        Ok(match &args.mode {
            Mode::Preview { out } => {
                Self::Preview(Encoder::spawn(out, Render::PREVIEW.size(), "3.2", audio)?)
            }
            Mode::Png { dir, every } => {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                Self::Png {
                    dir: dir.clone(),
                    every: *every,
                }
            }
            Mode::Final { dir } => {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let full = Render::FINAL.size();
                let delivery = |name: &str, size: UVec2, level| -> Result<_, String> {
                    Ok((
                        Shrink::new(
                            (full.x as usize, full.y as usize),
                            (size.x as usize, size.y as usize),
                        ),
                        Encoder::spawn(&dir.join(name), size, level, audio)?,
                        Vec::new(),
                    ))
                };
                Self::Final {
                    sub: Linear::default(),
                    pair: Linear::default(),
                    deliveries: vec![
                        delivery("reel-1080p60.mp4", UVec2::new(1920, 1080), "4.2")?,
                        delivery("reel-1440p60.mp4", UVec2::new(2560, 1440), "5.1")?,
                    ],
                }
            }
        })
    }

    fn frame(&mut self, n: u64, pixels: &[u8]) -> Result<(), String> {
        match self {
            Self::Preview(encoder) => encoder.write(pixels),
            Self::Png { dir, every } => {
                if n.is_multiple_of(*every) {
                    let path = dir.join(format!("{n:05}.png"));
                    let size = Render::PREVIEW.size();
                    image::save_buffer(&path, pixels, size.x, size.y, image::ColorType::Rgba8)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                }
                Ok(())
            }
            Self::Final {
                sub,
                pair,
                deliveries,
            } => {
                let size = Render::FINAL.size();
                if n.is_multiple_of(2) {
                    pair.load(pixels, size.x as usize, size.y as usize);
                    return Ok(());
                }
                sub.load(pixels, size.x as usize, size.y as usize);
                pair.average(sub);
                for (shrink, encoder, rgba) in deliveries {
                    shrink.encode(pair, rgba);
                    encoder.write(rgba)?;
                }
                Ok(())
            }
        }
    }

    fn finish(self, length: Duration) -> Result<(), String> {
        match self {
            Self::Preview(encoder) => encoder.finish(length),
            Self::Png { .. } => Ok(()),
            Self::Final { deliveries, .. } => {
                for (_, encoder, _) in deliveries {
                    encoder.finish(length)?;
                }
                Ok(())
            }
        }
    }
}

/// One ffmpeg encoding raw RGBA from its stdin as BT.709 H.264.
#[derive(Debug)]
struct Encoder {
    child: Child,
    /// What the encode writes: the delivery itself, or with a track, the picture alone, which
    /// [`Self::finish`] muxes once the length is known.
    video: PathBuf,
    out: PathBuf,
    audio: Option<PathBuf>,
}

/// The track's fade out at the end of the picture.
const FADE: Duration = Duration::from_secs(3);

impl Encoder {
    /// Each frame raw 8-bit RGBA of `size`, at 60 fps. The conversion tags BT.709 and converts
    /// with its matrix, as ffmpeg would take BT.601 and shift every color a browser shows; the
    /// level is named for the size and rate. `setparams` tags the frames themselves, as the
    /// output's `-color_primaries` and `-color_trc` alone left both unknown in the file
    /// (ffprobe).
    fn spawn(out: &Path, size: UVec2, level: &str, audio: Option<&Path>) -> Result<Self, String> {
        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-y"]);
        command.args(["-f", "rawvideo", "-pix_fmt", "rgba"]);
        command.args([
            "-s",
            &format!("{}x{}", size.x, size.y),
            "-r",
            "60",
            "-i",
            "-",
        ]);
        command.args([
            "-vf",
            "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv",
            "-c:v",
            "libx264",
            "-preset",
            "slow",
            "-crf",
            "17",
            "-profile:v",
            "high",
            "-level",
            level,
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-movflags",
            "+faststart",
        ]);
        let video = audio.map_or_else(|| out.to_path_buf(), |_| out.with_extension("video.mp4"));
        command.arg(&video).stdin(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|e| format!("ffmpeg: {e}; install it to encode, or render PNGs"))?;
        Ok(Self {
            child,
            video,
            out: out.to_path_buf(),
            audio: audio.map(Path::to_path_buf),
        })
    }

    fn write(&mut self, rgba: &[u8]) -> Result<(), String> {
        self.child
            .stdin
            .as_mut()
            .expect("the encoder's stdin is piped")
            .write_all(rgba)
            .map_err(|e| format!("{}: {e}", self.out.display()))
    }

    /// Close the encode, then, with a track, lay it under the picture, cut at the picture's
    /// `length` and faded out over its last [`FADE`]. The cut is the picture's length, not the
    /// shorter stream's (`-shortest`), so a track that ends early leaves the picture whole.
    fn finish(mut self, length: Duration) -> Result<(), String> {
        drop(self.child.stdin.take());
        let status = self.child.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "ffmpeg failed on {}: {status}",
                self.video.display()
            ));
        }
        let Some(audio) = &self.audio else {
            return Ok(());
        };
        let fade = format!(
            "afade=t=out:st={:.3}:d={:.3}",
            length.saturating_sub(FADE).as_secs_f64(),
            FADE.as_secs_f64()
        );
        let status = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&self.video)
            .arg("-i")
            .arg(audio)
            .args([
                "-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
            ])
            .args(["-af", &fade, "-t", &format!("{:.6}", length.as_secs_f64())])
            .args(["-movflags", "+faststart"])
            .arg(&self.out)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!(
                "ffmpeg failed muxing {}: {status}",
                self.out.display()
            ));
        }
        std::fs::remove_file(&self.video).map_err(|e| format!("{}: {e}", self.video.display()))
    }
}

#[cfg(test)]
mod tests {
    use palantir::glam::{UVec2, Vec2};
    use palantir::internals::headless_test_gpu;
    use palantir::{Harness, HarnessDepth, SystemScale};

    use crate::Beat;
    use crate::deck::{self, Deck};
    use crate::script::Runner;
    use crate::timeline::frame_time;

    /// No two widgets of the board share an explicit id, which paints a magenta outline over a
    /// render: the whole reel plays, its scripted input opening what it opens, at 10 frames a
    /// second, as the board records every slide on every frame.
    #[test]
    fn no_slide_collides_an_explicit_id() {
        let mut h = Harness::builder(UVec2::new(320, 180))
            .system_scale(SystemScale::new(0.25).unwrap())
            .depth(HarnessDepth::Compose)
            .build();
        let mut d = Deck::new();
        deck::dress(h.ui_mut());
        let frames = crate::play(&mut h, &mut d, 0..deck::SLIDES, 10, |beat, h, d| {
            if !matches!(beat, Beat::SlideEnd { .. }) {
                h.frame_app(d);
                assert_eq!(h.ui().explicit_id_collision_count(), 0, "{beat:?}");
            }
            Ok(())
        })
        .unwrap();
        assert!(frames > 0);
    }

    /// A slide rendered twice gives the same bytes: nothing reads the wall clock or a thread's
    /// timing. Slide 3 drives a switch, a slider, a combo box and typed text.
    #[test]
    fn a_slide_rendered_twice_gives_the_same_bytes() {
        let gpu = headless_test_gpu();
        let render = |gpu| {
            let mut h = Harness::builder(UVec2::new(320, 180))
                .system_scale(SystemScale::new(0.25).unwrap())
                .depth(HarnessDepth::gpu(gpu))
                .build();
            let mut d = Deck::new();
            deck::dress(h.ui_mut());
            crate::place_camera(&mut h, deck::slide_view(3));
            h.capture(&mut d, &mut Vec::new()).unwrap();
            let mut runner = Runner::new(
                crate::slides::steps(3),
                Vec2::new(800.0, 500.0),
                deck::slide_view(3),
            );
            let mut frames = Vec::new();
            for n in 0..90 {
                let now = frame_time(n * 4, 60);
                d.now = now;
                runner.feed(now, &mut h, &mut d);
                d.cursor = runner.cursor();
                crate::place_camera(&mut h, runner.camera());
                h.at(now);
                let mut pixels = Vec::new();
                h.capture(&mut d, &mut pixels).unwrap();
                frames.push(pixels);
            }
            frames
        };
        assert!(render(gpu.handles()) == render(gpu.handles()));
    }
}
