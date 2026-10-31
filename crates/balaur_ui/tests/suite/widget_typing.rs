//! What a reader types into a field: shaped as a label is, composed by an
//! input method in place, and taken by the keys and the clipboard.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use egui::pos2;

/// A field focused by its node, ready for typing on the next pass.
fn focused_field(
    text: &str,
) -> (
    tempfile::TempDir,
    balaur_core::App,
    egui::Context,
    balaur_core::hecs::Entity,
) {
    let (dir, app) = app();
    let field = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = text x = 0.0 y = 0.0 width = 300.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    {
        let focus = app.engine.resource::<balaur_ui::UiFocus>();
        let mut focus = focus.borrow_mut();
        focus.focused = Some(field);
        focus.taking = true;
    }
    pass(&app, &ctx, vec![]);
    (dir, app, ctx, field)
}

fn text_of(app: &balaur_core::App, field: balaur_core::hecs::Entity) -> String {
    property(app, field, "text")
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn changes(app: &balaur_core::App, field: balaur_core::hecs::Entity) -> usize {
    balaur_core::events::delivered_from(&app.engine, field, balaur_ui::CHANGE_EVENT).len()
}

fn chord(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    vec![egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }]
}

/// Each glyph's place in the atlas, its size, and how far it sits from the
/// first glyph drawn below `top` and above `bottom`.
fn shape_of(
    out: &egui::FullOutput,
    top: f32,
    bottom: f32,
) -> Vec<(egui::Rect, egui::Vec2, egui::Vec2)> {
    let mut drawn: Vec<(egui::Rect, egui::Rect)> = glyphs(out)
        .into_iter()
        .filter(|(at, _)| at.center().y > top && at.center().y < bottom)
        .collect();
    drawn.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x));
    let Some(first) = drawn.first().map(|(at, _)| at.min) else {
        return Vec::new();
    };
    drawn
        .into_iter()
        .map(|(at, uv)| (uv, at.size(), at.min - first))
        .collect()
}

#[test]
fn an_arabic_a_hindi_and_a_thai_string_give_the_same_glyphs_in_a_field_as_in_a_label() {
    for text in ["سلام عليكم", "नमस्ते दुनिया", "สวัสดีครับ"]
    {
        let (_dir, app) = app();
        add_widget(
            &app,
            &toml::toml! { kind = "label" text = text font_size = 18.0 x = 0.0 y = 0.0 }.into(),
        );
        add_widget(
            &app,
            &toml::toml! { kind = "text_field" text = text font_size = 18.0 x = 0.0 y = 100.0 width = 400.0 }.into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        let out = pass(&app, &ctx, vec![]);
        let label = shape_of(&out, -1.0, 60.0);
        let field = shape_of(&out, 90.0, 200.0);
        assert!(
            !label.is_empty(),
            "control: the label of {text:?} drew glyphs"
        );
        assert_eq!(label.len(), field.len(), "{text:?}: as many glyphs");
        for (one, other) in label.iter().zip(&field) {
            assert_eq!(one.0, other.0, "{text:?}: the same glyph");
            assert!((one.1 - other.1).length() < 0.01, "{text:?}: the same size");
            assert!(
                (one.2 - other.2).length() < 0.01,
                "{text:?}: in the same place"
            );
        }
    }
}

#[test]
fn a_composition_lands_as_one_insert() {
    let (_dir, mut app, ctx, field) = focused_field("");
    let preedit = |text: &str| {
        vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: text.into(),
            active_range_chars: None,
        })]
    };
    let shown = pass(&app, &ctx, preedit("ㅎ"));
    assert_eq!(glyphs(&shown).len(), 1, "what is composed shows in place");
    pass(&app, &ctx, preedit("하"));
    consume_input(&mut app);
    assert_eq!(text_of(&app, field), "", "a composition is not text yet");
    assert_eq!(changes(&app, field), 0, "and changes nothing");
    pass(
        &app,
        &ctx,
        vec![egui::Event::Ime(egui::ImeEvent::Commit("한".into()))],
    );
    consume_input(&mut app);
    assert_eq!(text_of(&app, field), "한");
    assert_eq!(changes(&app, field), 1, "the commit is one change");
    pass(&app, &ctx, chord(egui::Key::Z, egui::Modifiers::COMMAND));
    consume_input(&mut app);
    assert_eq!(text_of(&app, field), "", "and one step of undo");
}

#[test]
fn the_keys_move_the_caret_and_take_the_clipboard() {
    let (_dir, mut app, ctx, field) = focused_field("hello world");
    // Focus put on the field selected its text; Home drops the selection.
    pass(&app, &ctx, chord(egui::Key::Home, egui::Modifiers::NONE));
    pass(
        &app,
        &ctx,
        chord(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
    );
    pass(
        &app,
        &ctx,
        chord(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
    );
    let out = pass(&app, &ctx, chord(egui::Key::C, egui::Modifiers::COMMAND));
    let copied: Vec<String> = out
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        copied,
        vec!["he".to_string()],
        "the copy takes the selection"
    );
    pass(&app, &ctx, chord(egui::Key::X, egui::Modifiers::COMMAND));
    pass(&app, &ctx, chord(egui::Key::End, egui::Modifiers::NONE));
    pass(&app, &ctx, vec![egui::Event::Paste("!".into())]);
    consume_input(&mut app);
    assert_eq!(text_of(&app, field), "llo world!");
}

#[test]
fn a_click_puts_the_caret_where_it_lands() {
    let (_dir, mut app) = app();
    let field = add_widget(
        &app,
        &toml::toml! { kind = "text_field" text = "abcdef" x = 0.0 y = 0.0 width = 300.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let mut letters = glyphs(&out);
    letters.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x));
    // Between the third and the fourth letter.
    let at = pos2(
        f32::midpoint(letters[2].0.max.x, letters[3].0.min.x),
        letters[2].0.center().y,
    );
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    pass(&app, &ctx, vec![egui::Event::Text("-".into())]);
    consume_input(&mut app);
    assert_eq!(text_of(&app, field), "abc-def");
}
