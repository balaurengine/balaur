//! The `ui` module driven through a real script, around real egui passes.
//!
//! egui runs perfectly well without a window, so the UI is testable in CI.
//! Calling the bindings from a script is the point: it is the path a game
//! takes, and it catches a binding that registers but cannot be called. A
//! script runs between passes, in `update`, so every call here answers
//! there.

use balaur::{AppConfig, standard_app};
use balaur_core::App;

/// The log buffer is global, and tests run in parallel, so one test's
/// deliberate error would surface in another's assertions. Hold this for the
/// duration of a run.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `Root`'s script: `update` runs `body` each tick and counts the ticks in
/// `ran`. `scene` is more nodes under `Root`, as a scene file spells them.
#[derive(Default)]
pub(crate) struct Script<'a> {
    /// More fields, as a struct declares them: `focus: int`.
    pub(crate) fields: &'a str,
    pub(crate) update: &'a str,
    /// More methods, whole.
    pub(crate) methods: &'a str,
    pub(crate) scene: &'a str,
}

impl Script<'_> {
    fn source(&self) -> String {
        let sep = if self.fields.is_empty() { "" } else { ", " };
        format!(
            "pub struct Root {{ node: Node, ran: int{sep}{} }}\n\
             impl Root {{\n\
             pub fn update(dt: float) {{\nself.ran += 1;\n{}\n}}\n\
             {}\n}}\n",
            self.fields, self.update, self.methods
        )
    }
}

/// A label under `Root`, so a pass has something to draw.
const LABEL: &str = "[[nodes]]\nid = \"l\"\nname = \"Label\"\nparent = \"n\"\n\
     widget = { kind = \"label\", text = \"x\", x = 0.0, y = 0.0 }\n";

/// A project whose `update` runs `body`, after two ticks and two passes.
pub(crate) fn run_with(body: &str) -> (tempfile::TempDir, App, egui::Context, Vec<String>) {
    run(&Script {
        update: body,
        ..Script::default()
    })
}

/// Build a project running `script`, then tick it and run a pass, twice.
/// The context comes back: fonts are bound to that one, so a test running
/// further passes has to use it. The project directory comes back with the
/// app: dropped while the app is still alive, a later tick fails to re-read
/// the script and logs an error into whatever test is reading the log.
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
    std::fs::write(
        dir.path().join("main.toml"),
        format!(
            "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = {{ source = \"scripts/s.bs\" }}\n\n{}",
            script.scene
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
        app.tick(balaur_core::DEFAULT_FIXED_DT);
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

/// A run that logs nothing, and whose `update` ran: a script asserts by
/// logging an error, which holds vacuously if it never ran.
fn run_clean(body: &str) {
    let (_dir, app, _, errors) = run_with(body);
    assert!(errors.is_empty(), "the run logged errors: {errors:#?}");
    assert!(
        field(&app, "ran").unwrap_or(0.0) > 0.0,
        "update never ran, so the script checked nothing"
    );
}

/// A number the script keeps in a field, read back from Rust.
fn field(app: &App, name: &str) -> Option<f64> {
    crate::support::script_number(app, node(app, "Root"), name)
}

/// A node by its path from the scene's root, as `Root/Label`.
fn node(app: &App, path: &str) -> balaur_core::hecs::Entity {
    balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), path)
        .unwrap_or_else(|| panic!("the scene has no {path}"))
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

/// With no window nothing runs the UI pass; the pass on no screen does,
/// after the one that binds the fonts, and the label it drew has a box.
#[test]
fn a_run_with_no_window_still_draws_its_ui() {
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
        format!("[[nodes]]\nid = \"n\"\nname = \"Root\"\n\n{LABEL}"),
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    balaur_ui::pass_without_window(&mut app, 800.0, 600.0);
    app.tick(1.0 / 60.0);
    assert!(
        balaur_ui::widget_rect(node(&app, "Root/Label")).is_none(),
        "the first pass only binds the fonts"
    );
    app.tick(1.0 / 60.0);
    assert!(
        balaur_ui::widget_rect(node(&app, "Root/Label")).is_some(),
        "the second pass drew nothing"
    );
}

#[test]
fn a_run_with_no_widgets_is_quiet() {
    run_clean("");
}

/// A control: everything here asserts from inside the script, so if `update`
/// never ran it would all pass without testing anything.
#[test]
fn update_is_actually_called() {
    let (_dir, app, _, _) = run_with("");
    assert_eq!(field(&app, "ran"), Some(2.0));
}

#[test]
fn the_scale_factor_is_readable_and_settable() {
    run_clean(
        r#"
        let before = ui::scale();
        ui::set_scale(1.25);
        let after = ui::scale();
        if math::abs(after - 1.25) >= 0.000001 { log::error("the scale read back as {after}"); }
        ui::set_scale(before);
        "#,
    );
}

/// What a pass knows, a script reads between passes.
#[test]
fn the_frame_queries_answer_between_passes() {
    run_clean(
        r#"
        let size: Vec2 = ui::window_size();
        if size.x <= 0.0 { log::error("no window size: {size}"); }
        if ui::wants_keyboard() { log::error("a widget holds the keyboard with none focused"); }
        if ui::pasted_text() != "" { log::error("a paste with no input"); }
        "#,
    );
}

/// A script copies between passes; the next pass hands the text over.
#[test]
fn the_clipboard_is_written_by_the_next_pass() {
    let (_dir, mut app, ctx, errors) = run_with(r#"ui::set_clipboard("copied");"#);
    assert!(errors.is_empty(), "{errors:#?}");
    app.tick(balaur_core::DEFAULT_FIXED_DT);
    let out = feed(&app, &ctx, Vec::new());
    assert!(
        out.platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == "copied")),
        "the pass did not copy: {:?}",
        out.platform_output.commands
    );
}

/// Text pasted into a pass, read by the next tick.
#[test]
fn a_paste_is_read_after_the_pass_that_saw_it() {
    let (_dir, mut app, ctx, errors) = run(&Script {
        fields: "got: str",
        update: "self.got = ui::pasted_text();",
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    feed(&app, &ctx, vec![egui::Event::Paste("pasted".into())]);
    app.tick(balaur_core::DEFAULT_FIXED_DT);
    let root = node(&app, "Root");
    assert_eq!(
        crate::support::script_text(&app, root, "got").as_deref(),
        Some("pasted")
    );
}

#[test]
fn a_theme_can_be_set_from_a_script() {
    run_clean(
        r##"ui::set_theme(map().with("panel", "#101418").with("text", "#f0f0f0").with("accent", "#d5814e"));"##,
    );
}

#[test]
fn the_widget_layer_can_be_placed_and_turned_off() {
    run_clean(
        "ui::set_widget_layer(true, 0.0, 0.0, 320.0, 240.0);
        ui::set_widget_layer(false, 0.0, 0.0, 0.0, 0.0);",
    );
}

/// One chord for both keys: the window layer sets egui's `command` for
/// Control and for Command, and a key arrives as its physical code, so ⇧⌘\
/// is `Backslash` whatever the layout prints on it.
#[test]
fn a_cmd_chord_answers_to_control_and_to_command() {
    let (_dir, mut app, ctx, errors) = run(&Script {
        fields: "focus: int",
        methods: "pub fn hit() { self.focus += 1; }",
        scene: "[[nodes]]\nid = \"b\"\nname = \"Go\"\nparent = \"n\"\n\
                widget = { kind = \"button\", text = \"Go\", shortcut = \"cmd+shift+backslash\", on_click = \"hit\" }\n",
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
        app.tick(balaur_core::DEFAULT_FIXED_DT);
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
    app.tick(balaur_core::DEFAULT_FIXED_DT);
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
    let (_dir, mut app, ctx, _) = run(&Script {
        scene: LABEL,
        ..Script::default()
    });
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

/// Nothing is polled: a lazy shell with no input and nothing written runs no
/// pass however long it waits, where a 250 ms tick once ran one regardless.
#[test]
fn a_lazy_shell_nobody_touched_runs_no_pass_however_long_it_waits() {
    let (_dir, app, ctx, errors) = run(&Script {
        update: "ui::set_lazy(true);",
        scene: LABEL,
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
