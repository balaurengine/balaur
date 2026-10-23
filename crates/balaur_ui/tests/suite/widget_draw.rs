//! A `draw` widget and the script that paints it: the rect it hands over,
//! the method it names, and what a script reads back a frame later.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;

/// A `draw` widget is a rect the scene places and a script fills: the node
/// owns where it is, the script owns what is in it.
#[test]
fn a_draw_widget_hands_its_rect_to_a_script() {
    let script = "pub struct Owner { node: Node }
impl Owner {
    pub fn fill() { ui::label(\"filled from a script\", map().with(\"size\", 20)); }
}
";
    let shapes = |target: &str| {
        let (_dir, app) = app_with_script(script);
        let params = toml::toml! {
            kind = "draw"
            x = 0.0
            y = 0.0
            width = 200.0
            height = 80.0
            draw = target
        };
        let entity = add_widget(&app, &params.into());
        app.engine
            .script_host()
            .unwrap()
            .attach(balaur::node_id_of(entity), "scripts/paint.bs")
            .unwrap();
        let ctx = egui::Context::default();
        pass(&app, &ctx, vec![]);
        pass(&app, &ctx, vec![]);
        let out = pass(&app, &ctx, vec![]);
        let rect = ctx.memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))));
        (out.shapes.len(), rect)
    };
    let (drawn, rect) = shapes("fill");
    let (empty, _) = shapes("");
    assert!(
        drawn > empty,
        "the script drew nothing into the rect: {drawn} shapes against {empty}"
    );
    let rect = rect.expect("the draw widget reserved a rect");
    assert!(
        (rect.width() - 200.0).abs() < 2.0 && (rect.height() - 80.0).abs() < 2.0,
        "the draw widget did not take the size it states: {rect:?}"
    );
}

/// `draw = "file:function"` calls a free function in a file, so a node that
/// only reserves a rect need carry no script instance.
#[test]
#[ignore = "needs engine: the .bs host answers no call_in, so a `file:function` target draws nothing"]
fn a_draw_widget_calls_a_function_in_a_file() {
    let script = "pub fn fill() { ui::label(\"filled from a file\", map().with(\"size\", 20)); }\n";
    let (_dir, app) = app_with_script(script);
    let params = toml::toml! {
        kind = "draw" x = 0.0 y = 0.0 width = 200.0 height = 80.0 draw = "scripts/paint.bs:fill"
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    assert!(
        texts(&out).iter().any(|(t, _)| t == "filled from a file"),
        "the function in the file drew nothing"
    );
}

/// A script's `draw_ui` reads this frame's rects, not last frame's. The
/// editor's whole shell is placed from them, and a frame behind is a frame
/// where every sheet is at 0,0.
#[test]
fn draw_ui_sees_the_rects_from_its_own_frame() {
    let script = "pub struct Watcher { node: Node, seen: float }
impl Watcher {
    pub fn draw_ui() {
        let r = ui::widget_rect(scene::get_node(\"Sheet\"));
        if r.is_some() { self.seen = r.unwrap().width; }
    }
}
";
    let (_dir, app) = app_with_script(script);
    let watcher =
        balaur::scene::spawn_node(&mut app.engine.world_mut(), "Watcher", app.engine.root());
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(watcher), "scripts/paint.bs")
        .unwrap();
    let sheet = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Sheet", app.engine.root());
    balaur::components::add(
        &app.engine,
        sheet,
        "widget",
        Some(
            &toml::toml! { kind = "panel" text = "s" x = 0.0 y = 0.0 width = 200.0 height = 40.0 }
                .into(),
        ),
    )
    .unwrap();

    let ctx = egui::Context::default();
    // The first pass installs fonts and draws nothing; the second is the
    // first that draws, and `draw_ui` should already have the rect.
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    let seen = crate::support::script_number(&app, watcher, "seen");
    assert_eq!(
        seen,
        Some(200.0),
        "draw_ui read {seen:?} for a 200 px sheet drawn in the same frame"
    );
}

/// A panel of `draw` nodes should need one script, not one each: the rect is
/// the node's, and what fills it belongs to whatever owns the state.
#[test]
fn a_draw_node_asks_the_nearest_scripted_ancestor() {
    let script = "pub struct Owner { node: Node }
impl Owner {
    pub fn fill() { ui::label(\"from the ancestor\", map().with(\"size\", 20)); }
}
";
    let (_dir, app) = app_with_script(script);
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", app.engine.root());
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(owner), "scripts/paint.bs")
        .unwrap();
    let column = add_child_widget(
        &app,
        owner,
        "column",
        &toml::toml! { kind = "column" x = 0.0 y = 0.0 width = 200.0 height = 120.0 }.into(),
    );
    add_child_widget(
        &app,
        column,
        "body",
        &toml::toml! { kind = "draw" draw = "fill" grow = 1.0 }.into(),
    );
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    let drew = out.shapes.iter().any(|shape| match &shape.shape {
        egui::epaint::Shape::Text(text) => text.galley.text() == "from the ancestor",
        _ => false,
    });
    assert!(
        drew,
        "the draw node did not reach the script two levels above it"
    );
}

/// `docs/PLAN-ui-layout.md` §5 promises one frame between a widget being
/// placed and a script reading its rect back; two would be a size change the
/// script never catches up with.
#[test]
fn a_script_reads_a_widget_rect_no_more_than_one_frame_late() {
    // `render.set_camera_2d` is the observation channel: nothing else in this
    // test writes it, and it is readable from Rust without a window.
    let script = "pub struct Owner { node: Node }
impl Owner {
    pub fn draw_ui() {
        let r = ui::widget_rect(self.node);
        if r.is_some() { render::set_camera_2d(r.unwrap().width, 0.0, 60.0); }
    }
}
";
    let (_dir, app) = app_with_script(script);
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", app.engine.root());
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(owner), "scripts/paint.bs")
        .unwrap();
    balaur::components::add(
        &app.engine,
        owner,
        "widget",
        Some(
            &toml::toml! { kind = "panel" text = "" x = 0.0 y = 0.0 width = 100.0 height = 40.0 }
                .into(),
        ),
    )
    .unwrap();
    let seen = || {
        app.engine
            .resource::<balaur::render::CameraConfig2d>()
            .borrow()
            .center[0]
    };
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    assert!(
        (seen() - 100.0).abs() < 2.0,
        "control: the script never read the panel's rect ({})",
        seen()
    );

    balaur::components::patch(
        &app.engine,
        owner,
        "widget",
        &toml::toml! { width = 300.0 }.into(),
    )
    .unwrap();
    // One pass to draw at the new width, one for the script to have read it.
    pass(&app, &ctx, vec![]);
    pass(&app, &ctx, vec![]);
    assert!(
        (seen() - 300.0).abs() < 2.0,
        "the rect a script reads is more than one frame stale ({})",
        seen()
    );
}

/// A script asking for focus is the pad's route in, so what it asks for has
/// to survive to the next draw.
#[test]
fn a_pending_move_from_a_script_is_taken_at_the_next_draw() {
    let (_dir, app) = app();
    let (_column, buttons) = menu(&app);
    keyboard(&app);
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    app.engine
        .resource::<balaur_ui::UiFocus>()
        .borrow_mut()
        .pending = Some(balaur_ui::Move::Next);
    pass(&app, &ctx, vec![]);
    assert_eq!(focused(&app), Some(buttons[0]));
}
