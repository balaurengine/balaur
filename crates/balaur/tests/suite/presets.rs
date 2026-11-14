//! Tags, presets and expectation warnings.
//!
//! These three exist so a flat list of components is navigable: what a
//! component is *about* (tags), a shortcut for the combinations people
//! actually want (presets), and a nudge when a combination is incomplete
//! (expectations). None of them adds a type -- a node is still exactly the
//! components on it. Tags and expectations are the engine's; the presets are
//! the editor's data, `editor/presets.toml`, built with `set_component`.

use balaur::{App, AppConfig, components, scene};

fn app() -> App {
    App::new(AppConfig::bare(".")).unwrap()
}

fn node(app: &App) -> balaur::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

fn def(app: &App, name: &str) -> components::ComponentDef {
    // Only the metadata is read here; cloning the boxed fns is not possible,
    // so the test reads through the registry instead.
    let registry = app.engine.resource::<components::ComponentRegistry>();
    let registry = registry.borrow();
    let d = registry.def(name).unwrap_or_else(|| panic!("no `{name}`"));
    components::ComponentDef {
        tags: d.tags,
        expects: d.expects,
        ..components::ComponentDef::by_hand(
            d.schema.clone(),
            Box::new(|_, _, _| Ok(())),
            Box::new(|_, _| Ok(())),
            Box::new(|_, _| None),
        )
    }
}

/// A tag is the thing a single category path cannot express: `collider2d` is
/// genuinely both, and forcing a choice buries whichever loses.
#[test]
fn a_component_can_carry_several_tags() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin).unwrap();
    let tags = def(&app, "collider2d").tags;
    assert!(tags.contains(&"2d"), "tags were {tags:?}");
    assert!(tags.contains(&"physics"), "tags were {tags:?}");
}

/// 3D carries no dimension marker (docs/NAMING.md D5), so the 3D body is
/// tagged `3d` while its name stays unmarked.
#[test]
fn dimension_tags_separate_the_two_worlds() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin).unwrap();
    assert!(def(&app, "body3d").tags.contains(&"3d"));
    assert!(def(&app, "body2d").tags.contains(&"2d"));
    assert!(!def(&app, "body3d").tags.contains(&"2d"));
}

/// Every component has to be findable under some facet, or the palette hides
/// it. This is the check that stops a new component shipping untagged.
#[test]
fn every_registered_component_is_tagged() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin).unwrap();
    let registry = app.engine.resource::<components::ComponentRegistry>();
    let registry = registry.borrow();
    for (name, def) in &*registry {
        assert!(!def.tags.is_empty(), "`{name}` has no tags");
    }
}

// The editor is a Balaur project, so it reaches all of this through the
// script API rather than through Rust. This is that surface.
#[test]
fn the_script_api_exposes_tags_and_warnings_and_builds_a_recipe() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    std::fs::write(
        dir.path().join("scripts/checks.bs"),
        r#"
        fn has(list: [str], item: str) -> bool {
            for x in list {
                if x == item {
                    return true;
                }
            }
            false
        }

        pub class Checks { node: Node, done: int, failed: bool }

        impl Checks {
            fn init() {
                let tags = scene::component_tags("collider2d").unwrap();
                self.check(has(tags, "2d"), "collider2d should be tagged 2d");
                self.check(has(tags, "physics"), "collider2d should be tagged physics");
                self.check(scene::component_tags("nope") == nil, "unknown component has no tags");

                // A recipe is its components set in order, as the editor's picker
                // builds one; nothing records that a recipe was used.
                let n = scene::root().add_child("Thing");
                n.set_component("body2d", { "kind": "dynamic" });
                n.set_component("collider2d");
                let present = n.component_names();
                self.check(has(present, "body2d"), "body2d not applied");
                self.check(has(present, "collider2d"), "collider2d not applied");
                self.check(n.body2d.kind == "dynamic", "wrong body kind");

                self.check(scene::warnings(n).len() == 0, "nothing should warn here");
                if !self.failed {
                    self.done = 1;
                }
            }

            fn check(ok: bool, what: str) {
                if !ok {
                    self.failed = true;
                    log::error("failed: {what}");
                }
            }
        }
        "#,
    )
    .unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let app = balaur::standard_app(config).unwrap();
    balaur::logbuf::capture_for_test();
    balaur::logbuf::clear();
    let entity = node(&app);
    app.engine
        .script_host()
        .unwrap()
        .attach(balaur::node_id_of(entity), "scripts/checks.bs")
        .unwrap();
    let host = app.engine.script_host().unwrap();
    assert_eq!(
        host.script_field(balaur::node_id_of(entity), "done"),
        Some(balaur_script::Value::Int(1)),
        "the script did not run to its end: {:#?}",
        balaur::logbuf::recent(10)
    );
}

/// What the engine's own preset registry held when the recipes moved to the
/// editor: each name, and the components it gave a node, in order.
#[rustfmt::skip]
const REGISTRY: &[(&str, &[&str])] = &[
    ("button", &["widget", "widget_check", "widget_button", "widget_text"]),
    ("checkbox", &["widget", "widget_check"]),
    ("cloth3d", &["soft_grid3d", "softbody3d"]),
    ("column", &["widget", "widget_layout"]),
    ("dialog", &["widget", "widget_window", "widget_layout"]),
    ("dropdown", &["widget", "widget_dropdown", "widget_text"]),
    ("flow", &["widget", "widget_layout"]),
    ("fold", &["widget", "widget_layout", "widget_fold"]),
    ("grid", &["widget", "widget_layout", "widget_grid"]),
    ("image", &["widget", "widget_image"]),
    ("label", &["widget", "widget_text"]),
    ("panel", &["widget", "widget_layout"]),
    ("polygon2d", &["polygon"]),
    ("progress_bar", &["widget", "widget_range"]),
    ("rect2d", &["shape2d"]),
    ("rigid_body2d", &["body2d", "collider2d"]),
    ("rigid_body3d", &["body3d", "collider3d"]),
    ("rope2d", &["soft_rope2d", "softbody2d"]),
    ("rope3d", &["soft_rope3d", "softbody3d"]),
    ("row", &["widget", "widget_layout"]),
    ("scroll", &["widget", "widget_scroll", "widget_layout"]),
    ("separator", &["widget", "widget_separator"]),
    ("slider", &["widget", "widget_range"]),
    ("soft_body2d", &["softbody2d"]),
    ("soft_body3d", &["softbody3d"]),
    ("sprite2d", &["sprite"]),
    ("stack", &["widget", "widget_layout"]),
    ("static_body2d", &["body2d", "collider2d"]),
    ("static_body3d", &["body3d", "collider3d"]),
    ("tabs", &["widget", "widget_layout", "widget_tabs"]),
    ("text2d", &["text2d"]),
    ("text3d", &["text3d"]),
    ("text_field", &["widget", "widget_field"]),
    ("tilemap2d", &["tilemap"]),
    ("window", &["widget", "widget_window", "widget_layout"]),
];

/// The editor's recipes, as `presets.toml` writes them: each name, and its
/// components in order with the properties the recipe sets on each.
fn editor_presets() -> Vec<(String, Vec<(String, toml::Table)>)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor/presets.toml");
    let text = std::fs::read_to_string(path).unwrap();
    let file: toml::Table = toml::from_str(&text).unwrap();
    file.into_iter()
        .map(|(name, body)| {
            let parts = body["components"]
                .as_array()
                .unwrap_or_else(|| panic!("`{name}` has no `components` list"))
                .iter()
                .map(|entry| {
                    let mut own = entry.as_table().unwrap().clone();
                    let component = own.remove("component").unwrap();
                    (component.as_str().unwrap().to_string(), own)
                })
                .collect();
            (name, parts)
        })
        .collect()
}

// Every recipe the picker reads names only keys its components read, lands
// whole on a fresh node as `set_component` builds it, and survives the
// frames after it.
#[test]
fn every_editor_preset_builds_whole_and_runs() {
    let (_dir, mut app) = every_plugin();
    let recipes = editor_presets();
    for (name, parts) in &recipes {
        let root = app.engine.root();
        let entity = scene::spawn_node(&mut app.engine.world_mut(), name, root);
        for (component, own) in parts {
            let schema = def(&app, component).schema;
            for key in own.keys() {
                assert!(
                    schema.get(key).is_some(),
                    "`{name}` sets `{key}`, which `{component}` does not read"
                );
            }
            let params = (!own.is_empty()).then(|| toml::Value::Table(own.clone()));
            if let Err(e) = components::add(&app.engine, entity, component, params.as_ref()) {
                panic!("`{name}` did not build `{component}`: {e:#}");
            }
        }
        let present = components::present_on(&app.engine, entity);
        for (component, _) in parts {
            assert!(
                present.contains(component),
                "`{name}` left out `{component}`: {present:?}"
            );
        }
        for _ in 0..3 {
            app.tick(1.0 / 60.0);
        }
    }
    assert!(
        recipes.len() >= REGISTRY.len(),
        "only {} presets, so this proved little",
        recipes.len()
    );
}

// The move to the editor lost no recipe: each one the registry held is in
// the editor's data, giving a node the same components in the same order.
#[test]
fn the_editor_s_presets_give_what_the_registry_gave() {
    let recipes = editor_presets();
    for (name, components) in REGISTRY {
        let Some((_, parts)) = recipes.iter().find(|(found, _)| found == name) else {
            panic!("the editor has no `{name}`");
        };
        let given: Vec<&str> = parts
            .iter()
            .map(|(component, _)| component.as_str())
            .collect();
        assert_eq!(given, *components, "`{name}`");
    }
    // The properties are the point: the two bodies differ only in one.
    let kind = |name: &str| {
        let (_, parts) = recipes.iter().find(|(found, _)| found == name).unwrap();
        parts[0].1["kind"].as_str().unwrap().to_string()
    };
    assert_eq!(kind("rigid_body2d"), "dynamic");
    assert_eq!(kind("static_body2d"), "static");
}

// A component that leaves its node a mesh the solver draws carries `render`,
// or the tree's Rendering filter misses the one thing that draws a rope.
#[test]
fn a_component_that_draws_its_node_is_tagged_render() {
    let (_dir, mut app) = every_plugin();
    let mut drawn = 0;
    for name in components::names(&app.engine) {
        let root = app.engine.root();
        let entity = scene::spawn_node(&mut app.engine.world_mut(), &name, root);
        if components::add(&app.engine, entity, &name, None).is_err() {
            continue;
        }
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let solved = world.get::<&balaur::mesh::SolvedMesh>(entity).is_ok()
            || world.get::<&balaur::mesh::SolvedPolygon>(entity).is_ok();
        drop(world);
        if solved {
            drawn += 1;
            let tags = def(&app, &name).tags;
            assert!(
                tags.contains(&"render"),
                "`{name}` draws its node and is tagged {tags:?}"
            );
        }
    }
    assert!(
        drawn >= 2,
        "only {drawn} components drew their node, so this proved little"
    );
}

/// Every plugin the editor ships, over an empty project.
fn every_plugin() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"presets\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    let mut app = balaur::standard_app(AppConfig::bare(dir.path().to_path_buf())).unwrap();
    app.load_project().unwrap();
    (dir, app)
}
