use balaur_core::{App, AppConfig, Engine, Stage};
use balaur_plugin::{load, load_order, Fingerprint, Manifest, Plugin, Registry};
use balaur_script::BindingsExt as _;

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

struct Marker(u32);

fn bump_marker_system(eng: &Engine, _: f32) {
    eng.resource::<Marker>().borrow_mut().0 += 1;
}

struct Probe {
    manifest: Manifest,
}

impl Probe {
    fn new(name: &str) -> Self {
        Self {
            manifest: Manifest::new(name, "1.0.0"),
        }
    }

    fn from_the_future(name: &str) -> Self {
        let mut manifest = Manifest::new(name, "1.0.0");
        manifest.fingerprint.engine = "999.0.0".into();
        Self { manifest }
    }
}

impl Plugin for Probe {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut Registry<'_>) -> anyhow::Result<()> {
        reg.insert_resource(Marker(7));
        reg.add_system(Stage::Update, bump_marker_system);
        let name = reg.plugin().to_string();
        let mut m = reg.script_module(&name)?;
        m.function("answer", |_: &Engine, ()| Ok(42i64));
        Ok(())
    }
}

#[test]
fn a_plugin_registers_its_resources_and_systems() {
    let mut app = app();
    load(&mut app, &mut Probe::new("probe")).unwrap();

    assert_eq!(app.engine.resource::<Marker>().borrow().0, 7);
    app.tick(1.0 / 60.0);
    assert_eq!(
        app.engine.resource::<Marker>().borrow().0,
        8,
        "the system did not run"
    );
}

#[test]
fn a_build_from_another_engine_version_is_refused_by_name() {
    let mut app = app();
    let Err(err) = load(&mut app, &mut Probe::from_the_future("stale")) else {
        panic!("a mismatched build was loaded");
    };
    let text = format!("{err:#}");
    assert!(text.contains("stale"), "does not name the plugin: {text}");
    assert!(
        text.contains("999.0.0"),
        "does not name the version it wants: {text}"
    );
}

#[test]
fn a_matching_fingerprint_reports_no_differences() {
    let host = Fingerprint::current();
    assert!(host.differences(&Fingerprint::current()).is_empty());
}

#[test]
fn each_part_of_a_fingerprint_is_reported_separately() {
    let host = Fingerprint::current();
    let mut other = Fingerprint::current();
    other.rustc = "1.0.0".into();
    other.registry_abi = 99;
    let differences = host.differences(&other);
    assert_eq!(differences.len(), 2, "{differences:?}");
    assert!(differences.iter().any(|d| d.contains("rustc")));
    assert!(differences.iter().any(|d| d.contains("abi")));
}

#[test]
fn dependencies_load_before_the_plugins_that_need_them() {
    let manifests = vec![
        Manifest::new("game", "1").requiring(&["physics"]),
        Manifest::new("physics", "1").requiring(&["core"]),
        Manifest::new("core", "1"),
    ];
    assert_eq!(load_order(&manifests).unwrap(), ["core", "physics", "game"]);
}

#[test]
fn independent_plugins_load_in_name_order() {
    let manifests = vec![
        Manifest::new("zebra", "1"),
        Manifest::new("alpha", "1"),
        Manifest::new("middle", "1"),
    ];
    assert_eq!(
        load_order(&manifests).unwrap(),
        ["alpha", "middle", "zebra"]
    );
}

#[test]
fn the_order_does_not_depend_on_how_the_plugins_were_listed() {
    let one = vec![
        Manifest::new("a", "1").requiring(&["b"]),
        Manifest::new("b", "1"),
        Manifest::new("c", "1"),
    ];
    let other = vec![
        Manifest::new("c", "1"),
        Manifest::new("b", "1"),
        Manifest::new("a", "1").requiring(&["b"]),
    ];
    assert_eq!(load_order(&one).unwrap(), load_order(&other).unwrap());
}

#[test]
fn a_missing_requirement_names_what_is_absent() {
    let manifests = vec![Manifest::new("game", "1").requiring(&["nowhere"])];
    let err = load_order(&manifests).unwrap_err().to_string();
    assert!(
        err.contains("nowhere") && err.contains("game"),
        "unhelpful: {err}"
    );
}

#[test]
fn plugins_that_require_each_other_are_refused() {
    let manifests = vec![
        Manifest::new("a", "1").requiring(&["b"]),
        Manifest::new("b", "1").requiring(&["a"]),
    ];
    let err = load_order(&manifests).unwrap_err().to_string();
    assert!(err.contains("cycle"), "unhelpful: {err}");
}

#[test]
fn a_plugin_with_no_requirements_still_loads() {
    assert_eq!(load_order(&[Manifest::new("solo", "1")]).unwrap(), ["solo"]);
    assert!(load_order(&[]).unwrap().is_empty());
}
