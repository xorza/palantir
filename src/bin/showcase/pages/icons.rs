//! Baked SVG icons. Each one is rasterized at the exact physical pixel size
//! it lands on and cached in the icon atlas, so drawing this page exercises
//! the whole path — parse, resvg raster, atlas insert, glyph shader — and the
//! size ladder runs again on every window scale change.
//!
//! The set is built at startup from the sources below rather than compiled in
//! as a generated `const` — which is the point of `IconAtlas::from_svgs`: it
//! derives each icon's viewBox, tintability, and filter use from the artwork
//! itself, so a demo page states only its SVGs.

use crate::support;
use crate::support::{demo_cell_at, section, tiles};
use palantir::{Color, IconAtlas, IconId, IconSet, Ui};
use std::cell::RefCell;
use std::rc::Rc;

/// A diskette: several flat fills over a vertical gradient, inside a clip
/// path. The "real toolbar icon" case.
const SAVE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<defs>
<linearGradient id="b" x1="0" y1="0" x2="0" y2="1">
<stop offset="0" stop-color="#5b9cf8"/><stop offset="1" stop-color="#1b4fa8"/>
</linearGradient>
<clipPath id="c"><rect x="2" y="2" width="20" height="20" rx="2.5"/></clipPath>
</defs>
<g clip-path="url(#c)">
<path d="M2 4a2 2 0 0 1 2-2h13l5 5v13a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2Z" fill="url(#b)"/>
<rect x="7" y="2" width="10" height="7" fill="#e8eefc"/>
<rect x="13" y="3" width="2" height="5" fill="#31456b"/>
<rect x="6" y="13" width="12" height="9" fill="#f7faff"/>
<rect x="8" y="15.5" width="8" height="1.4" fill="#9fb4d8"/>
<rect x="8" y="18" width="8" height="1.4" fill="#9fb4d8"/>
</g></svg>"##;

/// A folder with a radial gradient and a soft drop shadow. The filter case —
/// 10-20x the raster cost of the others, and so the one the backend prewarms
/// at load instead of meeting on the frame that first draws it.
const FOLDER_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<defs>
<filter id="s" x="-40%" y="-40%" width="180%" height="180%">
<feGaussianBlur in="SourceAlpha" stdDeviation="0.7"/><feOffset dy="0.7" result="o"/>
<feMerge><feMergeNode in="o"/><feMergeNode in="SourceGraphic"/></feMerge>
</filter>
<radialGradient id="g" cx="35%" cy="28%" r="85%">
<stop offset="0" stop-color="#ffdc93"/><stop offset="1" stop-color="#e08a1f"/>
</radialGradient>
</defs>
<g filter="url(#s)">
<path d="M3 6a2 2 0 0 1 2-2h4.5l2 2H19a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" fill="url(#g)"/>
<path d="M3 18.2 6.2 11H22l-3.2 7.2Z" fill="#ffefcb"/>
</g></svg>"##;

/// A single-colour outline. Every paint resolves to one colour, so it is
/// marked tintable and takes a shape's full tint rather than only its alpha.
const NEW_FILE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path d="M6 2h8l5 5v15H6Z" fill="none" stroke="#fff" stroke-width="1.8" stroke-linejoin="round"/>
<path d="M14 2v5h5" fill="none" stroke="#fff" stroke-width="1.8" stroke-linejoin="round"/>
<path d="M12 11v7M8.5 14.5h7" fill="none" stroke="#fff" stroke-width="1.8" stroke-linecap="round"/>
</svg>"##;

/// A 2:1 artwork, so `Contain` has an aspect ratio to preserve and the size
/// ladder has a short axis to carry along with the long one.
const WIDE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 16">
<rect width="32" height="16" rx="3" fill="#2b3c5e"/>
<circle cx="8" cy="8" r="4" fill="#6ee7c8"/><circle cx="20" cy="8" r="4" fill="#f0a5d0"/>
<rect x="26" y="4" width="3" height="8" rx="1.5" fill="#facc15"/>
</svg>"##;

/// The set, plus the ids resolved once. `from_svgs` parses each source to
/// derive its viewBox, tintability, and filter use, and sorts by name — so
/// ids come back through `by_name` rather than being counted off by hand.
#[derive(Clone, Debug)]
struct Icons {
    set: IconSet,
    folder: IconId,
    new_file: IconId,
    save: IconId,
    wide: IconId,
}

fn icons(ui: &Ui) -> Icons {
    thread_local! {
        /// Built once rather than rebuilt per frame — parsing four SVGs to
        /// derive their viewBoxes is not free, and the `Rc` is what lets
        /// `load_icons` recognise the same allocation on the next frame.
        static BUILT: Rc<IconAtlas> = Rc::new(IconAtlas::from_svgs([
            ("folder", FOLDER_SVG),
            ("new-file", NEW_FILE_SVG),
            ("save", SAVE_SVG),
            ("wide", WIDE_SVG),
        ]));
        /// The *loaded* set, parked across frames. An `IconSet` owns the
        /// host's parsed SVGs and atlas rasters for its icons, so a page
        /// that let its own drop at the end of every frame would unload
        /// and re-rasterize the whole set on every frame. Holding it here
        /// is the same thing an app does by parking one in its state.
        static LOADED: RefCell<Option<IconSet>> = const { RefCell::new(None) };
    }
    let set = LOADED.with(|held| {
        held.borrow_mut()
            .get_or_insert_with(|| BUILT.with(|atlas| ui.load_icons(Rc::clone(atlas))))
            .clone()
    });
    let id = |name| set.by_name(name).expect("bundled icon");
    Icons {
        folder: id("folder"),
        new_file: id("new-file"),
        save: id("save"),
        wide: id("wide"),
        set,
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let icons = icons(ui);

    section(
        ui,
        "sizes — each cell rasterizes at its own physical size, not a scaled copy",
        |ui| {
            tiles(ui, |ui| {
                for (px, label) in SIZES {
                    demo_cell_at(ui, label, px + 24.0, px + 24.0, |ui| {
                        draw(ui, &icons, icons.save, Color::WHITE);
                    });
                }
            });
        },
    );

    section(
        ui,
        "artwork — gradients, a filter, a tintable outline, and a 2:1 aspect",
        |ui| {
            tiles(ui, |ui| {
                demo_cell_at(ui, "save — gradient + clip path", 120.0, 120.0, |ui| {
                    draw(ui, &icons, icons.save, Color::WHITE);
                });
                demo_cell_at(ui, "folder — radial + drop shadow", 120.0, 120.0, |ui| {
                    draw(ui, &icons, icons.folder, Color::WHITE);
                });
                demo_cell_at(ui, "new-file — tintable outline", 120.0, 120.0, |ui| {
                    draw(ui, &icons, icons.new_file, Color::WHITE);
                });
                demo_cell_at(ui, "wide — 2:1, Contain", 160.0, 120.0, |ui| {
                    draw(ui, &icons, icons.wide, Color::WHITE);
                });
            });
        },
    );

    section(
        ui,
        "tint — whole for a tintable icon, alpha and desaturation for a colour one",
        |ui| {
            tiles(ui, |ui| {
                for (label, tint) in [
                    ("tintable, white", Color::WHITE),
                    ("tintable, amber", Color::rgb(0.98, 0.75, 0.15)),
                    ("tintable, teal", Color::rgb(0.30, 0.85, 0.78)),
                ] {
                    demo_cell_at(ui, label, 96.0, 96.0, |ui| {
                        draw(ui, &icons, icons.new_file, tint);
                    });
                }
                // A colour icon takes only the tint's alpha, so fading is
                // what it gets instead of recolouring.
                demo_cell_at(ui, "colour icon at 40% alpha", 96.0, 96.0, |ui| {
                    draw(ui, &icons, icons.save, Color::rgba(1.0, 1.0, 1.0, 0.4));
                });
                // …and `desaturate` is the other half of a disabled state:
                // the artwork's own luminance, hue gone.
                demo_cell_at(ui, "colour icon desaturated", 96.0, 96.0, |ui| {
                    ui.add_shape(icons.set.shape(icons.save).desaturate(true));
                });
                demo_cell_at(ui, "desaturated + 50% alpha", 96.0, 96.0, |ui| {
                    ui.add_shape(
                        icons
                            .set
                            .shape(icons.save)
                            .desaturate(true)
                            .tint(Color::rgba(1.0, 1.0, 1.0, 0.5)),
                    );
                });
            });
        },
    );

    support::note(
        ui,
        "Resize the window between display scales: every cell re-rasterizes at \
         the new physical size rather than resampling, so edges stay exact.",
    );
}

fn draw(ui: &mut Ui, icons: &Icons, icon: IconId, tint: Color) {
    ui.add_shape(icons.set.shape(icon).tint(tint));
}

/// Cell captions need a `&'static str`, and the sizes are a fixed list, so
/// the label rides alongside the number rather than being formatted.
const SIZES: [(f32, &str); 5] = [
    (16.0, "16 px"),
    (24.0, "24 px"),
    (32.0, "32 px"),
    (48.0, "48 px"),
    (96.0, "96 px"),
];
