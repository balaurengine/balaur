//! The settings registry: what is declared, what a page holds, and what it
//! writes back.
//!
//! The write path is the one worth testing. A manifest is a file people edit
//! by hand, so writing settings into it must change the keys a page declares
//! and leave everything else — including tables no page knows about — alone.

use balaur_core::settings::{self, Scope, SettingDef};
use balaur_core::{App, AppConfig, ComponentDef};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

#[test]
fn core_defines_settings_in_both_scopes() {
    let app = app();
    let all = settings::all(&app.engine);
    let all = all.borrow();
    let paths: Vec<&str> = all.0.iter().map(|d| d.path.as_str()).collect();
    assert!(paths.contains(&"application/name"));
    assert!(paths.contains(&"netcode/faults"));
    let faults = all.0.iter().find(|d| d.path == "netcode/faults").unwrap();
    assert_eq!(
        faults.scope,
        Scope::Editor,
        "fault injection is a developer's tool, not something a game ships"
    );
    assert_eq!(faults.category(), "netcode");
    assert_eq!(faults.label(), "faults");
}

/// A path nests: `editor/appearance/theme` is `[editor.appearance] theme`.
#[test]
fn a_nested_path_reads_and_writes_where_it_says() {
    let app = app();
    settings::set(
        &app.engine,
        "editor/appearance/theme",
        toml::Value::String(String::from("light")),
    );
    let written = settings::to_toml(&app.engine, Scope::Editor, "").unwrap();
    let parsed: toml::Value = toml::from_str(&written).unwrap();
    assert_eq!(
        parsed["editor"]["appearance"]["theme"].as_str(),
        Some("light")
    );
    settings::load(&app.engine, &written).unwrap();
    assert_eq!(
        settings::get(&app.engine, "editor/appearance/theme"),
        Some(toml::Value::String(String::from("light")))
    );
}

#[test]
fn a_setting_falls_back_to_its_schema_default() {
    let app = app();
    assert_eq!(
        settings::get(&app.engine, "netcode/faults"),
        Some(toml::Value::Boolean(false))
    );
    settings::set(&app.engine, "netcode/faults", toml::Value::Boolean(true));
    assert_eq!(
        settings::get(&app.engine, "netcode/faults"),
        Some(toml::Value::Boolean(true))
    );
}

/// The load-bearing one: writing settings into a manifest touches the keys a
/// page declares and nothing else.
#[test]
fn writing_a_manifest_leaves_what_it_does_not_describe_alone() {
    let app = app();
    let before = "\
[application]
name = \"mine\"
main_scene = \"scenes/main.toml\"

[something_else]
kept = true
";
    // A page of this test's own: the HTTP and physics pages belong to their
    // plugins, and a bare core app has not loaded them.
    settings::define_group(
        &app.engine,
        "weather",
        Scope::Project,
        &ComponentDef::parse_schema(
            "settings.weather",
            r#"rain = { type = "bool", default = true }"#,
        ),
    );
    settings::load(&app.engine, before).unwrap();
    settings::set(&app.engine, "weather/rain", toml::Value::Boolean(false));
    let after = settings::to_toml(&app.engine, Scope::Project, before).unwrap();
    let parsed: toml::Value = toml::from_str(&after).unwrap();

    assert_eq!(
        parsed["application"]["name"].as_str(),
        Some("mine"),
        "the name survived"
    );
    assert_eq!(
        parsed["something_else"]["kept"].as_bool(),
        Some(true),
        "a table no page declares is not dropped"
    );
    assert_eq!(
        parsed["weather"]["rain"].as_bool(),
        Some(false),
        "and the changed setting landed"
    );
}

/// Editor settings never reach the project file, whatever they are set to.
#[test]
fn an_editor_setting_stays_out_of_the_manifest() {
    let app = app();
    settings::set(&app.engine, "netcode/faults", toml::Value::Boolean(true));
    let written = settings::to_toml(
        &app.engine,
        Scope::Project,
        "[application]\nname = \"mine\"\n",
    )
    .unwrap();
    assert!(
        !written.contains("netcode"),
        "an editor-scope page must not be written to project.toml: {written}"
    );
    let prefs = settings::to_toml(&app.engine, Scope::Editor, "").unwrap();
    assert!(
        prefs.contains("netcode"),
        "it belongs in the editor's own file"
    );
}

/// A plugin's page joins the same list, which is what makes the screen
/// extensible rather than a fixed set of tabs.
#[test]
fn anyone_can_define_a_setting() {
    let app = app();
    settings::define(
        &app.engine,
        SettingDef {
            path: String::from("weather/rain"),
            scope: Scope::Project,
            spec: toml::from_str(
                r#"type = "bool"
default = true
help = "Whether it rains.""#,
            )
            .unwrap(),
        },
    );
    assert_eq!(
        settings::get(&app.engine, "weather/rain"),
        Some(toml::Value::Boolean(true))
    );
}

#[test]
fn the_netcode_page_produces_the_faults_it_describes() {
    let app = app();
    assert!(
        settings::faults(&app.engine).is_none(),
        "off by default: a link misbehaves only when asked"
    );
    settings::set(&app.engine, "netcode/faults", toml::Value::Boolean(true));
    settings::set(&app.engine, "netcode/delay", toml::Value::Float(9.0));
    let faults = settings::faults(&app.engine).expect("turned on");
    assert_eq!(faults.delay, 9);
}

/// `[override.android.window] fullscreen` is what `window/fullscreen` reads
/// on a phone, and nothing at all anywhere else.
#[test]
fn an_override_answers_only_where_its_tag_is_in_force() {
    let app = app();
    settings::load(
        &app.engine,
        "[window]\nfullscreen = false\n\n[override.android.window]\nfullscreen = true\n",
    )
    .unwrap();

    app.engine.insert_resource(balaur_core::tags::Tags(vec![
        "desktop".into(),
        "linux".into(),
    ]));
    assert_eq!(
        settings::get(&app.engine, "window/fullscreen"),
        Some(toml::Value::Boolean(false))
    );

    app.engine.insert_resource(balaur_core::tags::Tags(vec![
        "mobile".into(),
        "android".into(),
    ]));
    assert_eq!(
        settings::get(&app.engine, "window/fullscreen"),
        Some(toml::Value::Boolean(true))
    );
}

/// The narrow tag wins wherever the file wrote it: precedence is the tag
/// order this run holds, not the order two tables happen to appear in.
#[test]
fn the_narrower_tag_outranks_the_broader_one() {
    let app = app();
    settings::load(
        &app.engine,
        "[override.android.physics]\nsolver_iterations = 3.0\n\n\
         [override.mobile.physics]\nsolver_iterations = 2.0\n",
    )
    .unwrap();
    app.engine.insert_resource(balaur_core::tags::Tags(vec![
        "mobile".into(),
        "android".into(),
    ]));
    assert_eq!(
        settings::get(&app.engine, "physics/solver_iterations"),
        Some(toml::Value::Float(3.0))
    );
}

/// What the editor edits is the file's own value, not the one this machine
/// resolves: a screen showing the override would write it onto the base key.
#[test]
fn the_base_read_ignores_every_override() {
    let app = app();
    let source = "[window]\nfullscreen = false\n\n[override.android.window]\nfullscreen = true\n";
    settings::load(&app.engine, source).unwrap();
    app.engine
        .insert_resource(balaur_core::tags::Tags(vec!["android".into()]));

    assert_eq!(
        settings::base(&app.engine, "window/fullscreen"),
        Some(toml::Value::Boolean(false))
    );
    let written = settings::to_toml(&app.engine, Scope::Project, source).unwrap();
    let parsed: toml::Value = toml::from_str(&written).unwrap();
    assert_eq!(
        parsed["override"]["android"]["window"]["fullscreen"].as_bool(),
        Some(true),
        "an override no page declares survives a write: {written}"
    );
}

/// A table core knows nothing about is the game's own space, readable by the
/// same call as everything else.
#[test]
fn an_undeclared_table_is_readable_by_path() {
    let app = app();
    settings::load(
        &app.engine,
        "[mygame]\nlocal_server_url = \"http://localhost:8080\"\n",
    )
    .unwrap();
    assert_eq!(
        settings::get(&app.engine, "mygame/local_server_url"),
        Some(toml::Value::String(String::from("http://localhost:8080")))
    );
}

/// The screen owns the override tree: setting one writes it, clearing one
/// takes it out, and the table it lived in goes with it.
#[test]
fn an_override_is_written_and_removed_by_the_same_write() {
    let app = app();
    settings::set(
        &app.engine,
        "override/android/window/fullscreen",
        toml::Value::Boolean(true),
    );
    let written = settings::to_toml(&app.engine, Scope::Project, "").unwrap();
    assert!(written.contains("[override.android.window]"), "{written}");

    settings::clear(&app.engine, "override/android/window/fullscreen");
    let written = settings::to_toml(&app.engine, Scope::Project, &written).unwrap();
    assert!(
        !written.contains("override"),
        "the emptied table stayed behind: {written}"
    );
}

/// An override on a key nothing declares is not the screen's to write, and a
/// hand-written one has to survive the screen's save.
#[test]
fn an_override_the_screen_does_not_know_survives_a_write() {
    let app = app();
    let source = "[override.ios.mygame]\nurl = \"https://example.test\"\n";
    settings::load(&app.engine, source).unwrap();
    let written = settings::to_toml(&app.engine, Scope::Project, source).unwrap();
    assert!(written.contains("example.test"), "{written}");
}

/// A save from the settings screen is a small diff: comments stay, and a key
/// nobody set is not written out as its default.
#[test]
fn a_write_keeps_comments_and_adds_no_defaults() {
    let app = app();
    let source = "# The game.\n[application]\nname = \"g\" # shown in the title\nmain_scene = \"main.toml\"\n";
    settings::load(&app.engine, source).unwrap();
    settings::set(&app.engine, "window/fullscreen", toml::Value::Boolean(true));

    let written = settings::to_toml(&app.engine, Scope::Project, source).unwrap();
    assert!(written.contains("# The game."), "{written}");
    assert!(written.contains("# shown in the title"), "{written}");
    assert!(written.contains("[window]\nfullscreen = true"), "{written}");
    assert!(
        !written.contains("width") && !written.contains("solver"),
        "a default nobody chose was written: {written}"
    );
}

/// A table of the game's own names takes an override key by key: rebinding
/// one action on a phone leaves every other action as the file wrote it.
#[test]
fn a_table_folds_each_override_on_key_by_key() {
    let app = app();
    settings::load(
        &app.engine,
        "[input.actions]\njump = [\"Space\"]\nfire = [\"KeyF\"]\n\n\
         [override.mobile.input.actions]\njump = [\"touch:jump\"]\n",
    )
    .unwrap();
    app.engine
        .insert_resource(balaur_core::tags::Tags(vec!["mobile".into(), "android".into()]));

    let actions = settings::table(&app.engine, "input/actions");
    assert_eq!(actions["jump"][0].as_str(), Some("touch:jump"));
    assert_eq!(actions["fire"][0].as_str(), Some("KeyF"));
}
