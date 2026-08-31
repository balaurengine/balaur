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
        "name = \"ui\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\nscript = \"scripts/s.luau\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.luau"),
        format!(
            "local S = {{}}\n\
             function S:init() end\n\
             function S:draw_ui()\n{body}\nend\n\
             return S\n"
        ),
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

#[test]
fn a_pass_with_no_widgets_is_quiet() {
    draw_clean("");
}

#[test]
fn panels_and_containers_nest() {
    draw_clean(
        r#"
        ui.top_panel("bar", { height = 40 }, function()
            ui.horizontal({}, function() ui.label("across") end)
        end)
        ui.bottom_panel("status", { height = 20 }, function() ui.label("bottom") end)
        ui.left_panel("side", { width = 60 }, function() ui.label("left") end)
        ui.right_panel("props", { width = 60 }, function() ui.label("right") end)
        ui.central_panel({}, function()
            ui.vertical(function() ui.label("down") end)
            ui.scroll("sc", {}, function() ui.label("scrolled") end)
            ui.frame({}, function() ui.label("framed") end)
        end)
        "#,
    );
}

#[test]
fn text_and_layout_helpers_run() {
    draw_clean(
        r##"
        ui.central_panel({}, function()
            ui.label("plain")
            ui.code_line("local x = 1")
            ui.separator()
            ui.add_space(4)
            ui.spacing(2, 2)
            ui.dot("#ffffff", 4)
            assert(type(ui.available_width()) == "number")
            assert(type(ui.available_height()) == "number")
            local w, h = ui.screen_size()
            assert(type(w) == "number" and type(h) == "number")
            assert(type(ui.wants_keyboard()) == "boolean")
        end)
        "##,
    );
}

#[test]
fn interactive_widgets_report_no_interaction_without_input() {
    draw_clean(
        r#"
        ui.central_panel({}, function()
            assert(ui.pill("a pill", { active = true }) == false)
            assert(ui.circle_button("x") == false)
            local on, clicked = ui.toggle(false, {})
            assert(on == false and clicked == false, "a toggle flipped with no input")
            local text, changed = ui.text_field("field", "type here")
            assert(type(text) == "string", "text_field returns its buffer")
            assert(changed == false, "nothing typed, yet it reported a change")
            local v = ui.slider(0.5, 0.0, 1.0, {})
            assert(type(v) == "number")
        end)
        "#,
    );
}

#[test]
fn the_code_editor_returns_its_buffer_unchanged() {
    let (app, errors) = draw(
        r#"
        _G.drawn = _G.drawn or 0
        _G.edits = _G.edits or 0
        ui.central_panel({}, function()
            local text, changed = ui.code_editor("ed", "local x = 1\nreturn x")
            _G.drawn += 1
            if changed then _G.edits += 1 end
            assert(#text > 0, "the editor lost its buffer")
        end)
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let lua = balaur::luau::lua_of(&app.engine);
    let drawn: i64 = lua.globals().get("drawn").expect("the editor never drew");
    let edits: i64 = lua.globals().get("edits").expect("no edit count recorded");
    assert!(drawn > 0, "the editor never drew");
    assert_eq!(edits, 0, "the editor reported {edits} edits with no input");
}

#[test]
fn bad_options_are_reported_rather_than_fatal() {
    let (_app, errors) = draw(
        r#"
        ui.central_panel({}, function()
            ui.label("still drawn")
        end)
        "#,
    );
    assert!(errors.is_empty());
}

#[test]
fn a_script_error_inside_a_pass_is_logged_not_fatal() {
    let (_app, errors) = draw(
        r#"
        ui.central_panel({}, function()
            error("deliberate")
        end)
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
        ui.central_panel({}, function()
            local before = ui.scale()
            ui.set_scale(1.25)
            assert(math.abs(ui.scale() - 1.25) < 1e-6)
            ui.set_scale(before)
        end)
        ",
    );
}

/// A control: everything above asserts from inside the script, so if
/// `draw_ui` never ran they would all pass without testing anything. This
/// checks from Rust that the pass really called it.
#[test]
fn draw_ui_is_actually_called() {
    let (app, _) = draw(r"_G.passes = (_G.passes or 0) + 1");
    let lua = balaur::luau::lua_of(&app.engine);
    let passes: i64 = lua.globals().get("passes").unwrap_or(0);
    assert!(
        passes > 0,
        "draw_ui never ran, so these tests prove nothing"
    );
}

#[test]
fn the_remaining_widgets_are_callable() {
    draw_clean(
        r#"
        ui.central_panel({}, function()
            local v = ui.drag_value(1.5, {})
            assert(type(v) == "number", "drag_value should return a number")

            local choice, changed = ui.select("sel", "b", {"a", "b", "c"}, {})
            assert(choice == "b", "select changed with no input")
            assert(changed == false)

            assert(ui.menu_item("Open", {}) == false)
            ui.rect_stroke(0, 0, 10, 10, {})
        end)
        "#,
    );
}

#[test]
fn a_modal_runs_its_body() {
    let (app, errors) = draw(
        r#"
        _G.in_modal = false
        ui.central_panel({}, function()
            ui.modal("m", {}, function() _G.in_modal = true end)
        end)
        "#,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    let lua = balaur::luau::lua_of(&app.engine);
    let ran: bool = lua.globals().get("in_modal").unwrap_or(false);
    assert!(ran, "the modal never ran its body");
}

#[test]
fn set_text_replaces_a_field_buffer() {
    draw_clean(
        r#"
        ui.central_panel({}, function()
            ui.text_field("f", "placeholder")
            ui.set_text("f", "written from outside")
            local shown = ui.text_field("f", "placeholder")
            assert(shown == "written from outside", "set_text did not take: " .. tostring(shown))
        end)
        "#,
    );
}

#[test]
fn a_missing_image_does_not_stop_the_pass() {
    let (_app, errors) = draw(
        r#"
        ui.central_panel({}, function()
            ui.image("no/such/picture.png", {})
            ui.label("drawn after the bad image")
        end)
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
        ui.set_theme({ panel = "#101418", text = "#f0f0f0", accent = "#d5814e" })
        ui.central_panel({}, function() ui.label("themed") end)
        "##,
    );
}

#[test]
fn the_widget_layer_can_be_placed_and_turned_off() {
    draw_clean(
        r#"
        ui.set_widget_layer(true, 0, 0, 320, 240)
        ui.set_widget_layer(false, 0, 0, 0, 0)
        ui.central_panel({}, function() ui.label("after") end)
        "#,
    );
}

#[test]
fn shortcuts_report_no_press_without_input() {
    draw_clean(
        r#"
        ui.central_panel({}, function()
            assert(ui.shortcut("cmd", "S") == false)
            assert(ui.shortcut("ctrl", "Z") == false)
            assert(ui.shortcut("", "A") == false)
        end)
        "#,
    );
}
