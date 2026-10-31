//! What a script prints for each read step: a shot's path, size and legend, a probe's bytes, a
//! widget list.

use std::io::{self, Write};
use std::path::Path;

use palantir::glam::{UVec2, Vec2};
use palantir::{InspectedWidget, Rect, Screenshot, SrgbaU8};

/// How many widgets a legend lists; the rest are counted.
const LEGEND_LIMIT: usize = 60;

/// How many characters of a widget's text a line shows.
const TEXT_CUT: usize = 48;

/// The visible widgets with text a picture shows, each where it lands in the picture's texels:
/// what a locator names them by.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Legend {
    /// At most [`LEGEND_LIMIT`], in paint order.
    pub(crate) entries: Vec<LegendEntry>,
    /// The widgets the picture shows past the limit.
    pub(crate) omitted: usize,
}

/// One widget a picture shows.
#[derive(Debug, PartialEq)]
pub(crate) struct LegendEntry {
    pub(crate) text: String,
    /// In the picture's texels.
    pub(crate) rect: Rect,
}

impl Legend {
    /// The widgets of `widgets` with text that `shot`, fitted to `picture`, shows.
    pub(crate) fn of(shot: &Screenshot, picture: UVec2, widgets: &[InspectedWidget]) -> Self {
        let mut legend = Self::default();
        for widget in widgets
            .iter()
            .filter(|widget| !widget.text.trim().is_empty())
        {
            let Some(rect) = shot.place(widget.rect, picture) else {
                continue;
            };
            if legend.entries.len() == LEGEND_LIMIT {
                legend.omitted += 1;
            } else {
                legend.entries.push(LegendEntry {
                    text: widget.text.trim().to_owned(),
                    rect,
                });
            }
        }
        legend
    }
}

/// A shot's report: where it went, its size, the logical region and scale, and its legend.
pub(crate) fn shot(
    out: &mut impl Write,
    path: &Path,
    shot: &Screenshot,
    picture: UVec2,
    legend: &Legend,
) -> io::Result<()> {
    let region = shot.region();
    writeln!(
        out,
        "shot {} {}x{} region {} scale {}",
        path.display(),
        picture.x,
        picture.y,
        rect(region),
        shot.scale(),
    )?;
    for entry in &legend.entries {
        writeln!(out, "  {} {}", quoted(&entry.text), rect(entry.rect))?;
    }
    if legend.omitted > 0 {
        writeln!(out, "  … {} more", legend.omitted)?;
    }
    Ok(())
}

/// A probe's report: the point and the sRGB bytes there.
pub(crate) fn probe(out: &mut impl Write, point: Vec2, texel: SrgbaU8) -> io::Result<()> {
    let SrgbaU8 { r, g, b, a } = texel;
    writeln!(
        out,
        "probe {},{} #{r:02x}{g:02x}{b:02x}{a:02x}",
        point.x, point.y
    )
}

/// A widget list: each visible widget's text, logical rect and what it senses.
pub(crate) fn widgets(out: &mut impl Write, widgets: &[InspectedWidget]) -> io::Result<()> {
    writeln!(out, "widgets {}", widgets.len())?;
    for widget in widgets {
        writeln!(
            out,
            "  {} {}{}",
            quoted(&widget.text),
            rect(widget.rect),
            if widget.disabled { " disabled" } else { "" },
        )?;
    }
    Ok(())
}

/// `x,y,w,h`, to a tenth of a pixel.
fn rect(rect: Rect) -> String {
    let round = |v: f32| (v * 10.0).round() / 10.0;
    format!(
        "{},{},{},{}",
        round(rect.min.x),
        round(rect.min.y),
        round(rect.size.w),
        round(rect.size.h)
    )
}

/// `text` trimmed, cut at [`TEXT_CUT`] characters, and quoted as a locator reads it.
fn quoted(text: &str) -> String {
    let text = text.trim();
    let cut: String = text.chars().take(TEXT_CUT).collect();
    let more = if cut.len() < text.len() { "…" } else { "" };
    format!("{cut:?}{more}")
}
