//! A run's script cache is never an edit, however it is linked into the
//! project. Its own test binary: the platform data directory is named once a
//! process.

#![cfg(unix)]

use balaur_script::Value;

/// The paths `files_changed` carried in the pump just run.
fn said(app: &balaur::App) -> Vec<Value> {
    balaur_core::events::delivered(&app.engine, balaur_core::events::FILES_CHANGED)
        .into_iter()
        .filter_map(|paths| match paths {
            Value::List(paths) => Some(paths),
            _ => None,
        })
        .flatten()
        .collect()
}

/// What the watch heard until it named `marker`, written after `cache_file`.
fn heard_after(project: &std::path::Path, cache_file: &std::path::Path) -> Vec<Value> {
    std::fs::write(
        project.join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(project.join("main.toml"), "").unwrap();
    let mut app =
        balaur::standard_app(balaur::AppConfig::dev(project.to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    std::fs::write(cache_file, "compiled").unwrap();
    std::fs::write(project.join("marker.txt"), "edited").unwrap();
    let marker = Value::Str("marker.txt".into());
    let mut heard = Vec::new();
    // A cap for a stuck watch: a passing run hears the marker in milliseconds.
    for _ in 0..12_000 {
        app.tick(1.0 / 60.0);
        heard.extend(said(&app));
        if heard.contains(&marker) {
            return heard;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the watch never heard the marker: {heard:?}");
}

fn names_the_cache(heard: &[Value]) -> bool {
    heard
        .iter()
        .any(|path| matches!(path, Value::Str(p) if p.contains("0123.cwasm")))
}

#[test]
fn a_script_cache_linked_either_way_between_the_data_folder_and_the_project_is_no_edit() {
    let data = tempfile::tempdir().unwrap();
    balaur_core::engine_api::set_platform_data_dir(data.path().to_path_buf());
    let home = data.path().join("balaur/fixture");
    std::fs::create_dir_all(&home).unwrap();

    // The data folder's cache a link to a folder in the project.
    let project = tempfile::tempdir().unwrap();
    let cache = project.path().join("_build/bs-cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::os::unix::fs::symlink(&cache, home.join("bs-cache")).unwrap();
    let heard = heard_after(project.path(), &cache.join("0123.cwasm"));
    assert!(!names_the_cache(&heard), "{heard:?}");
    std::fs::remove_file(home.join("bs-cache")).unwrap();

    // The project's folder a link to the data folder's cache.
    let project = tempfile::tempdir().unwrap();
    let cache = home.join("bs-cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(project.path().join("_build")).unwrap();
    let link = project.path().join("_build/bs-cache");
    std::os::unix::fs::symlink(&cache, &link).unwrap();
    let heard = heard_after(project.path(), &link.join("0123.cwasm"));
    assert!(!names_the_cache(&heard), "{heard:?}");
}
