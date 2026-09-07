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
