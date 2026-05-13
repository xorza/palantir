use palantir::{Button, Configure, FocusPolicy, Panel, Sizing, Text, TextEdit, Ui, WidgetId};

/// Two TextEdits + a Button + an echo line.
///
/// What this exercises by hand:
/// - Click into either field → the focused border tint fades in.
/// - Type → characters land at the caret. Arrow keys / Home / End / Backspace
///   / Delete navigate. Escape clears focus.
/// - Click the Button. Default policy is `ClearOnMiss` so focus drops and
///   subsequent keys aren't routed to the editor — the toggle below flips to
///   `PreserveOnMiss` if you want to demonstrate sticky focus instead.
/// - Click another field → the original loses focus, new one takes over.
///
/// Buffer storage: stashed in `Ui::state_mut::<String>` under a non-widget id,
/// so it survives across showcase tab switches. The widget itself takes
/// `&mut String`, so we `mem::take` out of the state map for the body and put
/// it back at the end. Two moves of a small `String` — fine.
pub fn build(ui: &mut Ui) {
    let buf_a_id = WidgetId::from_hash("textedit_showcase__buffer_a");
    let buf_b_id = WidgetId::from_hash("textedit_showcase__buffer_b");
    let buf_ml_id = WidgetId::from_hash("textedit_showcase__buffer_ml");
    let policy_id = WidgetId::from_hash("textedit_showcase__policy");

    let mut buf_a = std::mem::take(ui.state_mut::<String>(buf_a_id));
    let mut buf_b = std::mem::take(ui.state_mut::<String>(buf_b_id));
    let mut buf_ml = std::mem::take(ui.state_mut::<String>(buf_ml_id));
    let policy = *ui.state_mut::<FocusPolicy>(policy_id);
    ui.set_focus_policy(policy);

    Panel::vstack()
        .auto_id()
        .padding(20.0)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("TextEdit — single-line editable text leaf.")
                .auto_id()
                .show(ui);
            Text::new(
                "Click to focus, type to insert, arrows / Home / End / Backspace / Delete \
                 navigate, Escape blurs.",
            )
            .auto_id()
            .wrapping()
            .show(ui);

            Panel::hstack()
                .id_salt("editors")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    TextEdit::new(&mut buf_a)
                        .id_salt("editor_a")
                        .placeholder("first field")
                        .size((Sizing::FILL, Sizing::Hug))
                        .min_size((180.0, 32.0))
                        .show(ui);
                    TextEdit::new(&mut buf_b)
                        .id_salt("editor_b")
                        .placeholder("second field")
                        .size((Sizing::FILL, Sizing::Hug))
                        .min_size((180.0, 32.0))
                        .show(ui);
                });

            Panel::hstack()
                .id_salt("controls")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    let label = match policy {
                        FocusPolicy::ClearOnMiss => "policy: ClearOnMiss",
                        FocusPolicy::PreserveOnMiss => "policy: PreserveOnMiss",
                    };
                    if Button::new()
                        .id_salt("policy_toggle")
                        .label(label)
                        .min_size((220.0, 32.0))
                        .show(ui)
                        .clicked()
                    {
                        let next = match policy {
                            FocusPolicy::ClearOnMiss => FocusPolicy::PreserveOnMiss,
                            FocusPolicy::PreserveOnMiss => FocusPolicy::ClearOnMiss,
                        };
                        *ui.state_mut::<FocusPolicy>(policy_id) = next;
                    }
                    if Button::new()
                        .id_salt("clear")
                        .label("clear both")
                        .min_size((140.0, 32.0))
                        .show(ui)
                        .clicked()
                    {
                        buf_a.clear();
                        buf_b.clear();
                    }
                });

            Text::new(format!("buffer A ({:>2} bytes): {}", buf_a.len(), buf_a))
                .auto_id()
                .show(ui);
            Text::new(format!("buffer B ({:>2} bytes): {}", buf_b.len(), buf_b))
                .auto_id()
                .show(ui);

            Text::new(
                "Multi-line: Enter inserts \\n, Up/Down navigate visual lines, \
                 selection spans newlines, paste preserves multi-line clipboard.",
            )
            .auto_id()
            .wrapping()
            .show(ui);
            TextEdit::new(&mut buf_ml)
                .id_salt("editor_ml")
                .multiline(true)
                .placeholder("paste a paragraph here")
                .size((Sizing::FILL, Sizing::Fixed(160.0)))
                .min_size((180.0, 80.0))
                .show(ui);
        });

    *ui.state_mut::<String>(buf_a_id) = buf_a;
    *ui.state_mut::<String>(buf_b_id) = buf_b;
    *ui.state_mut::<String>(buf_ml_id) = buf_ml;
}
