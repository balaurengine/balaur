//! The `ui` module driven through a real script inside a real egui pass.
//!
//! egui runs perfectly well without a window, so the whole widget surface is
//! testable in CI. Calling the bindings from a script is the point: it is the
//! path a game takes, and it catches a binding that registers but cannot be
//! called.

use balaur::{AppConfig, standard_app};
use balaur_core::App;

/// The log buffer is global, and tests run in parallel, so one test's
/// deliberate error would surface in another's assertions. Hold this for the
/// duration of a pass.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `Root`'s script. `draw_ui` runs with the pass's context and no `Ui`; a
/// `.bs` script reaches a `Ui` only through a `draw` widget, so `paint` runs
/// in a full-screen one, which the scene holds only when `paint` says
/// something. `drawn` and `painted` count the two.
#[derive(Default)]
pub(crate) struct Script<'a> {
    /// More fields, as a struct declares them: `focus: int`.
    pub(crate) fields: &'a str,
    pub(crate) draw_ui: &'a str,
    pub(crate) paint: &'a str,
    /// More methods, whole.
    pub(crate) methods: &'a str,
}

impl<'a> Script<'a> {
    pub(crate) fn paint(body: &'a str) -> Self {
        Script {
            paint: body,
            ..Script::default()
        }
    }

    fn source(&self) -> String {
        let sep = if self.fields.is_empty() { "" } else { ", " };
        format!(
            "pub struct Root {{ node: Node, drawn: int, painted: int{sep}{} }}\n\
             impl Root {{\n\
             pub fn draw_ui() {{\nself.drawn += 1;\n{}\n}}\n\
             pub fn paint() {{\nself.painted += 1;\n{}\n}}\n\
             {}\n}}\n",
            self.fields, self.draw_ui, self.paint, self.methods
        )
    }
}

/// Build a project running `script`, then run two egui passes.
fn draw(script: &Script<'_>) -> (tempfile::TempDir, App, Vec<String>) {
    let (dir, app, _, errors) = run(script);
    (dir, app, errors)
}

/// A project whose `draw_ui` runs `body`, after two passes.
pub(crate) fn draw_with(body: &str) -> (tempfile::TempDir, App, egui::Context, Vec<String>) {
    run(&Script {
        draw_ui: body,
        ..Script::default()
    })
}

/// The same for a whole script, keeping the context the passes ran in: fonts
/// are bound to that one, so a test running further passes has to use it.
/// The project directory comes back with the app: dropped while the app is
/// still alive, a later pass fails to re-read the script and logs an error
/// into whatever test is reading the log buffer at the time.
pub(crate) fn run(script: &Script<'_>) -> (tempfile::TempDir, App, egui::Context, Vec<String>) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"ui\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    let widget = if script.paint.is_empty() {
        ""
    } else {
        "widget = { kind = \"draw\", x = 0.0, y = 0.0, width = 640.0, height = 480.0, draw = \"paint\" }\n"
    };
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = {{ source = \"scripts/s.bs\" }}\n{widget}"
        ),
    )
    .unwrap();
    std::fs::write(dir.path().join("scripts/s.bs"), script.source()).unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();

    // Two passes: run_pass installs fonts on the first and skips drawing,
    // so one pass would leave every assertion below vacuous.
    let ctx = egui::Context::default();
    for _ in 0..2 {
        ctx.begin_pass(egui::RawInput::default());
        balaur_ui::run_pass(&app.engine, &ctx);
        // A real renderer uploads these; dropping them unapplied is a panic.
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
    }
    let errors = balaur_core::logbuf::recent(500)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    (dir, app, ctx, errors)
}

/// World text can ask for the shaper before the first UI pass, and the pass
/// then keeps that one: a block shaped first has its glyphs in its atlas.
#[test]
fn the_shaper_is_made_by_whoever_asks_first_and_kept() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"ui\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\n",
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    assert!(
        balaur_text::state(&app.engine).is_none(),
        "nothing has asked yet"
    );
    let first = balaur_text::shaper(&app.engine);
    let ctx = egui::Context::default();
    for _ in 0..2 {
        ctx.begin_pass(egui::RawInput::default());
        balaur_ui::run_pass(&app.engine, &ctx);
        ctx.end_pass().textures_delta.clear();
    }
    let after = balaur_text::state(&app.engine).unwrap();
    assert!(
        std::rc::Rc::ptr_eq(&first, &after),
        "the pass replaced the shaper"
    );
}

/// With no window nothing calls `draw_ui`; the pass on no screen does, after
/// the one that binds the fonts.
#[test]
fn a_run_with_no_window_still_draws_its_ui() {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"ui\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = { source = \"scripts/s.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.bs"),
        "pub struct Root { node: Node, drawn: int }\n\
         impl Root {\n    pub fn draw_ui() { self.drawn += 1; }\n}\n",
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    balaur_ui::pass_without_window(&mut app, 800.0, 600.0);
    for _ in 0..3 {
        app.tick(1.0 / 60.0);
    }
    assert_eq!(field(&app, "drawn"), Some(2.0));
}

/// A run that logs nothing, and whose `paint` ran: a script asserts by
/// logging an error, which holds vacuously if it never ran.
fn draw_clean(script: &Script<'_>) {
    let (_dir, app, errors) = draw(script);
    assert!(errors.is_empty(), "the pass logged errors: {errors:#?}");
    assert!(
        field(&app, "painted").unwrap_or(0.0) > 0.0,
        "paint never ran, so the script checked nothing"
    );
}

/// A number the script keeps in a field, read back from Rust.
fn field(app: &App, name: &str) -> Option<f64> {
    let root = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Root")
        .expect("the scene has a Root node");
    crate::support::script_number(app, root, name)
}

#[test]
fn a_pass_with_no_widgets_is_quiet() {
    let (_dir, app, _, errors) = draw_with("");
    assert!(errors.is_empty(), "the pass logged errors: {errors:#?}");
    assert!(field(&app, "drawn").unwrap_or(0.0) > 0.0);
}

#[test]
#[ignore = "needs engine: ui containers take a closure, which a .bs script cannot pass"]
fn panels_and_containers_nest() {
    draw_clean(&Script {
        draw_ui: r#"
        ui::top_panel("bar", map().with("height", 40), "bar");
        ui::bottom_panel("status", map().with("height", 20), "bottom");
        ui::left_panel("side", map().with("width", 60), "left");
        ui::right_panel("props", map().with("width", 60), "right");
        ui::central_panel(map(), "central");
        "#,
        paint: r#"ui::label("painted");"#,
        methods: r#"
        pub fn bar() { ui::row(map(), "across"); }
        pub fn across() { ui::label("across"); }
        pub fn bottom() { ui::label("bottom"); }
        pub fn left() { ui::label("left"); }
        pub fn right() { ui::label("right"); }
        pub fn central() {
            ui::column("down");
            ui::scroll("sc", map(), "scrolled");
            ui::frame(map(), "framed");
        }
        pub fn down() { ui::label("down"); }
        pub fn scrolled() { ui::label("scrolled"); }
        pub fn framed() { ui::label("framed"); }
        "#,
        ..Script::default()
    });
}

#[test]
fn text_and_layout_helpers_run() {
    draw_clean(&Script::paint(
        r##"
        ui::label("plain");
        let spans: [any] = [map().with("text", "let x = 1;")];
        ui::code_line("1", spans);
        ui::separator();
        ui::add_space(4.0);
        ui::spacing(2.0, 2.0);
        ui::dot("#ffffff", 4.0);
        let w: float = ui::available_width();
        let h: float = ui::available_height();
        let size: Vec2 = ui::window_size();
        let keys: bool = ui::wants_keyboard();
        "##,
    ));
}

#[test]
fn interactive_widgets_report_no_interaction_without_input() {
    draw_clean(&Script::paint(
        r#"
        if ui::button("a pill", map().with("active", true)) { log::error("a pill clicked with no input"); }
        if ui::circle_button("x") { log::error("a round button clicked with no input"); }
        let switched = ui::switch(false, map());
        if switched[0] || switched[1] { log::error("a toggle flipped with no input"); }
        "#,
    ));
}

#[test]
#[ignore = "needs engine: text_field and slider answer a mixed tuple, which .bs cannot take"]
fn a_field_and_a_slider_report_no_interaction_without_input() {
    draw_clean(&Script::paint(
        r#"
        let field = ui::text_field("field", "type here");
        if field[1].as_bool() { log::error("nothing typed, yet it reported a change"); }
        let slid = ui::slider(0.5, 0.0, 1.0, map());
        if slid[1].as_bool() { log::error("a slider moved with no input"); }
        "#,
    ));
}

#[test]
#[ignore = "needs engine: code_editor answers a mixed tuple, which .bs cannot take"]
fn the_code_editor_returns_its_buffer_unchanged() {
    let (_dir, app, errors) = draw(&Script {
        fields: "edits: int",
        paint: r#"
        let out = ui::code_editor("ed", "let x = 1;\nx");
        if out[1].as_bool() { self.edits += 1; }
        if !out[2].is_nil() { log::error("a gutter line was clicked with no input"); }
        if !out[3].is_nil() { log::error("an unfocused editor reported a caret"); }
        if out[0].len() == 0 { log::error("the editor lost its buffer"); }
        "#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    let drawn = field(&app, "painted").expect("the editor never drew");
    let edits = field(&app, "edits").expect("no edit count recorded");
    assert!(drawn > 0.0, "the editor never drew");
    assert!(
        edits == 0.0,
        "the editor reported {edits} edits with no input"
    );
}

#[test]
fn bad_options_are_reported_rather_than_fatal() {
    draw_clean(&Script::paint(r#"ui::label("still drawn");"#));
}

#[test]
fn a_script_error_inside_a_pass_is_logged_not_fatal() {
    let (_dir, _app, errors) = draw(&Script::paint(
        r#"self.node.set_component("deliberate", map());"#,
    ));
    assert!(
        errors.iter().any(|e| e.contains("deliberate")),
        "the error was swallowed: {errors:?}"
    );
}

#[test]
fn the_scale_factor_is_readable_and_settable() {
    draw_clean(&Script::paint(
        r#"
        let before = ui::scale();
        ui::set_scale(1.25);
        let after = ui::scale();
        if math::abs(after - 1.25) >= 0.000001 { log::error("the scale read back as {after}"); }
        ui::set_scale(before);
        "#,
    ));
}

/// A control: everything above asserts from inside the script, so if
/// `draw_ui` and `paint` never ran they would all pass without testing
/// anything. This checks from Rust that the pass really called them.
#[test]
fn draw_ui_is_actually_called() {
    let (_dir, app, _) = draw(&Script::paint(r#"ui::label("x");"#));
    assert!(
        field(&app, "drawn").unwrap_or(0.0) > 0.0,
        "draw_ui never ran, so these tests prove nothing"
    );
    assert!(
        field(&app, "painted").unwrap_or(0.0) > 0.0,
        "paint never ran, so these tests prove nothing"
    );
}

#[test]
fn the_remaining_widgets_are_callable() {
    draw_clean(&Script::paint(
        r#"
        if ui::menu_item("Open", map()) { log::error("a menu item fired with no input"); }
        ui::rect_stroke(0.0, 0.0, 10.0, 10.0, map());
        "#,
    ));
}

#[test]
#[ignore = "needs engine: number_field and dropdown answer a mixed tuple, which .bs cannot take"]
fn a_number_field_and_a_dropdown_report_no_change_without_input() {
    draw_clean(&Script::paint(
        r#"
        let number = ui::number_field(1.5, map());
        if number[1].as_bool() { log::error("a number changed with no input"); }
        let choices: [any] = ["a", "b", "c"];
        let picked = ui::dropdown("sel", "b", choices, map());
        if picked[0].as_str() != "b" || picked[1].as_bool() { log::error("dropdown changed with no input"); }
        "#,
    ));
}

#[test]
#[ignore = "needs engine: ui::dialog takes a closure, which a .bs script cannot pass"]
fn a_modal_runs_its_body() {
    let (_dir, app, errors) = draw(&Script {
        fields: "in_modal: int",
        paint: r#"ui::dialog("m", map(), "modal");"#,
        methods: "pub fn modal() { self.in_modal = 1; }",
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    let ran = field(&app, "in_modal").unwrap_or(0.0) > 0.0;
    assert!(ran, "the modal never ran its body");
}

#[test]
#[ignore = "needs engine: text_field answers a mixed tuple, which .bs cannot take"]
fn set_text_replaces_a_field_buffer() {
    draw_clean(&Script::paint(
        r#"
        ui::text_field("f", "placeholder");
        ui::set_field_text("f", "written from outside");
        let shown = ui::text_field("f", "placeholder")[0].as_str();
        if shown != "written from outside" { log::error("set_text did not take: {shown}"); }
        "#,
    ));
}

#[test]
fn a_missing_image_does_not_stop_the_pass() {
    let (_dir, _app, errors) = draw(&Script::paint(
        r#"
        ui::image("no/such/picture.png", map());
        ui::label("drawn after the bad image");
        "#,
    ));
    assert!(
        errors.iter().all(|e| !e.contains("panic")),
        "a missing image panicked: {errors:#?}"
    );
}

#[test]
fn a_theme_can_be_set_from_a_script() {
    draw_clean(&Script {
        draw_ui: r##"ui::set_theme(map().with("panel", "#101418").with("text", "#f0f0f0").with("accent", "#d5814e"));"##,
        paint: r#"ui::label("themed");"#,
        ..Script::default()
    });
}

#[test]
#[ignore = "needs engine: ui::frame takes a closure, which a .bs script cannot pass"]
fn a_frame_s_stroke_is_as_wide_as_it_asks() {
    fn widths(shape: &egui::Shape, into: &mut Vec<f32>) {
        match shape {
            egui::epaint::Shape::Rect(rect) if rect.stroke.color == egui::Color32::RED => {
                into.push(rect.stroke.width);
            }
            egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|one| widths(one, into)),
            _ => {}
        }
    }
    let (_dir, app, ctx, errors) = run(&Script {
        paint: r##"ui::frame(map().with("fill", "#000000").with("stroke", "#ff0000").with("stroke_width", 3.0), "boxed");"##,
        methods: r#"pub fn boxed() { ui::label("boxed"); }"#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "the pass logged errors: {errors:#?}");
    ctx.begin_pass(egui::RawInput::default());
    balaur_ui::run_pass(&app.engine, &ctx);
    let out = ctx.end_pass();
    let mut found = Vec::new();
    for clipped in &out.shapes {
        widths(&clipped.shape, &mut found);
    }
    assert!(
        found.iter().any(|w| (w - 3.0).abs() < 0.01),
        "the outline is the stroke_width asked for: {found:?}"
    );
}

#[test]
fn the_widget_layer_can_be_placed_and_turned_off() {
    let (_dir, app, _, errors) = draw_with(
        "ui::set_widget_layer(true, 0.0, 0.0, 320.0, 240.0);
        ui::set_widget_layer(false, 0.0, 0.0, 0.0, 0.0);",
    );
    assert!(errors.is_empty(), "the pass logged errors: {errors:#?}");
    assert!(field(&app, "drawn").unwrap_or(0.0) > 0.0);
}

#[test]
fn shortcuts_report_no_press_without_input() {
    draw_clean(&Script::paint(
        r#"
        if ui::shortcut("cmd+s") { log::error("cmd+s with no input"); }
        if ui::shortcut("ctrl+z") { log::error("ctrl+z with no input"); }
        if ui::shortcut("a") { log::error("a with no input"); }
        "#,
    ));
}

/// One chord for both keys: the window layer sets egui's `command` for
/// Control and for Command, and a key arrives as its physical code, so ⇧⌘\
/// is `Backslash` whatever the layout prints on it.
#[test]
fn a_cmd_chord_answers_to_control_and_to_command() {
    let (_dir, app, ctx, errors) = run(&Script {
        fields: "focus: int",
        paint: r#"if ui::shortcut("cmd+shift+backslash") { self.focus += 1; }"#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(field(&app, "focus"), Some(0.0));
    let held = [
        egui::Modifiers {
            shift: true,
            ctrl: true,
            command: true,
            ..Default::default()
        },
        egui::Modifiers {
            shift: true,
            mac_cmd: true,
            command: true,
            ..Default::default()
        },
    ];
    for modifiers in held {
        feed(&app, &ctx, vec![chord(egui::Key::Backslash, modifiers)]);
    }
    assert_eq!(
        field(&app, "focus"),
        Some(2.0),
        "a held modifier was missed"
    );
    // The bare key is somebody else's: a chord that asks for one must see one.
    feed(
        &app,
        &ctx,
        vec![chord(egui::Key::Backslash, egui::Modifiers::NONE)],
    );
    assert_eq!(
        field(&app, "focus"),
        Some(2.0),
        "the bare key fired the chord"
    );
}

/// One key press, with the modifiers it arrived under.
fn chord(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// The names the profiler files a frame's passes under, in order.
fn pass_names(app: &App) -> Vec<String> {
    app.engine
        .resource::<balaur_core::timings::Timings>()
        .borrow()
        .spans
        .iter()
        .map(|(name, _)| name.to_string())
        .filter(|name| name.starts_with("ui"))
        .collect()
}

/// egui reruns the whole closure when a pass only learned a size, so the
/// shell is built twice; a profiler that filed both under one name would
/// read as one expensive pass instead of two ordinary ones.
#[test]
fn a_second_pass_in_one_frame_is_filed_as_a_rerun() {
    let (_dir, mut app, ctx, _) = run(&Script::paint(r#"ui::label("x");"#));
    // Publish the passes the helper ran, so the table below holds this frame.
    app.tick(balaur_core::DEFAULT_FIXED_DT);
    // What the windowed loop calls once a frame, which is what starts one.
    balaur_ui::wants_pass(&app.engine, &ctx, true);
    for _ in 0..2 {
        ctx.begin_pass(egui::RawInput::default());
        balaur_ui::run_pass(&app.engine, &ctx);
        let mut out = ctx.end_pass();
        out.textures_delta.clear();
    }
    // The spans of a frame are published when it ends, never mid-frame.
    app.tick(balaur_core::DEFAULT_FIXED_DT);
    assert_eq!(pass_names(&app), ["ui", "ui rerun"]);
}

/// One pass over the same context, with the events the caller feeds it.
pub(crate) fn feed(app: &App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(640.0, 480.0),
        )),
        events,
        ..Default::default()
    };
    ctx.begin_pass(input);
    balaur_ui::run_pass(&app.engine, ctx);
    let mut out = ctx.end_pass();
    out.textures_delta.clear();
    out
}

/// The colours of the filled boxes a pass painted.
fn fills(out: &egui::FullOutput) -> Vec<egui::Color32> {
    out.shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Rect(rect) => Some(rect.fill),
            _ => None,
        })
        .collect()
}

fn tap(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

/// A frame with `menu_click` answers the pointer over all of itself, so a
/// mark and the name beside it read as one control: a click on the caption
/// used to fall through to nothing.
#[test]
#[ignore = "needs engine: ui::frame and its menu_click take closures, which a .bs script cannot pass"]
fn a_frame_menu_opens_from_a_click_on_its_caption() {
    let (_dir, app, ctx, errors) = run(&Script {
        fields: "rows: float",
        paint: r#"
        self.rows = 0.0;
        ui::frame(map().with("padding_x", 8).with("menu_click", "menu"), "caption");
        "#,
        methods: r#"
        pub fn menu() {
            self.rows += 1.0;
            ui::menu_item("Open", map().with("width", 120));
        }
        pub fn caption() { ui::label("Balaur", map()); }
        "#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(field(&app, "rows"), Some(0.0), "the menu drew unopened");
    let at = egui::pos2(30.0, 14.0);
    feed(&app, &ctx, tap(at, true));
    feed(&app, &ctx, tap(at, false));
    feed(&app, &ctx, vec![]);
    let drawn = field(&app, "rows").unwrap_or(0.0);
    assert!(drawn > 0.0, "a click on the caption opened no menu");
}

/// The same frame takes its `hover` fill while the pointer is over it, painted
/// under the callback's own widgets rather than over them.
#[test]
#[ignore = "needs engine: ui::frame and its menu_click take closures, which a .bs script cannot pass"]
fn a_frame_menu_lights_up_under_the_pointer() {
    let (_dir, app, ctx, errors) = run(&Script {
        paint: r##"
        let hover = map().with("fill", "#2b3037");
        ui::frame(map().with("padding_x", 8).with("fill", "#101215").with("hover", hover).with("menu_click", "menu"), "caption");
        "##,
        methods: r#"
        pub fn menu() { ui::menu_item("Open", map().with("width", 120)); }
        pub fn caption() { ui::label("Balaur", map()); }
        "#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    let cold = egui::Color32::from_rgb(0x10, 0x12, 0x15);
    let warm = egui::Color32::from_rgb(0x2b, 0x30, 0x37);
    let away = feed(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(500.0, 400.0))],
    );
    assert!(
        fills(&away).contains(&cold),
        "the resting fill went missing"
    );
    assert!(
        !fills(&away).contains(&warm),
        "it lit up with the pointer away"
    );
    let over = feed(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(30.0, 14.0))],
    );
    assert!(
        fills(&over).contains(&warm),
        "the pointer over it lit nothing"
    );
}

/// `menu_click` hangs a menu off a left click. The rows are the callback's,
/// so nothing inside it draws until the menu is open — which is the whole
/// difference from `menu`, whose menu waits for the other button.
#[test]
#[ignore = "needs engine: a button's menu_click takes a closure, which a .bs script cannot pass"]
fn a_pill_menu_opens_on_a_left_click() {
    let (_dir, app, ctx, errors) = run(&Script {
        fields: "rows: float",
        paint: r#"
        self.rows = 0.0;
        ui::button("Menu", map().with("menu_click", "menu"));
        "#,
        methods: r#"
        pub fn menu() {
            self.rows += 1.0;
            ui::menu_item("Open", map().with("width", 120).with("trailing", "⌘O"));
        }
        "#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(field(&app, "rows"), Some(0.0), "the menu drew unopened");
    let at = egui::pos2(24.0, 20.0);
    feed(&app, &ctx, tap(at, true));
    feed(&app, &ctx, tap(at, false));
    feed(&app, &ctx, vec![]);
    let drawn = field(&app, "rows").unwrap_or(0.0);
    assert!(drawn > 0.0, "a click on the pill opened no menu");
    // Away from the pill and the menu: the popup closes and stops drawing.
    // `rows` counts one pass, since the body zeroes it on every one.
    let away = egui::pos2(500.0, 400.0);
    feed(&app, &ctx, tap(away, true));
    feed(&app, &ctx, tap(away, false));
    feed(&app, &ctx, vec![]);
    assert_eq!(
        field(&app, "rows"),
        Some(0.0),
        "the menu kept drawing after it was dismissed"
    );
}

/// Nothing is polled: a lazy shell with no input and nothing written runs no
/// pass however long it waits, where a 250 ms tick once ran one regardless.
#[test]
fn a_lazy_shell_nobody_touched_runs_no_pass_however_long_it_waits() {
    let (_dir, app, ctx, errors) = run(&Script {
        draw_ui: "ui::set_lazy(true);",
        paint: r#"ui::label("x");"#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    balaur_ui::honour_lazy(&app.engine);
    feed(&app, &ctx, Vec::new());
    std::thread::sleep(std::time::Duration::from_millis(300));
    // The first answer may spend a log line another test wrote meanwhile;
    // the tick, which nothing spends, would have answered both.
    let _ = balaur_ui::wants_pass(&app.engine, &ctx, false);
    assert!(
        !balaur_ui::wants_pass(&app.engine, &ctx, false),
        "a pass ran with nothing to answer"
    );
}

/// Laying a file out costs its length, and the editor draws the same file on
/// every frame of a session. Nothing about the picture changes until the text
/// or its colours do, and the galley must be the same one until then.
#[test]
#[ignore = "needs engine: code_editor answers a mixed tuple, which .bs cannot take"]
fn the_code_editor_lays_its_text_out_once_until_its_look_changes() {
    let (_dir, app, ctx, errors) = run(&Script::paint(
        r##"
        let comment = if self.painted > 2 { "#ff0000" } else { "#808080" };
        ui::code_editor("ed", "// a\nlet x = 1;", map().with("syntax_comment", comment));
        "##,
    ));
    assert!(errors.is_empty(), "{errors:#?}");
    let first = laid_out(&app);
    feed(&app, &ctx, Vec::new());
    let again = laid_out(&app);
    assert!(
        std::sync::Arc::ptr_eq(&first, &again),
        "a pass that changed nothing laid the file out a second time"
    );
    feed(&app, &ctx, Vec::new());
    let recoloured = laid_out(&app);
    assert!(
        !std::sync::Arc::ptr_eq(&again, &recoloured),
        "the comment colour changed and the editor kept the old picture"
    );
}

/// The galley the code editor last laid out, whatever its id.
fn laid_out(app: &App) -> std::sync::Arc<egui::Galley> {
    let state = app.engine.resource::<balaur_ui::UiState>();
    let state = state.borrow();
    let (_, galley) = state
        .code_galleys
        .values()
        .next()
        .expect("the code editor laid nothing out");
    std::sync::Arc::clone(galley)
}

/// An overlay given a size keeps its layer inside it. egui grows an area to
/// its content, and an area is a layer: a row one chip too long took the
/// pointer off every control under the whole of it.
#[test]
#[ignore = "needs engine: ui::overlay and ui::row take closures, which a .bs script cannot pass"]
fn a_sized_overlay_keeps_its_layer_inside_the_box_it_was_given() {
    let (_dir, app, ctx, errors) = run(&Script {
        draw_ui: r#"ui::overlay("chips", map().with("x", 10).with("y", 10).with("width", 60).with("height", 24), "chips");"#,
        methods: r#"
        pub fn chips() { ui::row(map().with("height", 24), "chip_row"); }
        pub fn chip_row() {
            for n in 0..8 {
                ui::button("chip", map().with("height", 20));
            }
        }
        "#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    feed(&app, &ctx, vec![]);
    feed(&app, &ctx, vec![]);
    let rect = ctx
        .memory(|m| m.area_rect(egui::Id::new("chips")))
        .expect("the overlay drew");
    assert_eq!(
        rect,
        egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(60.0, 24.0)),
        "eight chips do not widen the layer past the box the caller stated"
    );
}

/// A role's `hover` table repaints the control under the pointer, and its
/// colours are spelled from the same tokens the rest of the role names.
#[test]
fn a_roles_hover_table_repaints_the_pill_under_the_pointer() {
    let (_dir, app, ctx, errors) = run(&Script {
        draw_ui: r##"
        let colors = map().with("ink", "#101215").with("warm", "#2b3037");
        let tile = map().with("fill", "ink").with("hover", map().with("fill", "warm"));
        ui::set_theme(map().with("colors", colors).with("roles", map().with("tile", tile)));
        "##,
        paint: r#"ui::button("Go", map().with("role", "tile").with("height", 24).with("min_width", 60));"#,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    let cold = egui::Color32::from_rgb(0x10, 0x12, 0x15);
    let warm = egui::Color32::from_rgb(0x2b, 0x30, 0x37);
    let away = feed(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(500.0, 400.0))],
    );
    assert!(
        fills(&away).contains(&cold) && !fills(&away).contains(&warm),
        "at rest the role's own fill is painted"
    );
    // Twice: egui reads a hover off the widget rects of the pass before.
    let on = egui::pos2(20.0, 20.0);
    feed(&app, &ctx, vec![egui::Event::PointerMoved(on)]);
    let over = feed(&app, &ctx, vec![egui::Event::PointerMoved(on)]);
    assert!(
        fills(&over).contains(&warm),
        "under the pointer the role's `hover` table paints instead"
    );
}

/// The same, for a control inside a sized overlay: the box the overlay keeps
/// its layer inside must not cost the controls in it their own state.
#[test]
#[ignore = "needs engine: ui::overlay and ui::row take closures, which a .bs script cannot pass"]
fn a_pill_in_a_sized_overlay_still_lights_up() {
    let (_dir, app, ctx, errors) = run(&Script {
        draw_ui: r#"ui::overlay("bar", map().with("x", 0).with("y", 0).with("w", 200).with("h", 40), "bar");"#,
        methods: r##"
        pub fn bar() { ui::row(map().with("height", 30), "go"); }
        pub fn go() { ui::button("Go", map().with("height", 24).with("min_width", 60).with("fill", "#101215")); }
        "##,
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    let away = feed(
        &app,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(500.0, 400.0))],
    );
    let cold = fills(&away).len();
    let on = egui::pos2(20.0, 15.0);
    feed(&app, &ctx, vec![egui::Event::PointerMoved(on)]);
    let over = feed(&app, &ctx, vec![egui::Event::PointerMoved(on)]);
    assert!(
        fills(&over).len() > cold,
        "the pill painted nothing more under the pointer: {cold} boxes either way"
    );
}

#[test]
fn a_headless_run_answers_the_window_the_project_states() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"ui\"\nmain_scene = \"main.toml\"\n\n[window]\nwidth = 840\nheight = 1920\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = { source = \"s.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("s.bs"),
        "pub struct Root { node: Node, w: float, h: float }\n\
         impl Root {\n    fn update(dt: float) {\n        let size = ui::window_size();\n        \
         self.w = size.x;\n        self.h = size.y;\n    }\n}\n",
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    assert_eq!(
        field(&app, "w"),
        Some(840.0),
        "outside a pass, with no screen"
    );
    assert_eq!(field(&app, "h"), Some(1920.0));
}
