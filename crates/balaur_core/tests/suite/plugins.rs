use balaur_core::plugins::{self, PluginInfo};
use balaur_core::{App, AppConfig};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

#[test]
fn a_fresh_app_has_loaded_nothing() {
    assert!(app().plugins().is_empty());
}

#[test]
fn a_recorded_plugin_is_found_under_its_own_name() {
    let mut app = app();
    app.record_plugin(PluginInfo::new("weather", "1"));

    assert_eq!(plugins::names(&app.engine), ["weather"]);
    assert!(plugins::is_loaded(&app.engine, "weather"));
    assert!(!plugins::is_loaded(&app.engine, "elsewhere"));
}

#[test]
fn plugins_are_recorded_in_the_order_they_loaded() {
    let mut app = app();
    app.record_plugin(PluginInfo::new("zebra", "1"));
    app.record_plugin(PluginInfo::new("alpha", "1"));

    assert_eq!(plugins::names(&app.engine), ["zebra", "alpha"]);
}

#[test]
fn a_requirement_is_recorded_alongside_the_name() {
    let mut app = app();
    app.record_plugin(PluginInfo::new("late", "1").requiring(&["early".to_string()]));

    assert_eq!(app.plugins()[0].requires, ["early"]);
}
