//! The `widget` component driven through real egui passes: what the layer
//! draws and which clicks it takes, observed through the component API.

use balaur::{standard_app, AppConfig};
use balaur_core::hecs::Entity;
use balaur_core::App;
use egui::{pos2, vec2, Modifiers, PointerButton, Rect};

/// An app booted from an empty scene; widgets are added straight to the world.
fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"w\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    (dir, standard_app(config).unwrap())
}

fn add_widget(app: &App, params: &toml::Value) -> Entity {
    let root = app.engine.root();
    let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), "W", root);
    balaur::components::add(&app.engine, entity, "widget", Some(params)).unwrap();
    entity
}

/// One egui pass over `run_pass` with the given input. The first pass only
/// installs fonts and draws nothing, so callers spend one before asserting.
fn pass(app: &App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
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

fn press(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
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

fn clicked(app: &App, entity: Entity) -> bool {
    balaur::components::get(&app.engine, entity, "widget")
        .expect("the widget component is still on the node")
        .get("clicked")
        .and_then(toml::Value::as_bool)
        .expect("the component emits a `clicked` bool")
}

#[test]
fn a_hidden_widget_draws_nothing_and_takes_no_clicks() {
    let (_dir, app) = app();
    let params = toml::toml! { kind = "button" text = "hit" x = 0.0 y = 0.0 };
    let entity = add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    pass(&app, &ctx, vec![]);
    // A new Area is sized invisibly on its first frame, so shapes come later.
    pass(&app, &ctx, vec![]);
    let shown = pass(&app, &ctx, vec![]);
    assert!(!shown.shapes.is_empty(), "control: the button drew nothing");

    let target = pos2(10.0, 10.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    assert!(
        clicked(&app, entity),
        "control: the visible button did not take the click"
    );

    let hide = toml::toml! { visible = false };
    balaur::components::patch(&app.engine, entity, "widget", &hide.into()).unwrap();
    let hidden = pass(&app, &ctx, vec![]);
    assert!(
        hidden.shapes.is_empty(),
        "a hidden widget still drew {} shapes",
        hidden.shapes.len()
    );
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    assert!(!clicked(&app, entity), "a hidden button took a click");
}

#[test]
fn a_panel_takes_an_explicit_size() {
    let (_dir, app) = app();
    let sized_params = toml::toml! { kind = "panel" text = "p" width = 200.0 height = 120.0 };
    let sized = add_widget(&app, &sized_params.into());
    let auto_params = toml::toml! { kind = "panel" text = "p" anchor = "bottom_left" };
    let auto = add_widget(&app, &auto_params.into());
    let ctx = egui::Context::default();
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);

    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
            .expect("the panel drew, so its area has a rect")
    };
    let sized_rect = rect(sized);
    assert!(
        sized_rect.width() >= 200.0 && sized_rect.height() >= 120.0,
        "the explicit size was not honoured: {sized_rect:?}"
    );
    // The control: a content-sized panel of the same text stays smaller,
    // so the explicit size, not the content, made the first one big.
    let auto_rect = rect(auto);
    assert!(
        auto_rect.width() < 200.0 && auto_rect.height() < 120.0,
        "the auto panel is as big as the sized one: {auto_rect:?}"
    );
}
