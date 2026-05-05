use palantir::{
    Button, Color, Configure, FocusPolicy, Panel, Sizing, Text, TextEdit, TextStyle, Ui, WidgetId,
};

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
    let policy_id = WidgetId::from_hash("textedit_showcase__policy");

    let mut buf_a = std::mem::take(ui.state_mut::<String>(buf_a_id));
    let mut buf_b = std::mem::take(ui.state_mut::<String>(buf_b_id));
    let policy = *ui.state_mut::<FocusPolicy>(policy_id);
    ui.set_focus_policy(policy);

    Panel::vstack()
        .padding(20.0)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new("TextEdit — single-line editable text leaf.")
                .style(TextStyle::default().with_color(Color::rgba(1.0, 1.0, 1.0, 0.85)))
                .show(ui);
            Text::new(
                "Click to focus, type to insert, arrows / Home / End / Backspace / Delete \
                 navigate, Escape blurs.",
            )
            .style(TextStyle::default().with_color(Color::rgba(1.0, 1.0, 1.0, 0.55)))
            .wrapping()
            .show(ui);

            Panel::hstack()
                .with_id("editors")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    TextEdit::new(&mut buf_a)
                        .with_id("editor_a")
                        .placeholder("first field")
                        .size((Sizing::FILL, Sizing::Hug))
                        .min_size((180.0, 32.0))
                        .show(ui);
                    TextEdit::new(&mut buf_b)
                        .with_id("editor_b")
                        .placeholder("second field")
                        .size((Sizing::FILL, Sizing::Hug))
                        .min_size((180.0, 32.0))
                        .show(ui);
                });

            Panel::hstack()
                .with_id("controls")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    let label = match policy {
                        FocusPolicy::ClearOnMiss => "policy: ClearOnMiss",
                        FocusPolicy::PreserveOnMiss => "policy: PreserveOnMiss",
                    };
                    if Button::new()
                        .with_id("policy_toggle")
                        .label(label)
                        .min_size((220.0, 32.0))
                        .show(ui)
                        .clicked()
                    {
                        // Flip on next frame.
                        let next = match policy {
                            FocusPolicy::ClearOnMiss => FocusPolicy::PreserveOnMiss,
                            FocusPolicy::PreserveOnMiss => FocusPolicy::ClearOnMiss,
                        };
                        *ui.state_mut::<FocusPolicy>(policy_id) = next;
                    }
                    if Button::new()
                        .with_id("clear")
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
                .style(TextStyle::default().with_color(Color::rgba(1.0, 1.0, 1.0, 0.75)))
                .show(ui);
            Text::new(format!("buffer B ({:>2} bytes): {}", buf_b.len(), buf_b))
                .style(TextStyle::default().with_color(Color::rgba(1.0, 1.0, 1.0, 0.75)))
                .show(ui);
        });

    *ui.state_mut::<String>(buf_a_id) = buf_a;
    *ui.state_mut::<String>(buf_b_id) = buf_b;
}
