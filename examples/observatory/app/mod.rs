//! The observatory: five studies of one fictional tool over one [`Observatory`] model, the rail
//! that picks among them, and the eyepiece window.

pub(crate) mod model;
pub(crate) mod studies;

use palantir::{
    App, Checkbox, Configure, Key, Panel, Separator, Shortcut, Sizing, Ui, UserScale, Waker,
    WidgetId, WindowConfig, WindowToken,
};

use crate::app::model::Observatory;
use crate::app::studies::Study;
use crate::scaffold::rail;
use crate::scaffold::rail::Nav;
use crate::scaffold::tokens::Tokens;
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

    /// The rail: the studies, then the scheme and the debug switches.
    fn rail(&mut self, ui: &mut Ui, t: Tokens) {
        let nav = Nav::new(ui.theme(), t);
        rail::show(ui, t, |ui| {
            rail::brand(ui, t, "observatory", "a palantir study");
            rail::list(ui, WidgetId::from_hash("observatory::rail"), |ui| {
                for study in Study::ALL {
                    let id = WidgetId::from_hash(("observatory::rail", study.label()));
                    if nav.item(ui, id, study.label(), study == self.study) {
                        self.study = study;
                    }
                }
            });
            rail::block(ui, |ui| {
                rail::label(ui, t, "SCHEME");
                rail::scheme_switch(ui);
            });
            rail::block(ui, debug_toggles);
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

/// The debug overlays, each mirroring its key.
fn debug_toggles(ui: &mut Ui) {
    let mut overlay = ui.debug_overlay();
    Checkbox::new(&mut overlay.damage_rect)
        .label("damage rects  F12")
        .show(ui);
    Checkbox::new(&mut overlay.frame_stats)
        .label("frame stats  F9")
        .show(ui);
    ui.set_debug_overlay(overlay);
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
