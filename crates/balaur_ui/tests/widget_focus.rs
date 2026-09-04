//! What the widget layer decides about a tree rather than where it puts it:
//! focus, themes, locales and surfaces.

mod support;

use balaur::{standard_app, AppConfig};
use balaur_core::hecs::Entity;
#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use support::*;

/// Focus follows the tree that was drawn, not the arena: an accept on a
/// button under a hidden panel would fire an `on_click` nobody could see.
#[test]
fn a_button_under_a_hidden_panel_is_not_a_focus_stop() {
    let (_dir, app) = app();
    let column = add_widget(
        &app,
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 }.into(),
    );
    let panel = add_child_widget(
        &app,
        column,
        "Menu",
        &toml::toml! { kind = "panel" text = "" }.into(),
    );
    let buried = add_child_widget(
        &app,
        panel,
        "Buried",
        &toml::toml! { kind = "button" text = "Buried" }.into(),
    );
    let open = add_child_widget(
        &app,
        column,
        "Open",
        &toml::toml! { kind = "button" text = "Open" }.into(),
    );
    keyboard(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(
        focused(&app),
        Some(buried),
        "control: it is a stop while the panel is visible"
    );

    let hide = toml::toml! { visible = false };
    balaur::components::patch(&app.engine, panel, "widget", &hide.into()).unwrap();
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(
        focused(&app),
        Some(open),
        "focus landed inside a hidden panel"
    );
    pass(&app, &ctx, key(egui::Key::Enter));
    assert!(
        !clicked(&app, buried),
        "an accept activated a button nobody could see"
    );
}

/// A surface the host turned off is not somewhere focus can be either — in the
/// editor that is a played game's HUD with the widget layer switched off.
#[test]
fn a_root_on_a_disabled_surface_is_not_a_focus_stop() {
    let (_dir, app) = app();
    let (_column, buttons) = menu(&app);
    keyboard(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    {
        let config = app.engine.resource::<balaur_ui::WidgetLayerConfig>();
        config.borrow_mut().enabled = false;
    }
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), None, "focus landed on an undrawn surface");
    pass(&app, &ctx, key(egui::Key::Enter));
    assert!(
        !clicked(&app, buttons[0]),
        "an accept clicked a button on a surface nothing draws"
    );
}

#[test]
fn focus_walks_the_menu_in_scene_order_and_wraps() {
    let (_dir, app) = app();
    let (_column, buttons) = menu(&app);
    keyboard(&app);
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
    let (_dir, mut app) = app();
    let (_column, buttons) = menu(&app);
    keyboard(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    pass(&app, &ctx, key(egui::Key::ArrowDown));
    assert_eq!(focused(&app), Some(buttons[1]));

    pass(&app, &ctx, key(egui::Key::Enter));
    consume_input(&mut app);
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
    keyboard(&app);
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
    keyboard(&app);
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
    keyboard(&app);
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

/// A theme names how a kind is drawn, and a widget takes the one from the
/// nearest ancestor that has it — so a screen is themed by its root.
#[test]
fn a_theme_is_inherited_by_everything_under_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
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
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
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
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
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

/// A host that confines the default surface confines every layer it has not
/// been told about, or a scene naming `layer = "hud"` draws over the editor's
/// own chrome whether or not the game is playing.
#[test]
fn a_layer_nothing_configured_takes_the_default_surface() {
    let (_dir, app) = app();
    let hud = add_widget(
        &app,
        &toml::toml! { kind = "panel" text = "hud" x = 0.0 y = 0.0 width = 40.0 height = 20.0 layer = "unheard_of" }
            .into(),
    );
    {
        let config = app.engine.resource::<balaur_ui::WidgetLayerConfig>();
        config.borrow_mut().rect = Some([300.0, 200.0, 100.0, 100.0]);
    }
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let placed = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", hud))))
        .expect("control: the hud drew somewhere");
    assert!(
        placed.min.x >= 299.0,
        "an unconfigured layer escaped the default surface: {placed:?}"
    );

    // And its enabled flag too: turning the default off takes it with it.
    {
        let config = app.engine.resource::<balaur_ui::WidgetLayerConfig>();
        config.borrow_mut().enabled = false;
    }
    let out = pass(&app, &ctx, vec![]);
    let drew = out.shapes.iter().any(|shape| match &shape.shape {
        egui::epaint::Shape::Text(text) => text.galley.text() == "hud",
        _ => false,
    });
    assert!(
        !drew,
        "an unconfigured layer ignored the default's off switch"
    );
}
