//! What a widget says once a gesture is over: a value committed, a row
//! activated or folded, a popup up and gone, a scroll moved.

use balaur_core::hecs::Entity;
use balaur_script::Value;
use egui::pos2;

use crate::support::{
    add_widget, add_widget_to, app, app_with_script, consume_input, pass, pass_at, press,
    script_text, settle, texts,
};

/// What the widget's node heard under `event` on the last tick.
fn heard(app: &balaur::App, entity: Entity, event: &str) -> Vec<Value> {
    balaur_core::events::delivered_from(&app.engine, entity, event)
}

#[test]
fn a_slider_commits_its_value_when_the_press_lets_go() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "slider" x = 0.0 y = 0.0 width = 200.0 widget_range = { min = 0.0, max = 10.0 }   };
    let slider = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let target = pos2(150.0, 10.0);
    pass(&app, &ctx, press(target, true));
    consume_input(&mut app);
    assert!(
        heard(&app, slider, "committed").is_empty(),
        "committed while held"
    );
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    let committed = heard(&app, slider, "committed");
    assert!(
        matches!(committed.as_slice(), [Value::Num(n)] if *n > 5.0),
        "the release commits where it landed: {committed:?}"
    );
}

#[test]
fn a_dropdown_says_when_its_list_opens_and_shuts() {
    let (_dir, mut app) = app();
    let params = toml::toml! { kind = "dropdown" text = "One" x = 0.0 y = 0.0 widget_dropdown = { options = ["One", "Two"] } };
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
        widget_rows = { items = [{ label = "First" }, { label = "Second" }] }
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
        heard(&app, list, "activated"),
        vec![Value::Str("Second".into())]
    );
}

#[test]
fn a_tree_row_says_when_its_caret_folds_it() {
    let (_dir, mut app) = app();
    let params = toml::toml! {
        kind = "tree" x = 0.0 y = 0.0 width = 200.0 height = 120.0
        widget_rows = { items = [{ label = "Root" }, { label = "Child", depth = 1 }] }
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
    let folded = heard(&app, tree, "folded");
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
        &toml::toml! { kind = "scroll" x = 0.0 y = 0.0 width = 200.0 height = 100.0 widget_layout = { gap = [0.0, 0.0] } }
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
            source: egui::MouseWheelSource::Unknown,
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
        widget_window = { hide_on_close = false }
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
    assert_eq!(heard(&app, window, "close_requested"), vec![Value::Nil]);
    let open = balaur::components::get(&app.engine, window, "widget_window")
        .and_then(|w| w.get("open").and_then(toml::Value::as_bool));
    assert_eq!(open, Some(true), "the script decides whether it shuts");
}

/// A paste no field took is said from no node, with its text: the platform
/// clipboard cannot be read on demand, so this is how a shell hears ⌘V.
#[test]
fn a_paste_no_field_took_is_heard_with_its_text() {
    let (_dir, mut app) = app();
    let ear = add_widget(
        &app,
        &toml::toml! { kind = "label" text = "ear" x = 0.0 y = 0.0 }.into(),
    );
    balaur_core::events::subscribe(&app.engine, ear, "pasted", None);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, vec![egui::Event::Paste("[[nodes]]".to_owned())]);
    consume_input(&mut app);
    assert_eq!(
        balaur_core::events::delivered(&app.engine, "pasted"),
        vec![balaur_script::Value::Str("[[nodes]]".into())]
    );
}

// A clicked widget says `pointer_click` from its node, as a clicked world
// node does: its own script hears it unasked, with the button and itself.
#[test]
fn a_clicked_button_is_heard_as_pointer_click_by_its_own_script() {
    let script = "pub class Go { node: Node, said: str, changes: int }
impl Go {
    pub fn on_pointer_click(button: str, source: Node) {
        self.said = \"{button} on {source.name()}\";
    }
    pub fn on_value_changed(now: bool) { self.changes += 1; }
}
";
    let (_dir, mut app) = app_with_script("go.bs", script);
    let root = app.engine.root();
    let go = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Go", root);
    add_widget_to(
        &app,
        go,
        &toml::toml! { kind = "checkbox" text = "Go" x = 0.0 y = 0.0 }.into(),
    );
    let host = app.engine.script_host().unwrap();
    host.attach(balaur::node_id_of(go), "scripts/go.bs")
        .unwrap();
    let waiting =
        balaur_core::events::next(&app.engine, balaur_core::hooks::POINTER_CLICK, Some(go));
    assert!(balaur_ui::click(&app.engine, go, false));
    consume_input(&mut app);
    assert_eq!(script_text(&app, go, "said"), Some("left on Go".into()));
    assert_eq!(
        heard(&app, go, balaur_core::hooks::POINTER_CLICK),
        vec![Value::Str("left".into())],
        "and whoever awaits it hears the same name: token {waiting}"
    );
    assert!(
        heard(&app, go, "click").is_empty(),
        "the press has one name"
    );
    assert_eq!(
        crate::support::script_number(&app, go, "changes"),
        Some(1.0),
        "a widget's own event reaches its node's hook unasked, once"
    );
}

#[test]
fn a_handler_key_on_a_widget_is_refused() {
    let (_dir, app) = app();
    let root = app.engine.root();
    let node = balaur::scene::spawn_node(&mut app.engine.world_mut(), "W", root);
    let add = |component: &str, rows: toml::Value| {
        balaur::components::add(&app.engine, node, component, Some(&rows))
    };
    for key in [
        "on_click",
        "on_change",
        "on_focus",
        "on_blur",
        "on_double_clicked",
    ] {
        let mut rows = toml::map::Map::new();
        rows.insert("kind".into(), "button".into());
        rows.insert(key.into(), "pressed".into());
        let refused = add("widget", rows.into()).expect_err(key).to_string();
        assert!(refused.contains(key), "{key}: {refused}");
    }
    add("widget", toml::toml! { kind = "list" }.into()).unwrap();
    for (component, key) in [
        ("widget_list", "on_move"),
        ("widget_list", "on_drop"),
        ("widget_list", "on_mark"),
        ("widget_list", "on_fold"),
        ("widget_rows", "on_activate"),
        ("widget_text", "on_link"),
        ("widget_code", "on_gutter"),
        ("widget_field", "on_submit"),
        ("widget_field", "on_caret"),
        ("widget_range", "on_commit"),
        ("widget_color_picker", "on_commit"),
        ("widget_scroll", "on_scrolled"),
        ("widget_window", "on_opened"),
        ("widget_window", "on_closed"),
        ("widget_window", "on_close_requested"),
        ("widget_menu", "on_opened"),
        ("widget_dropdown", "on_closed"),
    ] {
        let mut rows = toml::map::Map::new();
        rows.insert(key.into(), "handler".into());
        let refused = add(component, rows.into())
            .expect_err(&format!("{component}.{key}"))
            .to_string();
        assert!(refused.contains(key), "{component}.{key}: {refused}");
    }
}
