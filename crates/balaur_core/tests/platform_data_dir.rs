//! Its own test binary: the platform data directory is named once per process,
//! and the suite's tests compare against the one `dirs` finds.

use balaur_core::engine_api::{EDITOR_NAME, set_platform_data_dir, user_data_dir_named};
use balaur_core::{App, AppConfig};

#[test]
fn a_named_platform_data_dir_holds_the_games_folders_and_the_editors() {
    let data = tempfile::tempdir().unwrap();
    set_platform_data_dir(data.path().to_path_buf());
    let app = App::new(AppConfig::bare(".")).unwrap();

    assert_eq!(
        user_data_dir_named(&app.engine, "hello"),
        data.path().join("balaur").join("hello"),
        "a packed game's root is `.`, and its user data must not land under it"
    );
    assert_eq!(
        user_data_dir_named(&app.engine, EDITOR_NAME),
        data.path().join("balaur-editor")
    );
}
