use balaur_core::plugins::{self, PluginInfo};
use balaur_core::{App, AppConfig, Plugin};

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

struct Quiet(&'static str);

impl Plugin for Quiet {
    fn name(&self) -> &str {
        self.0
    }

    fn build(&mut self, _: &mut App) -> anyhow::Result<()> {
        Ok(())
    }
}

struct Loud(&'static str);

impl Plugin for Loud {
    fn name(&self) -> &str {
        self.0
    }

    fn build(&mut self, _: &mut App) -> anyhow::Result<()> {
        anyhow::bail!("no")
    }
}

#[test]
fn a_fresh_app_has_loaded_nothing() {
    assert!(app().plugins().is_empty());
}

#[test]
fn adding_a_plugin_records_it_under_its_own_name() {
    let mut app = app();
    app.add_plugin(Quiet("weather")).unwrap();

    assert_eq!(plugins::names(&app.engine), ["weather"]);
    assert!(plugins::is_loaded(&app.engine, "weather"));
    assert!(!plugins::is_loaded(&app.engine, "elsewhere"));
}

#[test]
fn plugins_are_recorded_in_the_order_they_loaded() {
    let mut app = app();
    app.add_plugin(Quiet("zebra")).unwrap();
    app.add_plugin(Quiet("alpha")).unwrap();

    assert_eq!(plugins::names(&app.engine), ["zebra", "alpha"]);
}

#[test]
fn a_plugin_that_fails_to_build_is_not_recorded() {
    let mut app = app();
    assert!(app.add_plugin(Loud("broken")).is_err());

    assert!(app.plugins().is_empty());
}

#[test]
fn a_recorded_plugin_carries_a_version() {
    let mut app = app();
    app.add_plugin(Quiet("weather")).unwrap();

    let loaded = app.plugins();
    assert!(!loaded[0].version.is_empty(), "{loaded:?}");
}

#[test]
fn a_requirement_is_recorded_alongside_the_name() {
    let mut app = app();
    app.record_plugin(PluginInfo::new("late", "1").requiring(&["early".to_string()]));

    assert_eq!(app.plugins()[0].requires, ["early"]);
}
