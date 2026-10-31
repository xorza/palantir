//! The observatory: five studies of one fictional tool over one [`Observatory`] model, the rail
//! that picks among them, and the eyepiece window.

pub(crate) mod model;
pub(crate) mod scaffold;
pub(crate) mod studies;

use palantir::{
    Align, App, Axis, Button, ButtonTheme, Checkbox, Chrome, ColorScheme, Configure, Key, Panel,
    RadioButton, SchemePreference, Scroll, Separator, Shortcut, Sizing, SlotDefaults, Spacing,
    StatefulLook, Text, Theme, Ui, UserScale, Waker, WindowConfig, WindowToken, fmt,
};

use crate::app::model::Observatory;
use crate::app::scaffold::Tokens;
use crate::app::studies::Study;
use crate::shot::ShotApp;

/// The main window.
pub(crate) const MAIN_WINDOW: WindowToken = WindowToken(0);
/// The eyepiece, a second OS window that shows the selection from the same model.
pub(crate) const EYEPIECE_WINDOW: WindowToken = WindowToken(1);

/// Open the eyepiece, or close it if open; the live window set decides.
pub(crate) fn toggle_eyepiece(ui: &mut Ui) {
    if ui.is_window_open(EYEPIECE_WINDOW) {
        ui.close_window(EYEPIECE_WINDOW);
    } else {
        ui.open_window(EYEPIECE_WINDOW, WindowConfig::new("eyepiece"));
    }
}

const RAIL_WIDTH: f32 = 208.0;

/// The app: which study shows, the model every study shares, and how to wake the eyepiece.
#[derive(Debug)]
pub(crate) struct Shell {
    study: Study,
    pub(crate) model: Observatory,
    /// The eyepiece's waker, from its last frame: the main window wakes it, as a window that
    /// records nothing sleeps through a change it did not make.
    eyepiece: Option<Waker>,
}

impl Shell {
    /// The sky chart, the home study, over a fresh model.
    pub(crate) fn new() -> Self {
        Self::on(Study::SkyChart)
    }

    /// `study` over a fresh model.
    pub(crate) fn on(study: Study) -> Self {
        Self {
            study,
            model: Observatory::new(),
            eyepiece: None,
        }
    }

    fn main(&mut self, ui: &mut Ui) {
        self.record_main(ui);
        // An edit made in this frame builds its theme now; palantir asks for the frame that
        // records the window under it.
        self.model.appearance.apply(ui);
        // The eyepiece shows the same model and records only on its own frames, so every
        // main-window frame wakes it; a frame that changed nothing it shows paints nothing.
        if let Some(eyepiece) = &self.eyepiece {
            eyepiece.wake();
        }
    }

    fn record_main(&mut self, ui: &mut Ui) {
        self.shortcuts(ui);
        self.model.appearance.apply(ui);
        let t = Tokens::of(self.model.appearance.palette(ui.color_scheme()));
        Panel::hstack()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                self.rail(ui, t);
                Panel::vstack()
                    .id_salt(self.study.label())
                    .size((Sizing::FILL, Sizing::FILL))
                    .padding((24.0, 20.0, 16.0, 16.0))
                    .gap(16.0)
                    .show(ui, |ui| {
                        self.study.header(ui, t);
                        Separator::horizontal().show(ui);
                        if let Some(study) = self.study.show(ui, &mut self.model, t) {
                            self.study = study;
                        }
                    });
            });
    }

    /// The rail: the studies, then the look and the debug switches.
    fn rail(&mut self, ui: &mut Ui, t: Tokens) {
        let idle = nav_style(ui.theme(), t, false);
        let selected = nav_style(ui.theme(), t, true);
        Panel::vstack()
            .id_salt("rail")
            .size((Sizing::fixed(RAIL_WIDTH), Sizing::FILL))
            .padding((12.0, 16.0, 12.0, 12.0))
            .gap(12.0)
            .chrome(Chrome::fill(t.surround))
            .show(ui, |ui| {
                Panel::vstack()
                    .id_salt("brand")
                    .padding((12.0, 0.0, 0.0, 0.0))
                    .gap(2.0)
                    .show(ui, |ui| {
                        Text::new("observatory").style(&t.title()).show(ui);
                        Text::new("a palantir study").style(&t.caption()).show(ui);
                    });
                Scroll::vertical()
                    .id_salt("studies")
                    .size((Sizing::FILL, Sizing::FILL))
                    .overlay_bars()
                    .gap(2.0)
                    .arrow_focus(Axis::Y)
                    .show(ui, |ui| {
                        for (idx, study) in Study::ALL.into_iter().enumerate() {
                            let style = if study == self.study {
                                &selected
                            } else {
                                &idle
                            };
                            let clicked = Button::new()
                                .id_salt(study.label())
                                .label(study.label())
                                .style(style)
                                .text_align(Align::LEFT)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui)
                                .clicked();
                            if clicked {
                                self.study = Study::ALL[idx];
                            }
                        }
                    });
                Separator::horizontal().show(ui);
                scheme_switch(ui, t);
                debug_toggles(ui);
            });
    }

    /// Ctrl+1 … Ctrl+5 pick a study; F8 the eyepiece; F9, F10, F12 the debug overlays; Ctrl+plus,
    /// minus and 0 the UI scale; Ctrl+Q quits.
    fn shortcuts(&mut self, ui: &mut Ui) {
        for (idx, digit) in ['1', '2', '3', '4', '5'].into_iter().enumerate() {
            if ui.shortcut_pressed(Shortcut::ctrl(digit)) {
                self.study = Study::ALL[idx];
            }
        }
        if ui.shortcut_pressed(Shortcut::ctrl('Q')) {
            ui.close_window(MAIN_WINDOW);
        }
        if ui.shortcut_pressed(Key::F8) {
            toggle_eyepiece(ui);
        }
        let mut overlay = ui.debug_overlay();
        overlay.damage_rect ^= ui.shortcut_pressed(Key::F12);
        overlay.dim_undamaged ^= ui.shortcut_pressed(Key::F10);
        overlay.frame_stats ^= ui.shortcut_pressed(Key::F9);
        ui.set_debug_overlay(overlay);
        let mut scale = ui.user_scale();
        let up = ui.shortcut_pressed(Shortcut::ctrl('='))
            || ui.shortcut_pressed(Shortcut::ctrl('+'))
            || ui.shortcut_pressed(Shortcut::ctrl_shift('+'));
        if up {
            scale = scale.stepped_up();
        }
        if ui.shortcut_pressed(Shortcut::ctrl('-')) {
            scale = scale.stepped_down();
        }
        if ui.shortcut_pressed(Shortcut::ctrl('0')) {
            scale = UserScale::ONE;
        }
        ui.set_user_scale(scale);
    }
}

impl App for Shell {
    fn record(&mut self, window: WindowToken, ui: &mut Ui) {
        match window {
            EYEPIECE_WINDOW => {
                self.eyepiece = Some(ui.waker());
                self.model.appearance.apply(ui);
                let t = Tokens::of(self.model.appearance.palette(ui.color_scheme()));
                studies::eyepiece(ui, &self.model, t);
            }
            _ => self.main(ui),
        }
    }
}

impl ShotApp for Shell {
    fn page_labels() -> impl Iterator<Item = &'static str> {
        Study::ALL.into_iter().map(Study::label)
    }

    fn open(page: usize, _ui: &mut Ui) -> Self {
        Self::on(Study::ALL[page])
    }
}

/// System, Light or Dark, the system's own answer beside System.
fn scheme_switch(ui: &mut Ui, t: Tokens) {
    let mut preference = ui.scheme_preference();
    let system = match ui.system_color_scheme() {
        Some(ColorScheme::Light) => "light",
        Some(ColorScheme::Dark) => "dark",
        None => "unknown",
    };
    Panel::vstack()
        .id_salt("scheme")
        .padding((12.0, 0.0, 0.0, 0.0))
        .gap(6.0)
        .show(ui, |ui| {
            Text::new("SCHEME").style(&t.heading()).show(ui);
            let system_label = fmt!(ui, "system ({system})");
            for (value, label) in [
                (SchemePreference::System, system_label),
                (SchemePreference::Light, ui.intern("light")),
                (SchemePreference::Dark, ui.intern("dark")),
            ] {
                RadioButton::new(&mut preference, value)
                    .id_salt(("scheme", value as u8))
                    .label(label)
                    .show(ui);
            }
        });
    ui.set_scheme_preference(preference);
}

/// The debug overlays, each mirroring its key.
fn debug_toggles(ui: &mut Ui) {
    let mut overlay = ui.debug_overlay();
    Panel::vstack()
        .id_salt("debug")
        .padding((12.0, 0.0, 0.0, 0.0))
        .gap(6.0)
        .show(ui, |ui| {
            Checkbox::new(&mut overlay.damage_rect)
                .label("damage rects  F12")
                .show(ui);
            Checkbox::new(&mut overlay.frame_stats)
                .label("frame stats  F9")
                .show(ui);
        });
    ui.set_debug_overlay(overlay);
}

/// A rail item: the menu bar's title look on an `xs` inset, in the accent while its study shows.
fn nav_style(theme: &Theme, t: Tokens, selected: bool) -> ButtonTheme {
    let bar = &theme.menu_bar;
    let mut looks = bar.title.clone();
    if selected {
        let open = bar.open.clone();
        looks = StatefulLook {
            normal: open.clone(),
            hovered: open,
            active: looks.active,
            disabled: looks.disabled,
        };
        for look in [&mut looks.normal, &mut looks.hovered, &mut looks.active] {
            look.text = look.text.with_color(t.accent);
        }
    }
    ButtonTheme {
        selected: looks.clone(),
        unselected: looks,
        defaults: SlotDefaults {
            padding: Spacing::xy(12.0, 4.0),
            ..bar.defaults
        },
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::app::Shell;
    use crate::app::studies::Study;

    impl Shell {
        /// The study the rail shows.
        pub(crate) const fn study(&self) -> Study {
            self.study
        }
    }
}
