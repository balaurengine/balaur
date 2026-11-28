//! A finger a script feeds with `input::set_touch`, met by the widgets as a
//! finger the window reported: pressed, held and dragged with no egui event
//! from the test.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;
use balaur_script::Value;
use egui::pos2;

use crate::support::{
    add_child_widget, add_widget, clicked, consume_input, pass_at, root_rect, settle,
};
use crate::widget_menus::{context_scene, rows_drawn};
use crate::widget_scrolling::{as_touchscreen, list};

const FINGER: &str = "pub class Finger { node: Node }
impl Finger {
    pub fn touch(id: int, x: float, y: float, phase: str) {
        input::set_touch(id, vec2(x, y), phase);
    }
}
";

/// An app with an empty scene and a node whose script feeds a finger.
fn app_with_finger() -> (tempfile::TempDir, App, Entity) {
    app_feeding(FINGER)
}

/// The same, its script `source`, a class `Finger` with `touch`.
fn app_feeding(source: &str) -> (tempfile::TempDir, App, Entity) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(dir.path().join("scripts").join("finger.bs"), source).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();
    let root = app.engine.root();
    let finger = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Finger", root);
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(finger), "scripts/finger.bs")
        .unwrap();
    (dir, app, finger)
}

/// Finger 0 at `at`, in window pixels, through `input::set_touch`.
fn feed(app: &App, finger: Entity, at: egui::Pos2, phase: &str) {
    let args = [
        Value::Int(0),
        Value::Num(f64::from(at.x)),
        Value::Num(f64::from(at.y)),
        Value::Str(phase.into()),
    ];
    let fed = app
        .engine
        .script_host()
        .unwrap()
        .call_on(balaur::node_id_of(finger), "touch", &args);
    assert!(fed.is_some(), "the script fed no finger");
}

/// One frame for what was fed: the tick that delivers it, the pass egui
/// meets it in, and the tick that applies what the widgets heard.
fn frame(app: &mut App, ctx: &egui::Context, time: Option<f64>) -> egui::FullOutput {
    consume_input(app);
    let out = pass_at(app, ctx, vec![], time);
    consume_input(app);
    out
}

fn heard(app: &App, entity: Entity, event: &str) -> Vec<Value> {
    balaur_core::events::delivered_from(&app.engine, entity, event)
}

#[test]
fn a_fed_finger_pressed_and_lifted_on_a_button_clicks_it() {
    let (_dir, mut app, finger) = app_with_finger();
    let button = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Go" x = 0.0 y = 0.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = balaur_ui::widget_rect(button)
        .expect("the button drew")
        .center();
    feed(&app, finger, at, "start");
    frame(&mut app, &ctx, None);
    assert_eq!(
        heard(&app, button, "pointer_down"),
        vec![Value::Str("left".into())]
    );
    assert!(!clicked(&app, button), "a finger still down clicked");
    feed(&app, finger, at, "end");
    frame(&mut app, &ctx, None);
    assert_eq!(
        heard(&app, button, "pointer_up"),
        vec![Value::Str("left".into())]
    );
    assert!(clicked(&app, button), "the lifted finger did not click");
}

// Under `[input] long_press_seconds` the hold is still a press; past it,
// egui's long touch opens the menu the widget names.
#[test]
fn a_fed_finger_held_past_long_press_seconds_opens_the_named_menu() {
    let (_dir, mut app, finger) = app_with_finger();
    balaur_core::settings::set(
        &app.engine,
        "input/long_press_seconds",
        toml::Value::Float(0.3),
    );
    let target = context_scene(&app, "label");
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let at = root_rect(&ctx, target).center();
    feed(&app, finger, at, "start");
    frame(&mut app, &ctx, Some(1.0));
    frame(&mut app, &ctx, Some(1.1));
    let early = frame(&mut app, &ctx, Some(1.2));
    assert!(
        rows_drawn(&early, 173.0).is_none(),
        "a finger held under long_press_seconds opened the menu"
    );
    frame(&mut app, &ctx, Some(1.4));
    let held = frame(&mut app, &ctx, Some(1.45));
    assert!(
        rows_drawn(&held, 173.0).is_some(),
        "a fed finger held past long_press_seconds opened no menu"
    );
}

#[test]
fn a_fed_finger_dragged_up_a_scroll_moves_what_it_holds() {
    let (_dir, mut app, finger) = app_with_finger();
    let (_holder, _first, sixth) = list(&app, toml::toml! { axis = "vertical" }.into());
    as_touchscreen(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let before = balaur_ui::widget_rect(sixth).expect("the line drew").min.y;
    feed(&app, finger, pos2(20.0, 80.0), "start");
    frame(&mut app, &ctx, None);
    for step in 1..=6 {
        feed(&app, finger, pos2(20.0, 80.0 - 12.0 * step as f32), "move");
        frame(&mut app, &ctx, None);
    }
    frame(&mut app, &ctx, None);
    let moved = before - balaur_ui::widget_rect(sixth).expect("the line drew").min.y;
    assert!(moved > 40.0, "a fed finger's drag did not scroll: {moved}");
}

/// A card as a game's tile is: a column whose click its binding hears,
/// holding a picture button that takes no press and a wrapped name.
fn card(app: &App) -> (Entity, Entity, Entity) {
    let card = add_widget(
        app,
        &toml::toml! {
            kind = "column" x = 0.0 y = 0.0 width = 160.0 height = 160.0
            bindings = { rows = [{ event = "pointer_click", action = "call", target = "..", value = "on_go" }] }
        }
        .into(),
    );
    let icon = add_child_widget(
        app,
        card,
        "Icon",
        &toml::toml! { kind = "button" text = "*" interactive = false widget_button = { icon = "star" } }.into(),
    );
    let name = add_child_widget(
        app,
        card,
        "Name",
        &toml::toml! { kind = "label" text = "Word match" widget_text = { wrap = true } }.into(),
    );
    (card, icon, name)
}

const MOUSE: &str = "pub class Finger { node: Node }
impl Finger {
    pub fn touch(id: int, x: float, y: float, phase: str) {
        input::set_mouse_position(vec2(x, y));
        input::set_mouse_down(0, phase == \"start\");
    }
}
";

/// A fed tap at the centre of `at`, pressed and lifted a frame apart.
fn tap(app: &mut App, ctx: &egui::Context, feeder: Entity, at: Entity) {
    let at = balaur_ui::widget_rect(at)
        .expect("the widget drew")
        .center();
    feed(app, feeder, at, "start");
    frame(app, ctx, None);
    feed(app, feeder, at, "end");
    frame(app, ctx, None);
}

#[test]
fn a_fed_finger_on_a_card_s_picture_or_name_clicks_the_card() {
    for on_picture in [true, false] {
        let (_dir, mut app, finger) = app_with_finger();
        let (card, icon, name) = card(&app);
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        tap(&mut app, &ctx, finger, if on_picture { icon } else { name });
        assert!(clicked(&app, card), "a tap on the picture: {on_picture}");
        assert!(!clicked(&app, icon), "the picture lets the press through");
    }
}

#[test]
fn a_fed_mouse_click_on_a_card_s_picture_clicks_the_card() {
    let (_dir, mut app, mouse) = app_feeding(MOUSE);
    let (card, icon, _name) = card(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    tap(&mut app, &ctx, mouse, icon);
    assert!(clicked(&app, card));
}
