//! What a widget says once a gesture is over: a value committed, a row
//! activated or folded, a popup up and gone, a scroll moved.

use balaur_core::hecs::Entity;
use balaur_script::Value;
use egui::pos2;

use crate::support::{add_widget, app, consume_input, pass, pass_at, press, settle, texts};

/// What the widget's node heard under `event` on the last tick.
fn heard(app: &balaur::App, entity: Entity, event: &str) -> Vec<Value> {
    balaur_core::events::delivered_from(&app.engine, entity, event)
}

#[test]
fn a_slider_commits_its_value_when_the_press_lets_go() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "slider" x = 0.0 y = 0.0 width = 200.0 min = 0.0 max = 10.0 };
    let slider = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let target = pos2(150.0, 10.0);
    pass(&app, &ctx, press(target, true));
    consume_input(&mut app);
    assert!(
        heard(&app, slider, "commit").is_empty(),
        "committed while held"
    );
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    let committed = heard(&app, slider, "commit");
    assert!(
        matches!(committed.as_slice(), [Value::Num(n)] if *n > 5.0),
        "the release commits where it landed: {committed:?}"
    );
}

#[test]
fn a_dropdown_says_when_its_list_opens_and_shuts() {
    let (_dir, mut app) = app();
    let params =
        toml::toml! { kind = "dropdown" text = "One" options = ["One", "Two"] x = 0.0 y = 0.0 };
    let dropdown = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let head = pos2(20.0, 10.0);
    pass(&app, &ctx, press(head, true));
    pass(&app, &ctx, press(head, false));
    let open = pass(&app, &ctx, vec![]);
    consume_input(&mut app);
    assert_eq!(heard(&app, dropdown, "opened").len(), 1, "no opened");
    let (_, at) = texts(&open)
        .into_iter()
        .find(|(text, _)| text == "Two")
        .expect("the list opened with its options");
    let target = at + egui::vec2(6.0, 6.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    pass(&app, &ctx, vec![]);
    consume_input(&mut app);
    assert_eq!(heard(&app, dropdown, "closed").len(), 1, "no closed");
}

#[test]
fn a_double_clicked_row_is_activated() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "list" x = 0.0 y = 0.0 width = 200.0 height = 120.0
        options = ["First", "Second"]
    };
    let list = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let (_, at) = drawn
        .iter()
        .find(|(t, _)| t == "Second")
        .expect("the row drew")
        .clone();
    let on = at + egui::vec2(4.0, 4.0);
    for (i, down) in [true, false, true, false].into_iter().enumerate() {
        pass_at(&app, &ctx, press(on, down), Some(1.0 + i as f64 * 0.05));
    }
    consume_input(&mut app);
    assert_eq!(
        heard(&app, list, "activate"),
        vec![Value::Str("Second".into())]
    );
}

#[test]
fn a_tree_row_says_when_its_caret_folds_it() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 120.0
        options = ["Root", "\tChild"]
    };
    let tree = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let drawn = texts(&pass(&app, &ctx, vec![]));
    let (_, root) = drawn
        .iter()
        .find(|(t, _)| t == "Root")
        .expect("root drew")
        .clone();
    // The caret sits just left of the row's text.
    let caret = pos2(root.x - 8.0, root.y + 6.0);
    pass(&app, &ctx, press(caret, true));
    pass(&app, &ctx, press(caret, false));
    consume_input(&mut app);
    let folded = heard(&app, tree, "fold");
    let [Value::Map(said)] = folded.as_slice() else {
        panic!("no single fold: {folded:?}");
    };
    let get = |key: &str| said.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    assert_eq!(get("row"), Some(Value::Str("Root".into())));
    assert_eq!(
        get("open"),
        Some(Value::Bool(false)),
        "a first click shuts it"
    );
}

#[test]
fn a_scroll_says_where_it_moved_to() {
    let (_dir, mut app) = app();
    let scroll = add_widget(
        &app,
        &toml::toml! { kind = "scroll" x = 0.0 y = 0.0 width = 200.0 height = 100.0 gap = 0.0 }
            .into(),
    );
    for i in 0..20 {
        crate::support::add_child_widget(
            &app,
            scroll,
            &format!("line{i}"),
            &toml::toml! { kind = "label" text = "a line" height = 30.0 }.into(),
        );
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let over = pos2(100.0, 50.0);
    let wheel = vec![
        egui::Event::PointerMoved(over),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    pass(&app, &ctx, wheel);
    for _ in 0..10 {
        pass(&app, &ctx, vec![]);
    }
    consume_input(&mut app);
    let moved = heard(&app, scroll, "scrolled");
    assert!(
        moved
            .iter()
            .any(|v| matches!(v, Value::Vec2([_, y]) if *y > 0.0)),
        "no scroll down reported: {moved:?}"
    );
}

#[test]
fn a_window_that_keeps_itself_open_only_asks_to_close() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "window" text = "Debug" x = 0.0 y = 0.0 width = 200.0 height = 120.0
        hide_on_close = false
    };
    let window = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let (_, at) = texts(&pass(&app, &ctx, vec![]))
        .into_iter()
        .find(|(text, _)| text == "×")
        .expect("the title bar draws its cross");
    let cross = at + egui::vec2(3.0, 6.0);
    pass(&app, &ctx, press(cross, true));
    pass(&app, &ctx, press(cross, false));
    consume_input(&mut app);
    assert_eq!(heard(&app, window, "close_request"), vec![Value::Nil]);
    let open = balaur::components::get(&app.engine, window, "widget")
        .and_then(|w| w.get("open").and_then(toml::Value::as_bool));
    assert_eq!(open, Some(true), "the script decides whether it shuts");
}
