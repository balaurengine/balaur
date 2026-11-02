//! The `multiplayer` settings page read into the faults a link simulates.

use balaur_core::{App, AppConfig, settings};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

#[test]
fn the_multiplayer_page_produces_the_faults_it_describes() {
    let app = app();
    assert!(
        balaur_net::transport::faults(&app.engine).is_none(),
        "off by default: a link misbehaves only when asked"
    );
    settings::set(
        &app.engine,
        "multiplayer/simulate_faults",
        toml::Value::Boolean(true),
    );
    settings::set(
        &app.engine,
        "multiplayer/delay_ticks",
        toml::Value::Float(9.0),
    );
    let faults = balaur_net::transport::faults(&app.engine).expect("turned on");
    assert_eq!(faults.delay, 9);
}
