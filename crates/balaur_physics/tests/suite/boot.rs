//! Booting a project from a scene and one script, for the tests that read
//! assets or call the script API.

use balaur_core::App;
use balaur_core::hecs::Entity;

/// A booted project, its directory, and [`crate::LOG`] held for as long as
/// the caller keeps it, so the errors logged are the test's own.
pub(crate) struct Booted {
    pub(crate) app: App,
    _dir: tempfile::TempDir,
    _log: std::sync::MutexGuard<'static, ()>,
}

/// The project whose `main.toml` is `scene` and whose `scripts/s.rn` is
/// `script`, loaded and not yet ticked.
pub(crate) fn project(scene: &str, script: &str) -> Booted {
    let log = crate::LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), scene).unwrap();
    std::fs::write(dir.path().join("scripts/s.rn"), script).unwrap();
    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = balaur::standard_app(balaur::AppConfig::dev(
        dir.path().to_string_lossy().as_ref(),
    ))
    .unwrap();
    app.load_project().unwrap();
    Booted {
        app,
        _dir: dir,
        _log: log,
    }
}

/// Every error logged since the project booted, with its fields.
pub(crate) fn errors() -> Vec<String> {
    balaur_core::logbuf::recent(80)
        .into_iter()
        .filter(|e| e.level.eq_ignore_ascii_case("error"))
        .map(|e| format!("{} {:?}", e.message, e.fields))
        .collect()
}

impl Booted {
    pub(crate) fn node(&self, path: &str) -> Entity {
        balaur_core::scene::find_node(&self.app.engine.world(), self.app.engine.root(), path)
            .unwrap_or_else(|| panic!("the scene has no {path}"))
    }

    pub(crate) fn tick(&mut self, frames: usize) {
        for _ in 0..frames {
            self.app.tick(1.0 / 60.0);
        }
    }

    /// What `function` on `node`'s script answers.
    pub(crate) fn call(&self, node: Entity, function: &str) -> balaur_script::Value {
        let host = self.app.engine.script_host().expect("a script host");
        host.call_on(balaur_core::node_id_of(node), function, &[])
            .unwrap_or_else(|| panic!("{function} answered nothing: {:#?}", errors()))
    }

    pub(crate) fn position(&self, node: Entity) -> [f32; 3] {
        let world = self.app.engine.world();
        let t = world.get::<&balaur_core::Transform>(node).unwrap();
        t.position.to_array()
    }

    pub(crate) fn get(&self, node: Entity, component: &str) -> toml::Value {
        balaur_core::components::get(&self.app.engine, node, component)
            .unwrap_or_else(|| panic!("{component} reports nothing"))
    }
}

/// One entry of a script map, by key.
pub(crate) fn entry<'a>(value: &'a balaur_script::Value, key: &str) -> &'a balaur_script::Value {
    match value {
        balaur_script::Value::Map(fields) => fields
            .iter()
            .find(|(k, _)| k == key)
            .map_or_else(|| panic!("no {key} in {value:?}"), |(_, v)| v),
        other => panic!("not a map: {other:?}"),
    }
}

/// A script number, whichever way it was spelled.
pub(crate) fn number(value: &balaur_script::Value) -> f64 {
    match value {
        balaur_script::Value::Num(n) => *n,
        balaur_script::Value::Int(n) => *n as f64,
        other => panic!("not a number: {other:?}"),
    }
}

/// A float out of a component's read-back.
pub(crate) fn float(table: &toml::Value, key: &str) -> f64 {
    table
        .get(key)
        .and_then(balaur_core::components::as_f64)
        .unwrap_or_else(|| panic!("no number {key} in {table}"))
}

/// The floats of a vector property in a component's read-back.
pub(crate) fn floats(table: &toml::Value, key: &str) -> Vec<f64> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("no list {key} in {table}"))
        .iter()
        .filter_map(balaur_core::components::as_f64)
        .collect()
}
