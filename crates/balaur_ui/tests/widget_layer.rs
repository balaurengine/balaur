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

/// A widget under a container, as a scene node under a scene node. The tree
/// was always there; what changed is that the layer reads it.
fn add_child_widget(app: &App, parent: Entity, name: &str, params: &toml::Value) -> Entity {
    let entity = balaur::scene::spawn_node(&mut app.engine.world_mut(), name, parent);
    balaur::components::add(&app.engine, entity, "widget", Some(params)).unwrap();
    entity
}

/// Two passes to size, one to draw: a new Area is invisible on its first.
fn settle(app: &App, ctx: &egui::Context) {
    pass(app, ctx, vec![]);
    pass(app, ctx, vec![]);
    pass(app, ctx, vec![]);
}

#[test]
fn a_row_lays_its_children_out_along_its_own_direction() {
    let (_dir, in_a_row) = app();
    let row = add_widget(
        &in_a_row,
        &toml::toml! { kind = "row" x = 0.0 y = 0.0 gap = 10.0 }.into(),
    );
    for label in ["one", "two", "three"] {
        add_child_widget(
            &in_a_row,
            row,
            label,
            &toml::toml! { kind = "label" text = label }.into(),
        );
    }
    let ctx = egui::Context::default();
    settle(&in_a_row, &ctx);
    let row_rect = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", row))))
        .expect("the row drew");

    // A column of the same three, to compare against.
    let (_dir2, app2) = app();
    let column = add_widget(
        &app2,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 gap = 10.0 }.into(),
    );
    for label in ["one", "two", "three"] {
        add_child_widget(
            &app2,
            column,
            label,
            &toml::toml! { kind = "label" text = label }.into(),
        );
    }
    let ctx2 = egui::Context::default();
    settle(&app2, &ctx2);
    let column_rect = ctx2
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
        .expect("the column drew");

    assert!(
        row_rect.width() > column_rect.width(),
        "a row should be the wider of the two: row {row_rect:?} column {column_rect:?}"
    );
    assert!(
        column_rect.height() > row_rect.height(),
        "a column should be the taller of the two: row {row_rect:?} column {column_rect:?}"
    );
}

/// The gap is the thing a designer reaches for first, so it has to move the
/// layout rather than merely be stored.
#[test]
fn the_gap_widens_a_row() {
    let measure = |gap: f64| {
        let (_dir, app) = app();
        let row = add_widget(
            &app,
            &toml::toml! { kind = "row" x = 0.0 y = 0.0 gap = gap }.into(),
        );
        for label in ["a", "b", "c"] {
            add_child_widget(
                &app,
                row,
                label,
                &toml::toml! { kind = "label" text = label }.into(),
            );
        }
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", row))))
            .expect("the row drew")
            .width()
    };
    let tight = measure(0.0);
    let loose = measure(40.0);
    // Two gaps between three children, so 40 apiece is 80 more.
    assert!(
        loose > tight + 60.0,
        "the gap did not widen the row: {tight} then {loose}"
    );
}

/// A child is placed by its parent. Its own `x`, `y` and `anchor` are the
/// keys of a widget that stands alone, and a menu that moved when you nudged
/// one entry would not be a menu.
#[test]
fn a_childs_own_offset_is_ignored_inside_a_container() {
    let measure = |x: f64, y: f64| {
        let (_dir, app) = app();
        let column = add_widget(
            &app,
            &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
        );
        add_child_widget(
            &app,
            column,
            "entry",
            &toml::toml! { kind = "label" text = "entry" x = x y = y anchor = "bottom_right" }
                .into(),
        );
        let ctx = egui::Context::default();
        settle(&app, &ctx);
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
            .expect("the column drew")
    };
    assert_eq!(measure(0.0, 0.0), measure(300.0, 200.0));
}

#[test]
fn a_button_inside_a_container_still_takes_its_click() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 padding = 0.0 gap = 0.0 }.into(),
    );
    let button = add_child_widget(
        &app,
        column,
        "Play",
        &toml::toml! { kind = "button" text = "Play" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
        .expect("the column drew");

    let target = rect.center();
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    assert!(
        clicked(&app, button),
        "a laid-out button took no click at {target:?} inside {rect:?}"
    );
}

/// A panel with nothing in it is the panel it always was; one with children
/// lays them out. Both had to stay true, since every existing scene's panels
/// are the first kind.
#[test]
fn a_panel_with_children_grows_around_them() {
    let (_dir, app) = app();
    let bare = add_widget(&app, &toml::toml! { kind = "panel" text = "menu" }.into());
    let full = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "menu" anchor = "bottom_left" }.into(),
    );
    for label in ["New game", "Load", "Options", "Quit"] {
        add_child_widget(
            &app,
            full,
            label,
            &toml::toml! { kind = "label" text = label }.into(),
        );
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
            .expect("the panel drew")
    };
    assert!(
        rect(full).height() > rect(bare).height() + 40.0,
        "the panel did not grow around its children: bare {:?} full {:?}",
        rect(bare),
        rect(full)
    );
}

/// A grouping node with no widget of its own should not break the chain: a
/// menu is usually a panel with an empty node or two inside it.
#[test]
fn a_plain_node_between_container_and_child_is_seen_through() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
    );
    let group = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Group", column);
    let entry = add_child_widget(
        &app,
        group,
        "entry",
        &toml::toml! { kind = "label" text = "a long entry to measure" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let laid = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", column))))
        .expect("the column drew");
    // The child gets no area of its own: it is laid out, not placed.
    let child_area = ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entry))));
    assert!(
        child_area.is_none(),
        "the child was placed on its own at {child_area:?}"
    );
    assert!(laid.width() > 60.0, "the column did not adopt it: {laid:?}");
}

fn key(k: egui::Key) -> Vec<egui::Event> {
    vec![egui::Event::Key {
        key: k,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]
}

fn focused(app: &App) -> Option<Entity> {
    app.engine.resource::<balaur_ui::UiFocus>().borrow().focused
}

/// A menu of three buttons in a column, which is what focus is for.
fn menu(app: &App) -> (Entity, Vec<Entity>) {
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

#[test]
fn focus_walks_the_menu_in_scene_order_and_wraps() {
    let (_dir, app) = app();
    let (_column, buttons) = menu(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert_eq!(focused(&app), None, "nothing is focused until asked");

    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(buttons[0]), "the first entry takes it");
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(buttons[2]));
    // A menu is a ring: past the last entry is the first.
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(buttons[0]));
    pass(&app, &ctx, key(egui::Key::ArrowUp));
    assert_eq!(focused(&app), Some(buttons[2]), "and back the other way");
}

#[test]
fn accepting_the_focused_widget_is_a_click() {
    let (_dir, app) = app();
    let (_column, buttons) = menu(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(buttons[1]));

    pass(&app, &ctx, key(egui::Key::Enter));
    assert!(clicked(&app, buttons[1]), "accept did not click the focus");
    assert!(!clicked(&app, buttons[0]), "it clicked the wrong one");
}

/// Focus exists to activate something, so a widget with nothing to activate
/// is never a stop on the way to one.
#[test]
fn focus_skips_what_it_could_not_activate() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
    );
    add_child_widget(
        &app,
        column,
        "Title",
        &toml::toml! { kind = "label" text = "PAUSED" }.into(),
    );
    let button = add_child_widget(
        &app,
        column,
        "Quit",
        &toml::toml! { kind = "button" text = "Quit" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(button), "focus landed on the label");
}

/// `focusable = false` takes a candidate out; it cannot put one in, because
/// there would be nothing for an accept to do.
#[test]
fn focusable_false_is_skipped_and_a_plain_label_stays_out() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
    );
    add_child_widget(
        &app,
        column,
        "Locked",
        &toml::toml! { kind = "button" text = "Locked" focusable = false }.into(),
    );
    let open = add_child_widget(
        &app,
        column,
        "Open",
        &toml::toml! { kind = "button" text = "Open" }.into(),
    );
    add_child_widget(
        &app,
        column,
        "Note",
        &toml::toml! { kind = "label" text = "note" focusable = true }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(open));
    // Only one stop, so a second move comes back to it.
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(open));
}

/// A hidden widget keeps its state but is not somewhere focus can sit, or an
/// accept would activate something nobody can see.
#[test]
fn hiding_the_focused_widget_releases_focus() {
    let (_dir, app) = app();
    let (_column, buttons) = menu(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(buttons[0]));

    let hide = toml::toml! { visible = false };
    balaur::components::patch(&app.engine, buttons[0], "widget", &hide.into()).unwrap();
    pass(&app, &ctx, vec![]);
    assert_eq!(focused(&app), None, "focus stayed on a hidden widget");
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(buttons[1]), "and moves on to the next");
}

/// A script asking for focus is the pad's route in, so what it asks for has
/// to survive to the next draw.
#[test]
fn a_pending_move_from_a_script_is_taken_at_the_next_draw() {
    let (_dir, app) = app();
    let (_column, buttons) = menu(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    app.engine
        .resource::<balaur_ui::UiFocus>()
        .borrow_mut()
        .pending = Some(balaur_ui::Move::Next);
    pass(&app, &ctx, vec![]);
    assert_eq!(focused(&app), Some(buttons[0]));
}

/// A theme names how a kind is drawn, and a widget takes the one from the
/// nearest ancestor that has it — so a screen is themed by its root.
#[test]
fn a_theme_is_inherited_by_everything_under_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    // A padding a long way from the built-in 8, so the panel's size says
    // whether the theme reached it.
    std::fs::write(
        dir.path().join("themes/big.toml"),
        "type = \"widget_theme\"\n\n[panel]\npadding = 40\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();

    let plain = add_widget(&app, &toml::toml! { kind = "panel" text = "p" }.into());
    let themed = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "p" anchor = "bottom_left" theme = "themes/big.toml" }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
            .expect("the panel drew")
    };
    assert!(
        rect(themed).height() > rect(plain).height() + 40.0,
        "the theme's padding did not reach the panel: plain {:?} themed {:?}",
        rect(plain),
        rect(themed)
    );
}

/// A kind the theme is silent about keeps the look it always had, which is
/// what lets a three-line theme restyle buttons alone.
#[test]
fn a_kind_the_theme_does_not_mention_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/buttons.toml"),
        "type = \"widget_theme\"\n\n[button]\nradius = 2\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();

    let plain = add_widget(&app, &toml::toml! { kind = "panel" text = "p" }.into());
    let themed = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "p" anchor = "bottom_left" theme = "themes/buttons.toml" }
            .into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = |entity: Entity| {
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
            .expect("the panel drew")
    };
    assert_eq!(
        rect(plain).size(),
        rect(themed).size(),
        "a button-only theme changed the panel"
    );
}

/// A widget showing a key follows the locale, and follows it *now*: the
/// caption is resolved every frame, so a switch shows on the next one without
/// anything having to be told.
///
/// Measured shrinking rather than growing on purpose. `area_rect` reports an
/// Area that got smaller and does not report one that got bigger under this
/// harness, so the long string is the one the run starts in — the assertion
/// is about the caption changing, and this is the direction that can see it.
#[test]
fn a_text_key_follows_the_locale() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("strings")).unwrap();
    std::fs::write(
        dir.path().join("strings/en.toml"),
        "\"menu.play\" = \"An English caption long enough to measure\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("strings/ro.toml"),
        "\"menu.play\" = \"Joaca\"\n",
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = standard_app(config).unwrap();

    let label = add_widget(
        &app,
        &toml::toml! { kind = "label" text_key = "menu.play" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let width = || {
        ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", label))))
            .expect("the widget drew")
            .width()
    };
    let english = width();
    assert!(
        english > 100.0,
        "control: the key did not resolve ({english})"
    );

    balaur_core::strings::set_locale(&app.engine, "ro");
    settle(&app, &ctx);
    assert!(
        width() < english - 40.0,
        "the caption did not follow the locale: {english} then {}",
        width()
    );
}
