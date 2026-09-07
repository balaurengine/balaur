//! What both widget suites need: an app, widgets on it, one egui pass,
//! and the readers that observe what the pass did.

#![allow(
    dead_code,
    unreachable_pub,
    reason = "two test binaries share this module and each uses part of it"
)]

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_core::hecs::Entity;
use egui::{Modifiers, PointerButton, Rect, pos2, vec2};

/// An app booted from an empty scene; widgets are added straight to the world.
pub fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"w\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    (dir, standard_app(config).unwrap())
}

pub fn add_widget(app: &App, params: &toml::Value) -> Entity {
    let root = app.engine.root();
    let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), "W", root);
    balaur::components::add(&app.engine, entity, "widget", Some(params)).unwrap();
    entity
}

/// One egui pass over `run_pass` with the given input. The first pass only
/// installs fonts and draws nothing, so callers spend one before asserting.
pub fn pass(app: &App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
        events,
        ..Default::default()
    };
    ctx.begin_pass(input);
    balaur_ui::run_pass(&app.engine, ctx);
    // A real renderer uploads these; dropping them unapplied is a panic.
    let mut out = ctx.end_pass();
    out.textures_delta.clear();
    out
}

pub fn press(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        },
    ]
}

/// The tick that consumes what the last pass saw.
///
/// A frame is tick-then-draw, and egui's events arrive in the draw, so a click
/// is applied by the tick after the pass that took it -- which is what lets a
/// replay run the handler with no window at all.
pub fn consume_input(app: &mut App) {
    app.tick(1.0 / 60.0);
}

pub fn clicked(app: &App, entity: Entity) -> bool {
    balaur::components::get(&app.engine, entity, "widget")
        .expect("the widget component is still on the node")
        .get("clicked")
        .and_then(toml::Value::as_bool)
        .expect("the component emits a `clicked` bool")
}

/// A widget under a container, as a scene node under a scene node. The tree
/// was always there; what changed is that the layer reads it.
pub fn add_child_widget(app: &App, parent: Entity, name: &str, params: &toml::Value) -> Entity {
    let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), name, parent);
    balaur::components::add(&app.engine, entity, "widget", Some(params)).unwrap();
    entity
}

/// Two passes to size, one to draw: a new Area is invisible on its first.
pub fn settle(app: &App, ctx: &egui::Context) {
    pass(app, ctx, vec![]);
    pass(app, ctx, vec![]);
    pass(app, ctx, vec![]);
}

/// An app whose project also holds `scripts/paint.rn`, for the `draw` kind.
pub fn app_with_script(body: &str) -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"w\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(dir.path().join("scripts/paint.rn"), body).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    (dir, standard_app(config).unwrap())
}

pub fn key(k: egui::Key) -> Vec<egui::Event> {
    vec![egui::Event::Key {
        key: k,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]
}

pub fn focused(app: &App) -> Option<Entity> {
    app.engine.resource::<balaur_ui::UiFocus>().borrow().focused
}

/// Let the keyboard drive focus, which a game has to ask for: the keys are
/// the game's until it declares the `ui_*` actions or says so here.
pub fn keyboard(app: &App) {
    app.engine
        .resource::<balaur_ui::WidgetLayerConfig>()
        .borrow_mut()
        .keyboard = true;
}

/// A menu of three buttons in a column, which is what focus is for.
pub fn menu(app: &App) -> (Entity, Vec<Entity>) {
    let column = add_widget(app, &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into());
    let buttons = ["New game", "Options", "Quit"]
        .into_iter()
        .map(|label| {
            add_child_widget(
                app,
                column,
                label,
                &toml::toml! { kind = "button" text = label }.into(),
            )
        })
        .collect();
    (column, buttons)
}
