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
    for (name, def) in &registry.0 {
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
        "[application]\nname = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    std::fs::write(
        dir.path().join("scripts/t.rn"),
        r#"
        fn has(list, item) {
            for x in list {
                if x == item {
                    return true;
                }
            }
            false
        }

        pub fn init(this) {
            let tags = scene::component_tags("collider2d");
            assert!(has(tags, "2d"), "collider2d should be tagged 2d");
            assert!(has(tags, "physics"), "collider2d should be tagged physics");
            assert!(scene::component_tags("nope") is Tuple, "unknown component has no tags");

            let names = scene::presets();
            assert!(has(names, "rigid_body2d"), "rigid_body2d missing");
            assert!(has(names, "static_body2d"), "static_body2d missing");
            // Both dimensions are marked (D5): there is no bare `rigid_body`.
            assert!(has(names, "rigid_body3d"), "rigid_body3d missing");
            assert!(has(names, "static_body3d"), "static_body3d missing");
            assert!(!has(names, "rigid_body"), "the unmarked 3D name is gone");
            assert!(has(names, "sprite2d"), "sprite2d missing");

            let info = scene::preset_info("rigid_body2d");
            assert!(!(info is Tuple), "no info for rigid_body2d");
            assert!(info.components.len() == 2, "rigid_body2d adds two components");
            assert!(info.description != "", "a preset needs a description");

            // Applying one puts the components on the node, and nothing records
            // that a preset was used.
            let n = scene::spawn("Thing");
            scene::apply_preset(n, "rigid_body2d");
            let present = n.component_names();
            assert!(has(present, "body2d"), "body2d not applied");
            assert!(has(present, "collider2d"), "collider2d not applied");
            assert!(n.get_component("body2d").kind == "dynamic", "wrong body kind");

            assert!(scene::unmet_expectations(n).len() == 0, "nothing should warn here");
            this.done = 1;
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
        .attach(balaur::node_id_of(entity), "scripts/t.rn")
        .unwrap();
    assert_eq!(
        balaur::rune::rune_of(&app.engine).number_field(entity, "done"),
        Some(1.0),
        "the script did not run to its end: {:#?}",
        balaur::logbuf::recent(10)
    );
}
