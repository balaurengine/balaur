//! The `ui` module driven through a real script inside a real egui pass.
//!
//! egui runs perfectly well without a window, so the whole widget surface is
//! testable in CI. Calling the bindings from a script is the point: it is the
//! path a game takes, and it catches a binding that registers but cannot be
//! called.

use balaur::{standard_app, AppConfig};
use balaur_core::App;

/// The log buffer is global, and tests run in parallel, so one test's
/// deliberate error would surface in another's assertions. Hold this for the
/// duration of a pass.
static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Build a project whose `draw_ui` runs `body`, then run two egui passes.
fn draw(body: &str) -> (App, Vec<String>) {
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
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = \"scripts/s.rn\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        format!("pub fn init(this) {{}}\npub fn draw_ui(this) {{\n{body}\n}}\n"),
    )
    .unwrap();

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
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
    let errors = balaur_core::logbuf::recent(50)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| e.message)
        .collect();
    (app, errors)
}

fn draw_clean(body: &str) {
    let (_app, errors) = draw(body);
    assert!(errors.is_empty(), "the pass logged errors: {errors:#?}");
}

/// A number the script left on `this`, read back from Rust.
fn field(app: &App, name: &str) -> Option<f64> {
    let root = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Root")
        .expect("the scene has a Root node");
    balaur::rune::rune_of(&app.engine).number_field(root, name)
}

#[test]
fn a_pass_with_no_widgets_is_quiet() {
    draw_clean("");
}

#[test]
fn panels_and_containers_nest() {
    draw_clean(
        r#"
        ui::top_panel("bar", #{ height: 40 }, || {
            ui::horizontal(#{}, || { ui::label("across"); });
        });
        ui::bottom_panel("status", #{ height: 20 }, || { ui::label("bottom"); });
        ui::left_panel("side", #{ width: 60 }, || { ui::label("left"); });
        ui::right_panel("props", #{ width: 60 }, || { ui::label("right"); });
        ui::central_panel(#{}, || {
            ui::vertical(|| { ui::label("down"); });
            ui::scroll("sc", #{}, || { ui::label("scrolled"); });
            ui::frame(#{}, || { ui::label("framed"); });
        });
        "#,
    );
}

#[test]
fn text_and_layout_helpers_run() {
    draw_clean(
        r##"
        ui::central_panel(#{}, || {
            ui::label("plain");
            ui::code_line("1", [#{ text: "let x = 1;" }]);
            ui::separator();
            ui::add_space(4);
            ui::spacing(2, 2);
            ui::dot("#ffffff", 4);
            assert!(ui::available_width() is f64);
            assert!(ui::available_height() is f64);
            let (w, h) = ui::screen_size();
            assert!(w is f64 && h is f64);
            assert!(ui::wants_keyboard() is bool);
        });
        "##,
    );
}

#[test]
fn interactive_widgets_report_no_interaction_without_input() {
    draw_clean(
        r#"
        ui::central_panel(#{}, || {
            assert!(!ui::pill("a pill", #{ active: true }));
            assert!(!ui::circle_button("x"));
            let (on, clicked) = ui::toggle(false, #{});
            assert!(!on && !clicked, "a toggle flipped with no input");
            let (text, changed, _) = ui::text_field("field", "type here");
            assert!(text is String, "text_field returns its buffer");
            assert!(!changed, "nothing typed, yet it reported a change");
            let (v, _) = ui::slider(0.5, 0.0, 1.0, #{});
            assert!(v is f64);
        });
        "#,
    );
}

#[test]
fn the_code_editor_returns_its_buffer_unchanged() {
    let (app, errors) = draw(
        r#"
        this.drawn = this.get("drawn").unwrap_or(0);
        this.edits = this.get("edits").unwrap_or(0);
        ui::central_panel(#{}, || {
            let (text, changed, hit) = ui::code_editor("ed", "let x = 1;\nx");
            this.drawn = this.drawn + 1;
            if changed { this.edits = this.edits + 1; }
            assert!(hit is Tuple, "a gutter line was clicked with no input");
            assert!(text.len() > 0, "the editor lost its buffer");
        });
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let drawn = field(&app, "drawn").expect("the editor never drew");
    let edits = field(&app, "edits").expect("no edit count recorded");
    assert!(drawn > 0.0, "the editor never drew");
    assert!(
        edits == 0.0,
        "the editor reported {edits} edits with no input"
    );
}

#[test]
fn bad_options_are_reported_rather_than_fatal() {
    let (_app, errors) = draw(
        r#"
        ui::central_panel(#{}, || {
            ui::label("still drawn");
        });
        "#,
    );
    assert!(errors.is_empty());
}

#[test]
fn a_script_error_inside_a_pass_is_logged_not_fatal() {
    let (_app, errors) = draw(
        r#"
        ui::central_panel(#{}, || {
            panic!("deliberate");
        });
        "#,
    );
    assert!(
        errors.iter().any(|e| e.contains("deliberate")),
        "the error was swallowed: {errors:?}"
    );
}

#[test]
fn the_scale_factor_is_readable_and_settable() {
    draw_clean(
        r"
        ui::central_panel(#{}, || {
            let before = ui::scale();
            ui::set_scale(1.25);
            assert!(math::abs(ui::scale() - 1.25) < 1e-6);
            ui::set_scale(before);
        });
        ",
    );
}

/// A control: everything above asserts from inside the script, so if
/// `draw_ui` never ran they would all pass without testing anything. This
/// checks from Rust that the pass really called it.
#[test]
fn draw_ui_is_actually_called() {
    let (app, _) = draw(r#"this.passes = this.get("passes").unwrap_or(0) + 1;"#);
    let passes = field(&app, "passes").unwrap_or(0.0);
    assert!(
        passes > 0.0,
        "draw_ui never ran, so these tests prove nothing"
    );
}

#[test]
fn the_remaining_widgets_are_callable() {
    draw_clean(
        r#"
        ui::central_panel(#{}, || {
            let (v, _) = ui::drag_value(1.5, #{});
            assert!(v is f64, "drag_value should return a number");

            let (choice, changed) = ui::dropdown("sel", "b", ["a", "b", "c"], #{});
            assert!(choice == "b", "dropdown changed with no input");
            assert!(!changed);

            assert!(!ui::menu_item("Open", #{}));
            ui::rect_stroke(0, 0, 10, 10, #{});
        });
        "#,
    );
}

#[test]
fn a_modal_runs_its_body() {
    let (app, errors) = draw(
        r#"
        this.in_modal = 0;
        ui::central_panel(#{}, || {
            ui::modal("m", #{}, || { this.in_modal = 1; });
        });
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let ran = field(&app, "in_modal").unwrap_or(0.0) > 0.0;
    assert!(ran, "the modal never ran its body");
}

#[test]
fn set_text_replaces_a_field_buffer() {
    draw_clean(
        r#"
        ui::central_panel(#{}, || {
            ui::text_field("f", "placeholder");
            ui::set_text("f", "written from outside");
            let (shown, _, _) = ui::text_field("f", "placeholder");
            assert!(shown == "written from outside", "set_text did not take: {}", shown);
        });
        "#,
    );
}

#[test]
fn a_missing_image_does_not_stop_the_pass() {
    let (_app, errors) = draw(
        r#"
        ui::central_panel(#{}, || {
            ui::image("no/such/picture.png", #{});
            ui::label("drawn after the bad image");
        });
        "#,
    );
    assert!(
        errors.iter().all(|e| !e.contains("panic")),
        "a missing image panicked: {errors:#?}"
    );
}

#[test]
fn a_theme_can_be_set_from_a_script() {
    draw_clean(
        r##"
        ui::set_theme(#{ panel: "#101418", text: "#f0f0f0", accent: "#d5814e" });
        ui::central_panel(#{}, || { ui::label("themed"); });
        "##,
    );
}

#[test]
fn the_widget_layer_can_be_placed_and_turned_off() {
    draw_clean(
        r#"
        ui::set_widget_layer(true, 0, 0, 320, 240);
        ui::set_widget_layer(false, 0, 0, 0, 0);
        ui::central_panel(#{}, || { ui::label("after"); });
        "#,
    );
}

#[test]
fn shortcuts_report_no_press_without_input() {
    draw_clean(
        r#"
        ui::central_panel(#{}, || {
            assert!(!ui::shortcut("cmd", "S"));
            assert!(!ui::shortcut("ctrl", "Z"));
            assert!(!ui::shortcut("", "A"));
        });
        "#,
    );
}
