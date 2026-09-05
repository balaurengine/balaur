//! The `material` asset end to end, without a window.
//!
//! Linking a shader needs no GPU, so everything here — the asset resolving,
//! the shader file being found, the `Params` struct being read off the linked
//! output — runs wherever CI does. What a GPU would add is the pipeline.

use balaur_core::{App, AppConfig, components, scene};
use balaur_render::material::{Material, compile};
use balaur_render::{RenderPlugin, Renderable2d};

const SHADER: &str = r"
import package::sprite::{VertexInput, VertexOutput, vertex, sample_albedo, tint, time};

struct Params { speed: f32, glow: vec4<f32> }
@group(3) @binding(0) var<uniform> params: Params;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let pulse = 0.5 + 0.5 * sin(time() * params.speed);
    return sample_albedo(in.uv) * tint(in) + params.glow * pulse;
}
";

const MATERIAL: &str = r##"type = "material"
shader = "shaders/wave.wesl"
params = { speed = 3.0, glow = "#204080" }
"##;

const SHADER_3D: &str = r"
import package::mesh::{VertexInput, VertexOutput, vertex, shade};

struct Params { warmth: vec4<f32> }
@group(3) @binding(0) var<uniform> params: Params;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return shade(in) * params.warmth;
}
";

const MATERIAL_3D: &str = r##"type = "material"
shader = "shaders/lit.wesl"
params = { warmth = "#ffddaa" }
"##;

/// A project on disk with one shader and one material naming it.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("shaders")).unwrap();
    std::fs::create_dir_all(dir.path().join("materials")).unwrap();
    std::fs::write(dir.path().join("shaders/wave.wesl"), SHADER).unwrap();
    std::fs::write(dir.path().join("materials/wave.toml"), MATERIAL).unwrap();
    std::fs::write(dir.path().join("shaders/lit.wesl"), SHADER_3D).unwrap();
    std::fs::write(dir.path().join("materials/lit.toml"), MATERIAL_3D).unwrap();
    // A sprite sizes itself from its image, so the project needs a real one.
    std::fs::create_dir_all(dir.path().join("art")).unwrap();
    std::fs::copy(
        "tests/fixtures/sprite_200x100.png",
        dir.path().join("art/sprite.png"),
    )
    .unwrap();
    dir
}

fn app(root: &std::path::Path) -> App {
    let mut app = App::new(AppConfig::bare(root.to_path_buf())).unwrap();
    balaur_plugin::load(&mut app, &mut RenderPlugin::default()).unwrap();
    app
}

fn node(app: &App) -> balaur_core::hecs::Entity {
    let root = app.engine.root();
    scene::spawn_node(&mut app.engine.world_mut(), "N", root)
}

#[test]
fn a_sprite_remembers_the_material_it_names() {
    let dir = project();
    let app = app(dir.path());
    let entity = node(&app);
    let table =
        toml::from_str("texture = \"art/sprite.png\"\nmaterial = \"materials/wave.toml\"").unwrap();
    components::add(&app.engine, entity, "sprite", Some(&table)).unwrap();

    let world = app.engine.world();
    let renderable = world.get::<&Renderable2d>(entity).unwrap();
    assert_eq!(renderable.material, "materials/wave.toml");
}

#[test]
fn a_sprite_with_no_material_names_none() {
    let dir = project();
    let app = app(dir.path());
    let entity = node(&app);
    let table = toml::from_str("texture = \"art/sprite.png\"").unwrap();
    components::add(&app.engine, entity, "sprite", Some(&table)).unwrap();

    let world = app.engine.world();
    let renderable = world.get::<&Renderable2d>(entity).unwrap();
    assert!(renderable.material.is_empty());
}

#[test]
fn the_material_asset_loads_and_its_shader_links() {
    let dir = project();
    let app = app(dir.path());
    let asset =
        balaur_core::assets::load_typed::<Material>(&app.engine, "materials/wave.toml").unwrap();
    assert_eq!(asset.shader, "shaders/wave.wesl");

    let source = balaur_core::project::scene_text(&app.engine, &asset.shader).unwrap();
    let compiled = compile(&asset, &source).unwrap();
    assert!(compiled.wgsl.contains("fn vs_main"), "{}", compiled.wgsl);
    assert!(compiled.wgsl.contains("fn fs_main"), "{}", compiled.wgsl);
    // speed at 0, glow padded to 16: the shader's own struct decides, and the
    // material never says.
    assert_eq!(compiled.params.len(), 32);
    assert_eq!(
        compiled.fields.iter().map(|f| f.offset).collect::<Vec<_>>(),
        vec![0, 16]
    );
}

#[test]
fn a_material_naming_a_missing_shader_says_which_file() {
    let dir = project();
    std::fs::write(
        dir.path().join("materials/gone.toml"),
        "type = \"material\"\nshader = \"shaders/gone.wesl\"\n",
    )
    .unwrap();
    let app = app(dir.path());
    let asset =
        balaur_core::assets::load_typed::<Material>(&app.engine, "materials/gone.toml").unwrap();
    let err = balaur_core::project::scene_text(&app.engine, &asset.shader).unwrap_err();
    assert!(format!("{err:#}").contains("shaders/gone.wesl"), "{err:#}");
}

#[test]
fn invalidating_moves_the_generation_a_linked_material_watches() {
    let dir = project();
    let app = app(dir.path());
    let before = balaur_core::assets::generation(&app.engine);
    balaur_core::assets::invalidate(&app.engine);
    assert_ne!(balaur_core::assets::generation(&app.engine), before);
}

#[test]
fn rewriting_a_shader_is_what_the_next_link_reads() {
    let dir = project();
    let app = app(dir.path());
    let first = balaur_core::project::scene_text(&app.engine, "shaders/wave.wesl").unwrap();
    std::fs::write(
        dir.path().join("shaders/wave.wesl"),
        format!("{first}\n// edited"),
    )
    .unwrap();
    let second = balaur_core::project::scene_text(&app.engine, "shaders/wave.wesl").unwrap();
    assert!(second.ends_with("// edited"), "{second}");
}

#[test]
fn a_shape3d_remembers_the_material_it_names() {
    let dir = project();
    let app = app(dir.path());
    let entity = node(&app);
    let table = toml::from_str("kind = \"ball\"\nmaterial = \"materials/lit.toml\"").unwrap();
    components::add(&app.engine, entity, "shape3d", Some(&table)).unwrap();

    let world = app.engine.world();
    let renderable = world
        .get::<&balaur_render::Renderable>(entity)
        .expect("a shape3d writes a Renderable");
    assert_eq!(renderable.material, "materials/lit.toml");
}

#[test]
fn the_component_writes_the_material_back() {
    let dir = project();
    let app = app(dir.path());
    let entity = node(&app);
    let table = toml::from_str("kind = \"ball\"\nmaterial = \"materials/lit.toml\"").unwrap();
    components::add(&app.engine, entity, "shape3d", Some(&table)).unwrap();

    let read = components::get(&app.engine, entity, "shape3d").unwrap();
    assert_eq!(
        read.get("material").and_then(toml::Value::as_str),
        Some("materials/lit.toml")
    );
}

#[test]
fn a_3d_material_links_against_the_mesh_contract() {
    let dir = project();
    let app = app(dir.path());
    let asset =
        balaur_core::assets::load_typed::<Material>(&app.engine, "materials/lit.toml").unwrap();
    let source = balaur_core::project::scene_text(&app.engine, &asset.shader).unwrap();
    let compiled = compile(&asset, &source).unwrap();
    assert!(compiled.wgsl.contains("fn fs_main"), "{}", compiled.wgsl);
    // `shade` pulled the lighting loop and the fog in with it.
    assert!(compiled.wgsl.contains("ambient_count"), "{}", compiled.wgsl);
    assert!(compiled.wgsl.contains("fog_color"), "{}", compiled.wgsl);
    assert_eq!(compiled.params.len(), 16);
}
