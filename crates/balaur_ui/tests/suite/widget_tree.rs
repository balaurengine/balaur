//! What the node tree says about a widget: whether it draws, how faded, and
//! where its handler runs. None of it is the widget's own component; all of it
//! is read off the nodes above.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use egui::pos2;

/// A widget draws only while its node does, as a sprite does: a panel hidden
/// with `visible = false` on its node and shown later by a script. Shown and
/// hidden across passes, so the cached arena has to notice, not a rebuild.
#[test]
fn a_widget_under_a_hidden_node_draws_nothing_until_it_is_shown() {
    let (_dir, app) = app();
    let root = app.engine.root();
    let popup = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Popup", root);
    let title = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Title", popup);
    let params = toml::toml! { kind = "label" text = "ahoy" x = 0.0 y = 0.0 };
    balaur::components::add(&app.engine, title, "widget", Some(&params.into())).unwrap();
    let ctx = egui::Context::default();
    let show = |on: bool| {
        app.engine
            .world()
            .get::<&mut balaur::scene::Appearance>(popup)
            .unwrap()
            .visible = on;
        balaur::scene::propagate_transforms(&mut app.engine.world_mut(), root);
    };
    // A new Area is sized invisibly on its first frame, so shapes come later.
    let draws = || {
        pass(&app, &ctx, vec![]);
        pass(&app, &ctx, vec![]);
        !pass(&app, &ctx, vec![]).shapes.is_empty()
    };
    show(true);
    assert!(
        draws(),
        "control: the label drew nothing while its node showed"
    );
    show(false);
    assert!(!draws(), "the label still drew under a hidden node");
    show(true);
    assert!(
        draws(),
        "the label did not come back when its node was shown"
    );

    // A fade is the tint's alpha, inherited the same way.
    let fade = |alpha: f32| {
        app.engine
            .world()
            .get::<&mut balaur::scene::Appearance>(popup)
            .unwrap()
            .tint = balaur_core::glamx::Vec4::new(1.0, 1.0, 1.0, alpha);
        balaur::scene::propagate_transforms(&mut app.engine.world_mut(), root);
    };
    fade(0.0);
    assert!(
        !draws(),
        "the label still drew under a node faded to nothing"
    );
    fade(1.0);
    assert!(
        draws(),
        "the label did not come back when its node faded in"
    );
}

/// A Godot `pressed` wired to the scene's root script: the handler lives on
/// an ancestor, and the click has to find it there.
#[test]
fn a_click_reaches_the_nearest_ancestor_whose_script_declares_the_handler() {
    let script = "pub fn init(this) {\n    this.hits = 0;\n}\n\
                  pub fn on_go(this) {\n    this.hits += 1;\n}\n\
                  pub fn hits(this) {\n    this.hits\n}\n";
    let (_dir, mut app) = app_with_script(script);
    let root = app.engine.root();
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", root);
    let host = app.engine.script_host().unwrap();
    host.attach(balaur::node_id_of(owner), "scripts/paint.rn")
        .unwrap();
    let column = add_child_widget(
        &app,
        owner,
        "Column",
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 200.0 height = 80.0 }.into(),
    );
    add_child_widget(
        &app,
        column,
        "Go",
        &toml::toml! { kind = "button" text = "Sail" on_click = "on_go" }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let target = pos2(20.0, 12.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);

    let hits = host.call_on(balaur::node_id_of(owner), "hits", &[]);
    assert_eq!(
        hits,
        Some(balaur_script::Value::Int(1)),
        "the button's own node has no script, so the click belongs to the owner's"
    );
}

/// A button's click runs its node's `pointer_click` rows, so a scene can have
/// a button call any node's script, as a Godot `pressed` connected to a
/// sibling's script did.
#[test]
fn a_clicked_widget_runs_its_pointer_click_bindings() {
    let (_dir, mut app) = app();
    let go = add_widget(
        &app,
        &toml::toml! { kind = "button" text = "Sail" x = 0.0 y = 0.0 }.into(),
    );
    let rows = toml::toml! {
        rows = [{ event = "pointer_click", action = "set_variable", target = "sailed", value = 1.0 }]
    };
    balaur::components::add(&app.engine, go, "bindings", Some(&rows.into())).unwrap();
    let declared = toml::toml! { sailed = { type = "float", value = 0.0 } };
    balaur_core::variables::declare_from_toml(&app.engine, &declared).unwrap();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let target = pos2(12.0, 10.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    let variables = app.engine.resource::<balaur_core::variables::Variables>();
    let sailed = variables
        .borrow()
        .get("sailed")
        .map(balaur_core::variables::as_num);
    assert_eq!(sailed, Some(1.0), "the click ran the row");
}

/// A widget whose value changes emits `change` from its node, which a
/// binding row answers without a script: Godot's `toggled` connected in a
/// scene.
#[test]
fn a_ticked_check_emits_change_for_its_bindings() {
    let (_dir, mut app) = app();
    let tick = add_widget(
        &app,
        &toml::toml! { kind = "check" text = "Sails" x = 0.0 y = 0.0 }.into(),
    );
    let rows = toml::toml! {
        rows = [{ event = "emitted:change", action = "add_variable", target = "flips", value = 1.0 }]
    };
    balaur::components::add(&app.engine, tick, "bindings", Some(&rows.into())).unwrap();
    let declared = toml::toml! { flips = { type = "float", value = 0.0 } };
    balaur_core::variables::declare_from_toml(&app.engine, &declared).unwrap();
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let target = pos2(8.0, 10.0);
    pass(&app, &ctx, press(target, true));
    pass(&app, &ctx, press(target, false));
    consume_input(&mut app);
    app.tick(1.0 / 60.0);
    let variables = app.engine.resource::<balaur_core::variables::Variables>();
    let flips = variables
        .borrow()
        .get("flips")
        .map(balaur_core::variables::as_num);
    assert_eq!(flips, Some(1.0), "the tick emitted `change` once");
}

/// A harness clicks with no window: `click` settles at the next tick exactly
/// as a pointer click would, and refuses a widget no pointer could click.
#[test]
fn a_click_with_no_draw_pass_runs_the_buttons_rows_and_skips_a_disabled_one() {
    let (_dir, mut app) = app();
    let go = add_widget(&app, &toml::toml! { kind = "button" text = "Sail" x = 0.0 y = 0.0 }.into());
    let shut = add_widget(&app, &toml::toml! { kind = "button" text = "Shut" disabled = true }.into());
    let rows = toml::toml! {
        rows = [{ event = "pointer_click", action = "add_variable", target = "sailed", value = 1.0 }]
    };
    for button in [go, shut] {
        balaur::components::add(&app.engine, button, "bindings", Some(&rows.clone().into())).unwrap();
    }
    let declared = toml::toml! { sailed = { type = "float", value = 0.0 } };
    balaur_core::variables::declare_from_toml(&app.engine, &declared).unwrap();
    app.tick(1.0 / 60.0);
    assert!(balaur_ui::click(&app.engine, go, false));
    assert!(!balaur_ui::click(&app.engine, shut, true), "a disabled button takes no click");
    app.tick(1.0 / 60.0);
    let variables = app.engine.resource::<balaur_core::variables::Variables>();
    let sailed = variables.borrow().get("sailed").map(balaur_core::variables::as_num);
    assert_eq!(sailed, Some(1.0), "one click, from the enabled button");
}
