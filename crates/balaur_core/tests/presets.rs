//! Preset definitions parsed from a project's `presets.toml`.
//!
//! Only the parsing lives here: applying a preset needs the plugins that own
//! the components, so those tests are in the facade crate.

use balaur_core::components::ComponentDef;
use balaur_core::{components, presets, scene, App, AppConfig};

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

/// Two components that behave like real ones -- each owns storage, so
/// "present" means present. `lonely` applies fine on its own but only does
/// something when `partner` is there. No built-in component is shaped like
/// this today, so the mechanism is exercised against one the test registers.
struct Lonely;
struct Partner;

fn empty_table() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

fn register_pair(app: &mut App) {
    app.register_component(
        "lonely",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "lonely",
                r#"on = { type = "bool", default = true }"#,
            ),
            tags: &["test"],
            expects: &["partner"],
            apply: Box::new(|eng, entity, _| {
                let _ = eng.world_mut().insert_one(entity, Lonely);
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Lonely>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                eng.world()
                    .get::<&Lonely>(entity)
                    .ok()
                    .map(|_| empty_table())
            }),
        },
    );
    app.register_component(
        "partner",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "partner",
                r#"on = { type = "bool", default = true }"#,
            ),
            tags: &["test"],
            expects: &[],
            apply: Box::new(|eng, entity, _| {
                let _ = eng.world_mut().insert_one(entity, Partner);
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<Partner>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                eng.world()
                    .get::<&Partner>(entity)
                    .ok()
                    .map(|_| empty_table())
            }),
        },
    );
}

/// The warning fires only while the companion is missing, and never blocks:
/// the component applied, and adding the partner later clears it.
#[test]
fn an_expectation_warns_while_unmet_and_clears_when_met() {
    let mut app = app();
    register_pair(&mut app);
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "N", root);

    components::add(&app.engine, entity, "lonely", None).unwrap();
    let unmet = presets::unmet_expectations(&app.engine, entity);
    assert_eq!(unmet.len(), 1, "{unmet:?}");
    assert_eq!(unmet[0].0, "lonely");
    assert_eq!(unmet[0].1, vec!["partner".to_string()]);

    components::add(&app.engine, entity, "partner", None).unwrap();
    assert!(
        presets::unmet_expectations(&app.engine, entity).is_empty(),
        "adding the companion later clears it"
    );
}

/// Nothing to warn about is the normal case, and has to stay quiet.
#[test]
fn a_component_with_no_expectations_never_warns() {
    let mut app = app();
    register_pair(&mut app);
    let root = app.engine.root();
    let entity = scene::spawn_node(&mut app.engine.world_mut(), "N", root);
    components::add(&app.engine, entity, "partner", None).unwrap();
    assert!(presets::unmet_expectations(&app.engine, entity).is_empty());
}

#[test]
fn a_project_preset_is_parsed_from_toml() {
    let body = r#"
description = "A patrolling enemy"
tags = ["2d"]
components = [
  { component = "shape2d", kind = "rect" },
  { component = "color" },
]
"#;
    let body: toml::Value = toml::from_str(body).unwrap();
    let def = presets::from_toml("enemy", &body).unwrap();
    assert_eq!(def.description, "A patrolling enemy");
    assert_eq!(def.tags, vec!["2d".to_string()]);
    assert_eq!(def.parts.len(), 2);
    assert_eq!(def.parts[0].component, "shape2d");
    // The discriminant is stripped; what is left is the component's own table.
    let params = def.parts[0].params.as_ref().unwrap();
    assert!(params.get("component").is_none());
    assert_eq!(params["kind"].as_str().unwrap(), "rect");
    assert!(def.parts[1].params.is_none(), "no properties means none");
}

#[test]
fn a_project_preset_without_components_is_an_error() {
    let body: toml::Value = toml::from_str("description = \"x\"").unwrap();
    let err = presets::from_toml("broken", &body).unwrap_err();
    assert!(err.to_string().contains("components"), "{err}");
}

/// A host that answers for the pack's text map and does nothing else, which
/// is how a packed run reaches every document it did not compile.
struct PackedHost(balaur_core::Pack);

impl balaur_script::ScriptHost<balaur_core::Engine> for PackedHost {
    fn module(
        &self,
        _: &str,
    ) -> anyhow::Result<Box<dyn balaur_script::Bindings<balaur_core::Engine>>> {
        Ok(Box::new(balaur_script::NoBindings))
    }
    fn attach_with_props(
        &self,
        _: balaur_script::NodeId,
        _: &str,
        _: &[(String, balaur_script::Value)],
    ) -> anyhow::Result<()> {
        Ok(())
    }
    fn detach(&self, _: balaur_script::NodeId) {}
    fn update(&self, _: f32) {}
    fn fixed_update(&self, _: f32) {}
    fn save_state(&self) -> Vec<(balaur_script::NodeId, balaur_script::Value)> {
        Vec::new()
    }
    fn load_state(&self, _: &[(balaur_script::NodeId, balaur_script::Value)]) {}
    fn pump_reloads(&self) {}
    fn reload(&self, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn call_on(
        &self,
        _: balaur_script::NodeId,
        _: &str,
        _: &[balaur_script::Value],
    ) -> Option<balaur_script::Value> {
        None
    }
    fn call_all(&self, _: &str) {}
    fn wake(&self, _: u64, _: &balaur_script::Value) {}
    fn scene_source(&self, rel: &str) -> Option<String> {
        self.0.scenes.get(rel).cloned()
    }
    fn instance_count(&self) -> usize {
        0
    }
    fn invoke(
        &self,
        _: balaur_script::CallbackId,
        _: &[balaur_script::Value],
    ) -> anyhow::Result<balaur_script::Value> {
        Ok(balaur_script::Value::Nil)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct NoScripts;

impl balaur_script::ScriptCompiler for NoScripts {
    fn extensions(&self) -> &[&str] {
        &[]
    }
    fn compile(&self, _: &str, _: &str) -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }
}

/// A shipped game had no presets at all: `ProjectFiles` serves the pack's
/// *assets*, and `presets.toml` is filed with the pack's documents.
#[test]
fn a_packed_run_resolves_a_project_preset_as_a_dev_run_does() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"p\"\nmain_scene = \"m.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("m.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("presets.toml"),
        "[crate]\ncomponents = [{ component = \"lonely\", on = true }]\n",
    )
    .unwrap();

    let pack = balaur_core::Pack::build(dir.path(), &NoScripts).unwrap();
    // A project root that does not exist, so nothing can fall back to disk.
    let mut packed = App::new(AppConfig {
        project_root: dir.path().join("shipped"),
        pack: Some(pack.clone()),
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    register_pair(&mut packed);
    packed
        .engine
        .set_script_host(std::rc::Rc::new(PackedHost(pack)));
    packed.load_project().unwrap();

    assert!(
        presets::names(&packed.engine).contains(&String::from("crate")),
        "a shipped game gets the presets its project declared"
    );
}
