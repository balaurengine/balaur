//! What the Steam plugin can be held to without a Steam client running this
//! game: it loads, leaves `platform.*` as it found it, and a call answers.

use balaur_core::{App, AppConfig};
use balaur_platform::{PlatformPlugin, PlatformState};
use balaur_steam::SteamPlugin;

#[test]
fn with_no_steam_client_the_plugin_loads_and_leaves_platform_alone() {
    let mut app = App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
    balaur_plugin::load(&mut app, &mut SteamPlugin).unwrap();
    let state = app.engine.resource::<PlatformState>();
    assert_eq!(state.borrow().backend_name(), "none");
}
