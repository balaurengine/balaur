//! The settings registry: what is declared, what a page holds, and what it
//! writes back.
//!
//! The write path is the one worth testing. A manifest is a file people edit
//! by hand, so writing settings into it must change the keys a page declares
//! and leave everything else — including tables no page knows about — alone.

use balaur_core::settings::{self, Scope, SettingsPage};
use balaur_core::{App, AppConfig, ComponentDef};

fn app() -> App {
    App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap()
}

#[test]
fn core_declares_its_pages_in_both_scopes() {
    let app = app();
    let pages = settings::pages(&app.engine);
    let pages = pages.borrow();
    let categories: Vec<&str> = pages.0.iter().map(|p| p.category.as_str()).collect();
    assert!(categories.contains(&"General"));
    assert!(categories.contains(&"Netcode"));
    let netcode = pages.0.iter().find(|p| p.category == "Netcode").unwrap();
    assert_eq!(
        netcode.scope,
        Scope::Editor,
        "fault injection is a developer's tool, not something a game ships"
    );
}

#[test]
fn a_setting_falls_back_to_its_schema_default() {
    let app = app();
    assert_eq!(
        settings::get(&app.engine, "netcode", "faults"),
        Some(toml::Value::Boolean(false))
    );
    settings::set(&app.engine, "netcode", "faults", toml::Value::Boolean(true));
    assert_eq!(
        settings::get(&app.engine, "netcode", "faults"),
        Some(toml::Value::Boolean(true))
    );
}

/// The load-bearing one: writing settings into a manifest touches the keys a
/// page declares and nothing else.
#[test]
fn writing_a_manifest_leaves_what_it_does_not_describe_alone() {
    let app = app();
    let before = "\
name = \"mine\"
main_scene = \"scenes/main.toml\"

[something_else]
kept = true
";
    // A page of this test's own: the HTTP and physics pages belong to their
    // plugins, and a bare core app has not loaded them.
    settings::register(
        &app.engine,
        SettingsPage {
            category: String::from("Weather"),
            table: String::from("weather"),
            scope: Scope::Project,
            schema: ComponentDef::parse_schema(
                "settings.weather",
                r#"rain = { type = "bool", default = true }"#,
            ),
        },
    );
    settings::load_project(&app.engine, before).unwrap();
    settings::set(&app.engine, "weather", "rain", toml::Value::Boolean(false));
    let after = settings::project_toml(&app.engine, before).unwrap();
    let parsed: toml::Value = toml::from_str(&after).unwrap();

    assert_eq!(parsed["name"].as_str(), Some("mine"), "the name survived");
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
    settings::set(&app.engine, "netcode", "faults", toml::Value::Boolean(true));
    let written = settings::project_toml(&app.engine, "name = \"mine\"\n").unwrap();
    assert!(
        !written.contains("netcode"),
        "an editor-scope page must not be written to project.toml: {written}"
    );
    let prefs = settings::editor_toml(&app.engine).unwrap();
    assert!(
        prefs.contains("netcode"),
        "it belongs in the editor's own file"
    );
}

/// A plugin's page joins the same list, which is what makes the screen
/// extensible rather than a fixed set of tabs.
#[test]
fn a_plugin_can_add_a_page() {
    let app = app();
    settings::register(
        &app.engine,
        SettingsPage {
            category: String::from("Weather"),
            table: String::from("weather"),
            scope: Scope::Project,
            schema: ComponentDef::parse_schema(
                "settings.weather",
                r#"rain = { type = "bool", default = true, help = "Whether it rains." }"#,
            ),
        },
    );
    assert_eq!(
        settings::get(&app.engine, "weather", "rain"),
        Some(toml::Value::Boolean(true))
    );
}

/// The faults the Netcode page asks for reach whoever builds a link.
#[test]
fn the_netcode_page_produces_the_faults_it_describes() {
    let app = app();
    assert!(
        settings::faults(&app.engine).is_none(),
        "off by default: a link misbehaves only when asked"
    );
    settings::set(&app.engine, "netcode", "faults", toml::Value::Boolean(true));
    settings::set(&app.engine, "netcode", "delay", toml::Value::Float(9.0));
    let faults = settings::faults(&app.engine).expect("turned on");
    assert_eq!(faults.delay, 9);
}
