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
    pass_at(app, ctx, events, None)
}

/// A pass at a stated clock, for what egui times: a press held past its
/// click length is a long touch.
pub fn pass_at(
    app: &App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    time: Option<f64>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
        events,
        time,
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
    press_with(pos, PointerButton::Primary, pressed)
}

pub fn press_with(pos: egui::Pos2, button: PointerButton, pressed: bool) -> Vec<egui::Event> {
    press_mod(pos, pressed, Modifiers::NONE, button)
}

/// A press with modifiers held down.
pub fn press_mod(
    pos: egui::Pos2,
    pressed: bool,
    modifiers: Modifiers,
    button: PointerButton,
) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        },
    ]
}

/// A click with modifiers held, over two passes: egui reports a click on the
/// release, and a row reads the modifiers of that frame.
///
/// They ride on an event of their own rather than on the press, which is how
/// egui tracks them, and are put down again after: the input state carries
/// them from pass to pass until something says otherwise.
pub fn click_held(app: &App, ctx: &egui::Context, at: egui::Pos2, modifiers: Modifiers) {
    for pressed in [true, false] {
        let mut events = vec![egui::Event::ModifiersChanged(modifiers)];
        events.extend(press_mod(at, pressed, modifiers, PointerButton::Primary));
        pass(app, ctx, events);
    }
    pass(
        app,
        ctx,
        vec![egui::Event::ModifiersChanged(Modifiers::NONE)],
    );
}

/// Draw at a UI scale, the way the windowed backend does it: egui's zoom is
/// the only scale there is, so a design pixel stays a point whatever it is.
pub fn set_scale(app: &App, ctx: &egui::Context, scale: f32) {
    app.engine
        .resource::<balaur_ui::UiConfig>()
        .borrow_mut()
        .scale = scale;
    ctx.set_zoom_factor(scale);
}

/// A finger down or up: the touch event a screen sends, with the pointer
/// press egui derives from it, as a winit backend delivers both.
pub fn touch(pos: egui::Pos2, down: bool) -> Vec<egui::Event> {
    let mut events = vec![egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(0),
        phase: if down {
            egui::TouchPhase::Start
        } else {
            egui::TouchPhase::End
        },
        pos,
        force: None,
    }];
    events.extend(press(pos, down));
    events
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

pub fn property(app: &App, entity: Entity, key: &str) -> toml::Value {
    balaur::components::get(&app.engine, entity, "widget")
        .expect("the widget component is still on the node")
        .get(key)
        .cloned()
        .unwrap_or_else(|| panic!("the widget has no `{key}`"))
}

/// Every text shape's caption and top-left corner, for finding a child.
pub fn texts(out: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    out.shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some((text.galley.text().to_string(), text.pos)),
            _ => None,
        })
        .collect()
}

pub fn root_rect(ctx: &egui::Context, entity: Entity) -> egui::Rect {
    ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
        .expect("the root drew")
}
