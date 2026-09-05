use balaur_core::{App, AppConfig, Engine, Stage};
use balaur_plugin::{load, load_all, load_order, Fingerprint, Manifest, Plugin, Registry};
use balaur_script::BindingsExt as _;

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
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

    fn requiring(name: &str, needs: &[&str]) -> Self {
        Self {
            manifest: Manifest::new(name, "1.0.0").requiring(needs),
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
    assert_eq!(
        load_order(&manifests, &[]).unwrap(),
        ["core", "physics", "game"]
    );
}

#[test]
fn independent_plugins_load_in_name_order() {
    let manifests = vec![
        Manifest::new("zebra", "1"),
        Manifest::new("alpha", "1"),
        Manifest::new("middle", "1"),
    ];
    assert_eq!(
        load_order(&manifests, &[]).unwrap(),
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
    assert_eq!(
        load_order(&one, &[]).unwrap(),
        load_order(&other, &[]).unwrap()
    );
}

#[test]
fn a_missing_requirement_names_what_is_absent() {
    let manifests = vec![Manifest::new("game", "1").requiring(&["nowhere"])];
    let err = load_order(&manifests, &[]).unwrap_err().to_string();
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
    let err = load_order(&manifests, &[]).unwrap_err().to_string();
    assert!(err.contains("cycle"), "unhelpful: {err}");
}

#[test]
fn a_plugin_with_no_requirements_still_loads() {
    assert_eq!(
        load_order(&[Manifest::new("solo", "1")], &[]).unwrap(),
        ["solo"]
    );
    assert!(load_order(&[], &[]).unwrap().is_empty());
}

/// The check that decides whether loading foreign code is safe is only worth
/// something if it knows which compiler built this. It used to fall back to a
/// placeholder both sides agreed on, which made every comparison pass.
#[test]
fn the_fingerprint_names_a_real_compiler() {
    let rustc = &Fingerprint::current().rustc;
    assert_ne!(rustc, "unknown", "the build script did not stamp a version");
    assert!(!rustc.is_empty(), "the compiler version is empty");
    assert!(
        rustc.starts_with(|c: char| c.is_ascii_digit()),
        "does not look like a rustc version: {rustc}"
    );
}

/// `AbiTag` carries the fingerprint in fixed-size fields, and a value too long
/// for one is silently cut short — two compilers differing only past the cut
/// would compare equal. The round trip is what proves this build fits.
#[test]
fn the_fingerprint_survives_the_fixed_size_abi_tag() {
    assert_eq!(
        balaur_plugin::AbiTag::current().fingerprint(),
        Fingerprint::current(),
        "a field was truncated on the way into the tag"
    );
}

#[test]
fn a_requirement_is_satisfied_by_a_module_loaded_before_the_set() {
    let manifests = vec![Manifest::new("mod_of_mine", "1").requiring(&["http"])];
    let loaded = ["http".to_string()];
    assert_eq!(
        load_order(&manifests, &loaded).unwrap(),
        ["mod_of_mine"],
        "an already-loaded module does not order with the set"
    );
    assert!(load_order(&manifests, &[]).is_err());
}

#[test]
fn loading_a_plugin_records_who_it_is() {
    let mut app = app();
    load(&mut app, &mut Probe::new("probe")).unwrap();

    let loaded = app.plugins();
    assert_eq!(loaded.len(), 1, "{loaded:?}");
    assert_eq!(loaded[0].name, "probe");
    assert_eq!(loaded[0].version, "1.0.0");
    assert!(balaur_core::plugins::is_loaded(&app.engine, "probe"));
}

#[test]
fn a_plugin_that_requires_an_unloaded_one_is_refused() {
    let mut app = app();
    let mut probe = Probe::new("late");
    probe.manifest = probe.manifest.clone().requiring(&["early"]);

    let err = load(&mut app, &mut probe).unwrap_err().to_string();
    assert!(
        err.contains("late") && err.contains("early"),
        "unhelpful: {err}"
    );
    assert!(
        app.plugins().is_empty(),
        "a refused plugin was recorded as loaded"
    );
}

#[test]
fn a_requirement_loaded_first_is_accepted() {
    let mut app = app();
    load(&mut app, &mut Probe::new("early")).unwrap();
    let mut probe = Probe::new("late");
    probe.manifest = probe.manifest.clone().requiring(&["early"]);

    load(&mut app, &mut probe).unwrap();
    assert_eq!(
        app.plugins()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["early", "late"]
    );
}

#[test]
fn a_plugin_whose_registration_fails_is_not_recorded() {
    let mut app = app();
    load(&mut app, &mut Failing::new("broken")).unwrap_err();
    assert!(app.plugins().is_empty());
}

struct Failing {
    manifest: Manifest,
}

impl Failing {
    fn new(name: &str) -> Self {
        Self {
            manifest: Manifest::new(name, "1.0.0"),
        }
    }
}

impl Plugin for Failing {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn declare(&mut self, _: &mut Registry<'_>) -> anyhow::Result<()> {
        anyhow::bail!("no")
    }
}

#[test]
fn a_set_loads_in_dependency_order_whatever_order_it_was_given_in() {
    let mut app = app();
    let mut set: Vec<Box<dyn Plugin>> = vec![
        Box::new(Probe::requiring("late", &["early"])),
        Box::new(Probe::new("early")),
    ];

    load_all(&mut app, &mut set).unwrap();
    assert_eq!(
        app.plugins()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["early", "late"]
    );
}

#[test]
fn a_set_with_a_missing_requirement_registers_none_of_itself() {
    let mut app = app();
    let mut set: Vec<Box<dyn Plugin>> = vec![
        Box::new(Probe::new("present")),
        Box::new(Probe::requiring("needy", &["absent"])),
    ];

    let err = load_all(&mut app, &mut set).unwrap_err().to_string();
    assert!(err.contains("absent"), "unhelpful: {err}");
    assert!(
        app.plugins().is_empty(),
        "half the set registered before the refusal"
    );
}

#[test]
fn a_set_may_require_something_loaded_before_it() {
    let mut app = app();
    load(&mut app, &mut Probe::new("early")).unwrap();
    let mut set: Vec<Box<dyn Plugin>> = vec![Box::new(Probe::requiring("late", &["early"]))];

    load_all(&mut app, &mut set).unwrap();
    assert!(balaur_core::plugins::is_loaded(&app.engine, "late"));
}

struct Configured {
    manifest: Manifest,
    seen: Option<toml::Table>,
}

impl Plugin for Configured {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut Registry<'_>) -> anyhow::Result<()> {
        self.seen = reg.config();
        Ok(())
    }
}

#[test]
fn a_plugin_reads_the_table_the_project_handed_it() {
    let mut app = app();
    let mut configs = balaur_core::PluginConfigs::default();
    configs
        .0
        .insert("probe".to_string(), toml::from_str("timeout = 5").unwrap());
    configs
        .0
        .insert("other".to_string(), toml::from_str("nope = 1").unwrap());
    app.engine.insert_resource(configs);

    let mut plugin = Configured {
        manifest: Manifest::new("probe", "1"),
        seen: None,
    };
    load(&mut app, &mut plugin).unwrap();

    let seen = plugin.seen.expect("no table reached the plugin");
    assert_eq!(
        seen.get("timeout").and_then(toml::Value::as_integer),
        Some(5)
    );
    assert!(seen.get("nope").is_none(), "read another plugin's table");
}

#[test]
fn a_plugin_given_no_table_reads_nothing() {
    let mut app = app();
    let mut plugin = Configured {
        manifest: Manifest::new("probe", "1"),
        seen: None,
    };

    load(&mut app, &mut plugin).unwrap();
    assert!(plugin.seen.is_none());
}
