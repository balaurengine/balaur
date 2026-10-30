//! A scene written back from its nodes (PLAN-editor-as-scene §10.1): each node
//! as it was asked for, a prefab by what it changes, files as their project
//! spells them.

use balaur_core::components::ComponentDef;
use balaur_core::scene::find_node;
use balaur_core::{App, AppConfig, Engine};

struct Marker(toml::Value);

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
    let mut app = App::new(AppConfig::bare(dir.to_path_buf())).unwrap();
    app.register_component(
        "marker",
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "",
            schema: ComponentDef::parse_schema(
                "marker",
                r#"label = { type = "string", default = "none" }
size = { type = "float", default = 1.0 }
picture = { type = "string", default = "" }"#,
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

[nodes.transform]
position = [1.0, 0.0, 0.0]

[nodes.marker]
label = "arm"
size = 3.0

[[nodes]]
id = "n_hand"
name = "Hand"
parent = "Body/Arm"

[nodes.marker]
label = "hand"
"#;

/// The scene's root: the first node built under the engine's.
fn scene_root(app: &App) -> hecs::Entity {
    let world = app.engine.world();
    world
        .get::<&balaur_core::Children>(app.engine.root())
        .unwrap()
        .0[0]
}

fn written(app: &App) -> toml::Table {
    balaur_core::scene_toml::to_toml(&app.engine, scene_root(app), None).unwrap()
}

fn nodes(document: &toml::Table) -> Vec<toml::Table> {
    document["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node.as_table().unwrap().clone())
        .collect()
}

fn load(dir: &std::path::Path, scene: &str) -> App {
    let app = app_in(dir);
    let root = app.engine.root();
    balaur_core::project::instantiate_scene(&app.engine, scene, root, false).unwrap();
    app
}

#[test]
fn a_scene_is_written_as_it_was_asked_for() {
    let dir = project(&[("scripts/spin.bs", "")]);
    let app = load(
        dir.path(),
        r#"
[[nodes]]
id = "n_world"
name = "World"
tags = ["level"]

[[nodes]]
id = "n_box"
name = "Box"
parent = "n_world"
visible = false
z_index = 3
script = { source = "scripts/spin.bs", props = { speed = 2.0 } }
plugin_key = { kept = true }

[nodes.marker]
label = "box"
"#,
    );
    let document = written(&app);
    let nodes = nodes(&document);
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["id"].as_str(), Some("n_world"));
    assert!(nodes[0].get("parent").is_none(), "the root names no parent");
    assert_eq!(nodes[0]["tags"].as_array().unwrap().len(), 1);
    let node = &nodes[1];
    assert_eq!(node["parent"].as_str(), Some("n_world"));
    assert_eq!(node["visible"].as_bool(), Some(false));
    assert_eq!(node["z_index"].as_integer(), Some(3));
    assert!(node.get("tint").is_none(), "a default is not written");
    let marker = node["marker"].as_table().unwrap();
    assert_eq!(
        marker.len(),
        1,
        "what was asked, not every default: {marker:?}"
    );
    assert_eq!(marker["label"].as_str(), Some("box"));
    assert_eq!(
        node["script"]["source"].as_str(),
        Some("scripts/spin.bs"),
        "a script held unattached is still the node's"
    );
    assert_eq!(node["script"]["props"]["speed"].as_float(), Some(2.0));
    assert_eq!(
        node["plugin_key"]["kept"].as_bool(),
        Some(true),
        "a key nothing here handles is kept for the build that does"
    );
    // What it writes builds the same scene again.
    let again = app_in(dir.path());
    let root = again.engine.root();
    balaur_core::project::instantiate_scene(
        &again.engine,
        &toml::to_string(&document).unwrap(),
        root,
        false,
    )
    .unwrap();
    assert_eq!(written(&again), document);
}

#[test]
fn a_write_since_the_load_is_what_is_written() {
    let dir = project(&[]);
    let app = load(
        dir.path(),
        r#"
[[nodes]]
id = "n_box"
name = "Box"

[nodes.marker]
label = "box"
"#,
    );
    let entity = scene_root(&app);
    balaur_core::components::patch(
        &app.engine,
        entity,
        "marker",
        &toml::Value::Table(toml::toml! { size = 4.0 }),
    )
    .unwrap();
    let nodes = nodes(&written(&app));
    let marker = nodes[0]["marker"].as_table().unwrap();
    assert_eq!(marker["label"].as_str(), Some("box"));
    assert_eq!(marker["size"].as_float(), Some(4.0));
}

#[test]
fn a_prefab_is_written_as_its_instance_and_what_it_changed() {
    let dir = project(&[("scenes/enemy.toml", ENEMY)]);
    let app = load(
        dir.path(),
        r#"
[[nodes]]
id = "n_scene"
name = "Scene"

[[nodes]]
id = "n_enemy"
name = "Enemy"
parent = "n_scene"
instance = "scenes/enemy.toml"

[nodes.overrides.Arm.marker]
size = 5.0

[nodes.overrides.Gone.marker]
label = "kept"
"#,
    );
    let hand = {
        let world = app.engine.world();
        find_node(&world, scene_root(&app), "Enemy/Arm/Hand").unwrap()
    };
    balaur_core::components::patch(
        &app.engine,
        hand,
        "marker",
        &toml::Value::Table(toml::toml! { label = "fist" }),
    )
    .unwrap();
    let nodes = nodes(&written(&app));
    assert_eq!(
        nodes.len(),
        2,
        "the prefab's nodes are not the file's: {nodes:?}"
    );
    let enemy = &nodes[1];
    assert_eq!(enemy["instance"].as_str(), Some("scenes/enemy.toml"));
    assert!(
        enemy.get("marker").is_none(),
        "the prefab root's component is the prefab's"
    );
    let overrides = enemy["overrides"].as_table().unwrap();
    assert_eq!(overrides["Arm"]["marker"]["size"].as_float(), Some(5.0));
    assert_eq!(
        overrides["Arm"]["marker"].as_table().unwrap().len(),
        1,
        "only the property it changed"
    );
    assert_eq!(
        overrides["Arm/Hand"]["marker"]["label"].as_str(),
        Some("fist"),
        "an edit inside the instance is an override"
    );
    assert_eq!(
        overrides["Gone"]["marker"]["label"].as_str(),
        Some("kept"),
        "an override naming nothing is kept as the file held it"
    );
}

#[test]
fn files_are_written_as_their_project_spells_them() {
    let dir = project(&[
        ("pictures/ship.png", ""),
        ("pictures/flag.png", ""),
        ("assets/index.toml", "a1b2 = \"pictures/flag.png\"\n"),
    ]);
    let editor = tempfile::tempdir().unwrap();
    let app = app_in(editor.path());
    let root = app.engine.root();
    balaur_core::project::instantiate_scene_from_root(
        &app.engine,
        r#"
[[nodes]]
id = "n_ship"
name = "Ship"

[nodes.marker]
picture = "pictures/ship.png"

[[nodes]]
id = "n_flag"
name = "Flag"
parent = "n_ship"

[nodes.marker]
picture = "id://a1b2"
"#,
        root,
        false,
        dir.path(),
    )
    .unwrap();
    let ship = scene_root(&app);
    let read = app
        .engine
        .world()
        .get::<&Marker>(ship)
        .unwrap()
        .0
        .get("picture")
        .and_then(toml::Value::as_str)
        .unwrap()
        .to_string();
    assert!(read.starts_with(dir.path().to_str().unwrap()), "{read}");
    let document = balaur_core::scene_toml::to_toml(&app.engine, ship, Some(dir.path())).unwrap();
    let nodes = nodes(&document);
    assert_eq!(
        nodes[0]["marker"]["picture"].as_str(),
        Some("pictures/ship.png")
    );
    assert_eq!(
        nodes[1]["marker"]["picture"].as_str(),
        Some("id://a1b2"),
        "an id is written as the id"
    );
}

#[test]
fn a_node_spawned_since_is_written_with_an_id_of_its_name() {
    let dir = project(&[]);
    let app = load(
        dir.path(),
        r#"
[[nodes]]
id = "n_world"
name = "World"
"#,
    );
    let world_node = scene_root(&app);
    let child =
        balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Lamp Post", world_node);
    let minted = balaur_core::ids::of(&app.engine.world(), child).unwrap();
    assert!(balaur_core::ids::is_minted(&minted));
    let nodes = nodes(&written(&app));
    assert_eq!(nodes[1]["id"].as_str(), Some("n_lamp_post"));
    assert_eq!(nodes[1]["parent"].as_str(), Some("n_world"));
}

#[test]
fn a_node_of_another_projects_content_names_files_as_it_spells_them() {
    let dir = project(&[("pictures/ship.png", ""), ("scripts/spin.bs", "")]);
    let editor = tempfile::tempdir().unwrap();
    let app = app_in(editor.path());
    let root = app.engine.root();
    let mirror = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Edited", root);
    balaur_core::project::set_content_root(&app.engine, dir.path().to_str().unwrap());
    app.engine.set_debug_scope(Some(mirror));
    balaur_core::project::instantiate_scene_from_root(
        &app.engine,
        "[[nodes]]\nid = \"n_ship\"\nname = \"Ship\"\n",
        mirror,
        false,
        dir.path(),
    )
    .unwrap();
    let ship = app
        .engine
        .world()
        .get::<&balaur_core::Children>(mirror)
        .unwrap()
        .0[0];
    balaur_core::components::add(
        &app.engine,
        ship,
        "marker",
        Some(&toml::Value::Table(
            toml::toml! { picture = "pictures/ship.png" },
        )),
    )
    .unwrap();
    let held = app.engine.world().get::<&Marker>(ship).unwrap().0["picture"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        held.starts_with(dir.path().to_str().unwrap()),
        "the component loads the game's file: {held}"
    );
    let document = balaur_core::scene_toml::to_toml(&app.engine, ship, Some(dir.path())).unwrap();
    assert_eq!(
        nodes(&document)[0]["marker"]["picture"].as_str(),
        Some("pictures/ship.png")
    );
}
