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
    field_from(toml::toml! { text = text })
}

/// A field at the top left, 300 wide, its rows extended by `own`, holding
/// the keyboard and ready for typing on the next pass.
fn field_from(
    own: toml::Table,
) -> (
    tempfile::TempDir,
    balaur_core::App,
    egui::Context,
    balaur_core::hecs::Entity,
) {
    let (dir, app) = app();
    let mut params = toml::toml! { kind = "text_field" x = 0.0 y = 0.0 width = 300.0 };
    params.extend(own);
    let field = add_widget(&app, &params.into());
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
    balaur_core::events::delivered_from(&app.engine, field, balaur_ui::VALUE_CHANGED_EVENT).len()
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
fn a_letter_typed_in_the_pass_of_a_later_chord_is_kept() {
    let (_dir, mut app, ctx, field) = focused_field("");
    let mut events = vec![
        egui::Event::Text("a".into()),
        egui::Event::ModifiersChanged(egui::Modifiers::COMMAND),
    ];
    events.extend(chord(egui::Key::C, egui::Modifiers::COMMAND));
    pass(&app, &ctx, events);
    consume_input(&mut app);
    assert_eq!(text_of(&app, field), "a");
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

// A root is a layer of its own, and egui keys focus by an id on one layer:
// a field moved under another root is drawn under a new id.
#[test]
fn a_focused_field_keeps_its_caret_when_it_moves_under_another_root() {
    let (_dir, mut app) = app();
    let roots: Vec<balaur_core::hecs::Entity> = [0.0, 100.0]
        .into_iter()
        .map(|y| {
            let root = toml::toml! { kind = "column" x = 0.0 y = y width = 320.0 height = 60.0 };
            add_widget(&app, &root.into())
        })
        .collect();
    let field = toml::toml! { kind = "text_field" text = "abcdef" width = 300.0 };
    let field = add_child_widget(&app, roots[0], "field", &field.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    {
        let focus = app.engine.resource::<balaur_ui::UiFocus>();
        let mut focus = focus.borrow_mut();
        focus.focused = Some(field);
        focus.taking = true;
    }
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, chord(egui::Key::Home, egui::Modifiers::NONE));
    for _ in 0..3 {
        pass(
            &app,
            &ctx,
            chord(egui::Key::ArrowRight, egui::Modifiers::NONE),
        );
    }
    app.engine.reparent(field, roots[1]).unwrap();
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![egui::Event::Text("-".into())]);
    consume_input(&mut app);
    let moved = balaur_ui::widget_rect(field).expect("the field drew");
    assert!(
        moved.min.y >= 100.0,
        "control: the field did not move: {moved:?}"
    );
    assert_eq!(
        text_of(&app, field),
        "abc-def",
        "the caret did not survive the move"
    );
    assert_eq!(
        focused(&app),
        Some(field),
        "the move took focus off the field"
    );
}

/// A field holding the keyboard with `options` to offer, as a city search
/// with its matches.
fn offering_field() -> (
    tempfile::TempDir,
    balaur_core::App,
    egui::Context,
    balaur_core::hecs::Entity,
) {
    field_from(toml::toml! { text = "P" widget_dropdown = { options = ["Paris", "Porto"] } })
}

/// Where each line of text a pass drew sits, by what it says.
fn texts(out: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    out.shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some((
                text.galley.text().to_owned(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            _ => None,
        })
        .collect()
}

fn said(
    app: &balaur_core::App,
    field: balaur_core::hecs::Entity,
    event: &str,
) -> Vec<balaur_script::Value> {
    balaur_core::events::delivered_from(&app.engine, field, event)
}

// A field's `options` hang under it while it holds the keyboard; the arrows
// walk them and Enter picks the one walked to, leaving the text and the
// keyboard where they were.
#[test]
fn a_field_s_options_hang_under_it_and_the_arrows_pick_one() {
    let (_dir, mut app, ctx, field) = offering_field();
    let shown = texts(&pass(&app, &ctx, vec![]));
    let below = balaur_ui::widget_rect(field).expect("it drew").max.y;
    for city in ["Paris", "Porto"] {
        let row = shown.iter().find(|(text, _)| text == city);
        assert!(
            row.is_some_and(|(_, at)| at.min.y >= below - 0.5),
            "{city} hangs under the field: {shown:?}"
        );
    }
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    pass(&app, &ctx, key(egui::Key::Enter));
    consume_input(&mut app);
    assert_eq!(
        said(&app, field, balaur_ui::OPTION_PICKED_EVENT),
        vec![balaur_script::Value::Str("Porto".into())]
    );
    assert_eq!(text_of(&app, field), "P", "the text is the script's to set");
    assert!(
        balaur_core::events::delivered_from(&app.engine, field, balaur_ui::SUBMITTED_EVENT)
            .is_empty(),
        "Enter on a row picks it rather than submitting"
    );
    assert_eq!(focused(&app), Some(field), "the keyboard stays");
    let folded = texts(&pass(&app, &ctx, vec![]));
    assert!(
        !folded.iter().any(|(text, _)| text == "Paris"),
        "a pick folds the menu: {folded:?}"
    );
}

// Escape folds the menu and keeps the keyboard; typing hangs it again.
#[test]
fn escape_folds_a_field_s_options_until_it_is_typed_into() {
    let (_dir, mut app, ctx, field) = offering_field();
    // A pass holding the keyboard, as any key after focus arrives is.
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, key(egui::Key::Escape));
    let folded = texts(&pass(&app, &ctx, vec![]));
    assert!(
        !folded.iter().any(|(text, _)| text == "Paris"),
        "{folded:?}"
    );
    consume_input(&mut app);
    assert_eq!(
        focused(&app),
        Some(field),
        "Escape folded the menu, not the field"
    );
    pass(&app, &ctx, vec![egui::Event::Text("a".into())]);
    let again = texts(&pass(&app, &ctx, vec![]));
    assert!(again.iter().any(|(text, _)| text == "Paris"), "{again:?}");
}

#[test]
fn a_press_on_a_field_s_option_picks_it() {
    let (_dir, mut app, ctx, field) = offering_field();
    let shown = texts(&pass(&app, &ctx, vec![]));
    let (_, paris) = shown
        .iter()
        .find(|(text, _)| text == "Paris")
        .expect("the menu hangs");
    let at = paris.center();
    pass(&app, &ctx, vec![egui::Event::PointerMoved(at)]);
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    consume_input(&mut app);
    assert_eq!(
        said(&app, field, balaur_ui::OPTION_PICKED_EVENT),
        vec![balaur_script::Value::Str("Paris".into())]
    );
    assert_eq!(
        focused(&app),
        Some(field),
        "the keyboard stays on the field"
    );
    assert!(
        balaur_core::events::delivered_from(&app.engine, field, balaur_ui::SUBMITTED_EVENT)
            .is_empty(),
        "a press on its menu is no leaving the field"
    );
}

// Tab folds the menu and goes where it goes from a field with nothing to
// offer: never to a row of the menu.
#[test]
fn tab_folds_a_field_s_options_and_never_reaches_a_row() {
    let (_dir, app, ctx, _field) = offering_field();
    pass(&app, &ctx, vec![]);
    let held = ctx.memory(egui::Memory::focused);
    assert!(held.is_some(), "the field holds the keyboard");
    pass(&app, &ctx, key(egui::Key::Tab));
    let after = texts(&pass(&app, &ctx, vec![]));
    assert!(!after.iter().any(|(text, _)| text == "Paris"), "{after:?}");
    let now = ctx.memory(egui::Memory::focused);
    assert!(
        now.is_none() || now == held,
        "a row took the keyboard: {now:?}"
    );
}
