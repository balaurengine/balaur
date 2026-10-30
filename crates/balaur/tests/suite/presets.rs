//! Tags, presets and expectation warnings.
//!
//! These three exist so a flat list of components is navigable: what a
//! component is *about* (tags), a shortcut for the combinations people
//! actually want (presets), and a nudge when a combination is incomplete
//! (expectations). None of them adds a type -- a node is still exactly the
//! components on it.

use balaur::{App, AppConfig, components, presets, scene};

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
        events: &[],
        warnings: None,
        doc: "",
        schema: d.schema.clone(),
        tags: d.tags,
        expects: d.expects,
        apply: Box::new(|_, _, _| Ok(())),
        remove: Box::new(|_, _| Ok(())),
        get: Box::new(|_, _| None),
    }
}

/// A tag is the thing a single category path cannot express: `collider2d` is
/// genuinely both, and forcing a choice buries whichever loses.
#[test]
fn a_component_can_carry_several_tags() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin::default()).unwrap();
    let tags = def(&app, "collider2d").tags;
    assert!(tags.contains(&"2d"), "tags were {tags:?}");
    assert!(tags.contains(&"physics"), "tags were {tags:?}");
}

/// 3D carries no dimension marker (docs/NAMING.md D5), so the 3D body is
/// tagged `3d` while its name stays unmarked.
#[test]
fn dimension_tags_separate_the_two_worlds() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin::default()).unwrap();
    assert!(def(&app, "body3d").tags.contains(&"3d"));
    assert!(def(&app, "body2d").tags.contains(&"2d"));
    assert!(!def(&app, "body3d").tags.contains(&"2d"));
}

/// Every component has to be findable under some facet, or the palette hides
/// it. This is the check that stops a new component shipping untagged.
#[test]
fn every_registered_component_is_tagged() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin::default()).unwrap();
    let registry = app.engine.resource::<components::ComponentRegistry>();
    let registry = registry.borrow();
    for (name, def) in &*registry {
        assert!(!def.tags.is_empty(), "`{name}` has no tags");
    }
}

#[test]
fn a_preset_applies_every_component_it_names() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin::default()).unwrap();
    let entity = node(&app);
    presets::apply(&app.engine, entity, "rigid_body2d").unwrap();
    let present = components::present_on(&app.engine, entity);
    assert!(present.iter().any(|c| c == "body2d"), "{present:?}");
    assert!(present.iter().any(|c| c == "collider2d"), "{present:?}");
}

/// The parameters are the point: `static_body2d` and `rigid_body2d` add the
/// same two components and differ only in what the body is set to.
#[test]
fn presets_carry_the_parameters_that_distinguish_them() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin::default()).unwrap();
    let dynamic = node(&app);
    let stationary = node(&app);
    presets::apply(&app.engine, dynamic, "rigid_body2d").unwrap();
    presets::apply(&app.engine, stationary, "static_body2d").unwrap();
    let kind = |e| {
        components::get(&app.engine, e, "body2d").unwrap()["kind"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(kind(dynamic), "dynamic");
    assert_eq!(kind(stationary), "static");
}

/// A preset is a recipe, not a type: nothing records which one was used, so
/// removing a component afterwards is ordinary and leaves no broken claim.
#[test]
fn a_node_does_not_remember_its_preset() {
    let mut app = app();
    balaur_plugin::load(&mut app, &mut balaur::physics::PhysicsPlugin::default()).unwrap();
    let entity = node(&app);
    presets::apply(&app.engine, entity, "rigid_body2d").unwrap();
    components::remove(&app.engine, entity, "collider2d").unwrap();
    let present = components::present_on(&app.engine, entity);
    assert!(!present.iter().any(|c| c == "collider2d"));
    assert!(present.iter().any(|c| c == "body2d"), "the rest survives");
}

#[test]
fn an_unknown_preset_is_an_error() {
    let app = app();
    let entity = node(&app);
    let err = presets::apply(&app.engine, entity, "nope").unwrap_err();
    assert!(err.to_string().contains("nope"), "{err}");
}

/// The editor is a Balaur project, so it reaches all of this through the
/// script API rather than through Rust. This is that surface.
#[test]
fn the_script_api_exposes_tags_presets_and_warnings() {
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
                self.check(scene::component_tags("nope").is_none(), "unknown component has no tags");

                let names = scene::presets();
                self.check(has(names, "rigid_body2d"), "rigid_body2d missing");
                self.check(has(names, "static_body2d"), "static_body2d missing");
                // Both dimensions are marked (D5): there is no bare `rigid_body`.
                self.check(has(names, "rigid_body3d"), "rigid_body3d missing");
                self.check(has(names, "static_body3d"), "static_body3d missing");
                self.check(!has(names, "rigid_body"), "the unmarked 3D name is gone");
                self.check(has(names, "sprite2d"), "sprite2d missing");

                let found = scene::preset_info("rigid_body2d");
                self.check(found.is_some(), "no info for rigid_body2d");
                if found.is_some() {
                    let info = found.unwrap();
                    self.check(info.components.len() == 2, "rigid_body2d adds two components");
                    self.check(info.description != "", "a preset needs a description");
                    // Each part's own properties, which is what makes the preset mean
                    // something: a rigid body is a dynamic one.
                    self.check(info.parts.get("body2d").get("kind").as_str() == "dynamic", "the body part carries its kind");
                    self.check(info.parts.get("collider2d").len() == 0, "a part with no properties is an empty table");
                }

                // Applying one puts the components on the node, and nothing records
                // that a preset was used.
                let n = scene::root().add_child("Thing");
                scene::apply_preset(n, "rigid_body2d");
                let present = n.component_names();
                self.check(has(present, "body2d"), "body2d not applied");
                self.check(has(present, "collider2d"), "collider2d not applied");
                self.check(n.get_component("body2d", "kind").as_str() == "dynamic", "wrong body kind");

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

/// Every preset the picker lists names only keys its components read, lands
/// whole on a fresh node and survives the frames after it.
#[test]
fn every_preset_applies_whole_and_runs() {
    let (_dir, mut app) = every_plugin();
    let names = presets::names(&app.engine);
    for name in &names {
        let root = app.engine.root();
        let entity = scene::spawn_node(&mut app.engine.world_mut(), name, root);
        if let Err(e) = presets::apply(&app.engine, entity, name) {
            panic!("`{name}` did not apply: {e:#}");
        }
        let info = {
            let registry = app.engine.resource::<presets::PresetRegistry>();
            let registry = registry.borrow();
            registry.0[name].parts.clone()
        };
        let present = components::present_on(&app.engine, entity);
        for part in &info {
            assert!(
                present.contains(&part.component),
                "`{name}` left out `{}`: {present:?}",
                part.component
            );
            let schema = def(&app, &part.component).schema;
            let keys = part
                .params
                .as_ref()
                .and_then(toml::Value::as_table)
                .into_iter()
                .flat_map(|t| t.keys());
            for key in keys {
                assert!(
                    schema.get(key).is_some(),
                    "`{name}` sets `{key}`, which `{}` does not read",
                    part.component
                );
            }
        }
        for _ in 0..3 {
            app.tick(1.0 / 60.0);
        }
    }
    assert!(
        names.len() >= 10,
        "only {} presets, so this proved little",
        names.len()
    );
}

/// A component that leaves its node a mesh the solver draws carries `render`,
/// or the tree's Rendering filter misses the one thing that draws a rope.
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
