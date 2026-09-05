//! The `tileset` asset and the `tilemap` component, without a window.

use balaur_core::{components, scene, App, AppConfig};
use balaur_render::{Tilemap, Tileset};

fn app() -> App {
    let mut app = App::new(AppConfig {
        project_root: std::path::PathBuf::from("."),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .expect("App::new builds headless");
    balaur_plugin::load(&mut app, &mut balaur_render::RenderPlugin::default())
        .expect("the render plugin builds headless");
    app
}

fn node(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

/// A tilemap authored inline: the `tileset` property carries a definition
/// table, `cells` a two-row map.
fn tilemap_table() -> toml::Value {
    toml::from_str(
        r#"cells = """
.0
1a"""
pixels_per_unit = 50.0

[tileset]
texture = "tests/fixtures/sprite_200x100.png"
tile_size = 50.0
columns = 4
"#,
    )
    .expect("the tilemap params are valid TOML")
}

#[test]
fn a_tilemap_parses_cells_and_round_trips() {
    let app = app();
    let entity = node(&app);
    components::add(&app.engine, entity, "tilemap", Some(&tilemap_table()))
        .expect("a valid tilemap applies");

    // Control that apply really parsed: row 0 is the text's top line, `.` is
    // empty, digits then letters index the tileset.
    {
        let world = app.engine.world();
        let map = world
            .get::<&Tilemap>(entity)
            .expect("apply writes a Tilemap on the node");
        assert_eq!(
            map.grid,
            vec![vec![None, Some(0)], vec![Some(1), Some(10)]],
            "the cells text did not parse into the expected grid"
        );
    }

    let saved = components::get(&app.engine, entity, "tilemap").expect("the tilemap reads back");
    let table = saved.as_table().expect("get returns a property table");
    assert_eq!(
        table["cells"]
            .as_str()
            .expect("cells reads back as a string"),
        ".0\n1a"
    );
    assert!(
        (table["pixels_per_unit"]
            .as_float()
            .expect("pixels_per_unit reads back as a float")
            - 50.0)
            .abs()
            < 1e-6
    );
    let reference = table["tileset"]
        .as_str()
        .expect("the inline tileset reads back as a reference string");
    assert!(
        reference.starts_with("#!"),
        "an inline definition should have become a reference, got '{reference}'"
    );

    // The reference resolves through the registered parser.
    let tileset = balaur_core::assets::load_typed::<Tileset>(&app.engine, reference)
        .expect("the inline tileset parses");
    assert_eq!(tileset.texture, "tests/fixtures/sprite_200x100.png");
    assert!((tileset.tile_size - 50.0).abs() < 1e-6);
    assert_eq!(tileset.columns, 4);

    // What get returned applies again to the same grid.
    let reloaded = node(&app);
    components::add(&app.engine, reloaded, "tilemap", Some(&saved))
        .expect("a saved tilemap reloads");
    {
        let world = app.engine.world();
        let a = world.get::<&Tilemap>(entity).expect("original still there");
        let b = world
            .get::<&Tilemap>(reloaded)
            .expect("reload writes a Tilemap");
        assert_eq!(a.grid, b.grid);
    }

    // A character outside [.0-9a-z] is refused, by name.
    let bad: toml::Value = toml::from_str("cells = \".X\"").expect("valid TOML");
    let entity = node(&app);
    let err = components::add(&app.engine, entity, "tilemap", Some(&bad))
        .expect_err("an illegal cell character must be rejected");
    assert!(
        format!("{err:#}").contains("'X'"),
        "the error should name the character: {err:#}"
    );
}

#[test]
fn reapplying_the_same_tilemap_does_not_bump_the_version() {
    let app = app();
    let entity = node(&app);
    components::add(&app.engine, entity, "tilemap", Some(&tilemap_table()))
        .expect("a valid tilemap applies");
    let before = app
        .engine
        .world()
        .get::<&Tilemap>(entity)
        .expect("apply writes a Tilemap")
        .version;
    components::add(&app.engine, entity, "tilemap", Some(&tilemap_table()))
        .expect("re-applying the same tilemap succeeds");
    assert_eq!(
        app.engine
            .world()
            .get::<&Tilemap>(entity)
            .expect("still there")
            .version,
        before,
        "identical content must not force a backend rebuild"
    );

    let mut changed = tilemap_table();
    changed
        .as_table_mut()
        .expect("params are a table")
        .insert("cells".into(), toml::Value::String("22".into()));
    components::add(&app.engine, entity, "tilemap", Some(&changed))
        .expect("a changed tilemap applies");
    assert!(
        app.engine
            .world()
            .get::<&Tilemap>(entity)
            .expect("still there")
            .version
            > before,
        "changed cells must bump the version"
    );
}

#[test]
fn a_tileset_that_declares_no_grid_is_refused() {
    let app = app();
    let entity = node(&app);
    // Well-formed component, ill-formed asset: `columns` is missing.
    let params: toml::Value = toml::from_str(
        r#"cells = "0"

[tileset]
texture = "tests/fixtures/sprite_200x100.png"
tile_size = 50.0
"#,
    )
    .expect("the params are valid TOML");
    components::add(&app.engine, entity, "tilemap", Some(&params))
        .expect("a bad tileset warns rather than killing the scene");
    let reference = components::get(&app.engine, entity, "tilemap")
        .expect("the tilemap reads back")
        .get("tileset")
        .and_then(|v| v.as_str().map(str::to_string))
        .expect("the tileset reference reads back");
    let Err(err) = balaur_core::assets::load_typed::<Tileset>(&app.engine, &reference) else {
        panic!("a tileset without `columns` must not parse");
    };
    assert!(
        format!("{err:#}").contains("columns"),
        "the error should name the missing field: {err:#}"
    );
}

#[test]
fn cells_as_rows_of_ids_reach_past_the_thirty_sixth_tile() {
    let app = app();
    let entity = node(&app);
    let mut table = tilemap_table();
    table.as_table_mut().unwrap().insert(
        "cells".into(),
        toml::from_str::<toml::Value>("v = [[40, -1], [0, 99]]").unwrap()["v"].clone(),
    );
    components::add(&app.engine, entity, "tilemap", Some(&table)).unwrap();
    let world = app.engine.world();
    let map = world.get::<&Tilemap>(entity).unwrap();
    assert_eq!(
        map.grid,
        vec![vec![Some(40), None], vec![Some(0), Some(99)]]
    );
}

#[test]
fn a_material_on_the_map_is_kept_and_bumps_the_version_when_it_changes() {
    let app = app();
    let entity = node(&app);
    components::add(&app.engine, entity, "tilemap", Some(&tilemap_table())).unwrap();
    let before = app.engine.world().get::<&Tilemap>(entity).unwrap().version;
    let mut table = tilemap_table();
    table.as_table_mut().unwrap().insert(
        "material".into(),
        toml::Value::String("materials/water.toml".into()),
    );
    components::add(&app.engine, entity, "tilemap", Some(&table)).unwrap();
    let world = app.engine.world();
    let map = world.get::<&Tilemap>(entity).unwrap();
    assert_eq!(map.material, "materials/water.toml");
    assert_eq!(map.version, before + 1);
}
