//! Prefabs: the `instance` scene key, the ids that keep two instances of one
//! file apart, and the `overrides` that make them differ.

use balaur_core::components::{ComponentDef, StableId};
use balaur_core::scene::{find_node, Children, Name};
use balaur_core::{App, AppConfig, Engine};

struct Marker(toml::Value);

/// A project on disk, because a prefab is read by path the way a scene is.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn app_in(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.register_component(
        "marker",
        ComponentDef {
            doc: "",
            schema: ComponentDef::parse_schema(
                "marker",
                r#"label = { type = "string", default = "none" }
size = { type = "float", default = 1.0 }"#,
            ),
            tags: &[],
            expects: &[],
            apply: Box::new(|eng: &Engine, entity, params| {
                eng.world_mut()
                    .insert_one(entity, Marker(params.clone()))
                    .map_err(|_| anyhow::anyhow!("dead node"))?;
                Ok(())
            }),
            remove: Box::new(|eng: &Engine, entity| {
                let _ = eng.world_mut().remove_one::<Marker>(entity);
                Ok(())
            }),
            get: Box::new(|eng: &Engine, entity| {
                eng.world().get::<&Marker>(entity).ok().map(|m| m.0.clone())
            }),
        },
    );
    app
}

const ENEMY: &str = r#"
[[nodes]]
id = "n_body"
name = "Body"

[nodes.marker]
label = "body"

[[nodes]]
id = "n_arm"
name = "Arm"
parent = "n_body"
position = [1.0, 0.0, 0.0]

[nodes.marker]
label = "arm"
size = 3.0
"#;

fn label(app: &App, path: &str) -> Option<String> {
    let world = app.engine.world();
    let entity = find_node(&world, app.engine.root(), path)?;
    let marker = world.get::<&Marker>(entity).ok()?;
    Some(marker.0.get("label")?.as_str()?.to_string())
}

fn stable_id(app: &App, path: &str) -> Option<String> {
    let world = app.engine.world();
    let entity = find_node(&world, app.engine.root(), path)?;
    let id = world.get::<&StableId>(entity).ok()?;
    Some(id.0.clone())
}

fn load(scene: &str) -> (tempfile::TempDir, App) {
    let dir = project(&[("scenes/enemy.toml", ENEMY)]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(&app.engine, scene, root, false).unwrap();
    (dir, app)
}

#[test]
fn an_instance_builds_the_prefab_under_the_node_that_names_it() {
    let (_dir, app) = load(
        r#"
[[nodes]]
id = "n_enemy"
name = "Enemy"
instance = "scenes/enemy.toml"
position = [4.0, 0.0, 0.0]
"#,
    );
    assert_eq!(label(&app, "Enemy/Body"), Some(String::from("body")));
    assert_eq!(label(&app, "Enemy/Body/Arm"), Some(String::from("arm")));
}

#[test]
fn two_instances_of_one_prefab_differ_only_where_overridden() {
    let (_dir, app) = load(
        r#"
[[nodes]]
id = "n_left"
name = "Left"
instance = "scenes/enemy.toml"

[nodes.overrides."Body/Arm".marker]
label = "left arm"

[[nodes]]
id = "n_right"
name = "Right"
instance = "scenes/enemy.toml"
"#,
    );
    assert_eq!(label(&app, "Left/Body/Arm"), Some(String::from("left arm")));
    assert_eq!(label(&app, "Right/Body/Arm"), Some(String::from("arm")));
    // Untouched by either override, so both still carry the prefab's own.
    assert_eq!(label(&app, "Left/Body"), Some(String::from("body")));
    assert_eq!(label(&app, "Right/Body"), Some(String::from("body")));
}

#[test]
fn ids_inside_an_instance_are_prefixed_by_the_instance() {
    let (_dir, app) = load(
        r#"
[[nodes]]
id = "n_left"
name = "Left"
instance = "scenes/enemy.toml"

[[nodes]]
id = "n_right"
name = "Right"
instance = "scenes/enemy.toml"
"#,
    );
    assert_eq!(stable_id(&app, "Left"), Some(String::from("n_left")));
    assert_eq!(
        stable_id(&app, "Left/Body/Arm"),
        Some(String::from("n_left/n_arm"))
    );
    assert_eq!(
        stable_id(&app, "Right/Body/Arm"),
        Some(String::from("n_right/n_arm")),
        "the same node in another instance must not share an id"
    );
}

#[test]
fn an_override_reaches_the_transform_too() {
    let (_dir, app) = load(
        r#"
[[nodes]]
id = "n_enemy"
name = "Enemy"
instance = "scenes/enemy.toml"

[nodes.overrides."Body/Arm"]
position = [0.0, 2.0, 0.0]
"#,
    );
    let world = app.engine.world();
    let arm = find_node(&world, app.engine.root(), "Enemy/Body/Arm").unwrap();
    let t = world.get::<&balaur_core::scene::Transform>(arm).unwrap();
    assert!((t.position.y - 2.0).abs() < 1e-6, "{:?}", t.position);
    assert!(t.position.x.abs() < 1e-6, "the prefab's x was replaced");
}

/// The prefab may have moved the node an override names. Losing the edit
/// silently is the one outcome worth ruling out; the scene still loads.
#[test]
fn an_override_naming_nothing_is_not_fatal() {
    let (_dir, app) = load(
        r#"
[[nodes]]
id = "n_enemy"
name = "Enemy"
instance = "scenes/enemy.toml"

[nodes.overrides."Body/Leg".marker]
label = "gone"
"#,
    );
    assert_eq!(label(&app, "Enemy/Body"), Some(String::from("body")));
}

#[test]
fn a_prefab_that_contains_itself_is_an_error_not_a_hang() {
    let dir = project(&[(
        "scenes/loop.toml",
        r#"
[[nodes]]
id = "n_self"
name = "Self"
instance = "scenes/loop.toml"
"#,
    )]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    let err = balaur_core::project::instantiate_scene(
        &app.engine,
        r#"
[[nodes]]
id = "n_outer"
name = "Outer"
instance = "scenes/loop.toml"
"#,
        root,
        false,
    )
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("cannot contain itself"),
        "{err:#}"
    );
}

#[test]
fn a_prefab_inside_a_prefab_prefixes_twice() {
    let dir = project(&[
        ("scenes/enemy.toml", ENEMY),
        (
            "scenes/squad.toml",
            r#"
[[nodes]]
id = "n_leader"
name = "Leader"
instance = "scenes/enemy.toml"
"#,
        ),
    ]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(
        &app.engine,
        r#"
[[nodes]]
id = "n_squad"
name = "Squad"
instance = "scenes/squad.toml"
"#,
        root,
        false,
    )
    .unwrap();
    assert_eq!(
        stable_id(&app, "Squad/Leader/Body/Arm"),
        Some(String::from("n_squad/n_leader/n_arm"))
    );
}

#[test]
fn the_instance_node_keeps_its_own_name_and_children() {
    let (_dir, app) = load(
        r#"
[[nodes]]
id = "n_enemy"
name = "Enemy"
instance = "scenes/enemy.toml"
"#,
    );
    let world = app.engine.world();
    let enemy = find_node(&world, app.engine.root(), "Enemy").unwrap();
    assert_eq!(world.get::<&Name>(enemy).unwrap().0, "Enemy");
    assert_eq!(
        world.get::<&Children>(enemy).unwrap().0.len(),
        1,
        "the prefab's roots are the instance's children"
    );
}

/// The most useful override there is: the same prefab, tuned. Asserted here
/// on the scene key rather than on a live instance — `balaur_script_rune`'s
/// `props` suite runs the script half.
#[test]
fn a_script_key_with_no_source_is_refused_on_a_node_of_its_own() {
    let dir = project(&[("scenes/enemy.toml", ENEMY)]);
    let app = app_in(dir.path());
    let root = app.engine.root();
    let err = balaur_core::project::instantiate_scene(
        &app.engine,
        "[[nodes]]\nname = \"Enemy\"\nscript = { props = { speed = 1.0 } }\n",
        root,
        false,
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("no source"), "{err:#}");
}

fn size(app: &App, path: &str) -> Option<f64> {
    let world = app.engine.world();
    let entity = find_node(&world, app.engine.root(), path)?;
    let marker = world.get::<&Marker>(entity).ok()?;
    marker.0.get("size")?.as_float()
}

/// An override changes one property of a component the prefab described whole.
/// Applied as an `add` it would put every other property back to the schema
/// default, which is a scene silently losing what the prefab said.
#[test]
fn an_override_of_one_property_leaves_the_others_alone() {
    let (_dir, app) = load(
        r#"
[[nodes]]
id = "n_enemy"
name = "Enemy"
instance = "scenes/enemy.toml"

[nodes.overrides."Body/Arm".marker]
label = "left arm"
"#,
    );
    assert_eq!(
        label(&app, "Enemy/Body/Arm"),
        Some(String::from("left arm"))
    );
    assert_eq!(
        size(&app, "Enemy/Body/Arm"),
        Some(3.0),
        "the prefab's size survived an override of its label"
    );
}
