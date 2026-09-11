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

/// A material that never mentions instancing still gets the per-copy inputs,
/// which is what lets a cloner multiply a node the shader knows nothing
/// about. The locations have to match `vertex_layouts` in
/// `shader_material_3d.rs`, and this is what says so without a GPU.
#[test]
fn a_3d_material_carries_the_per_copy_inputs_it_never_asked_for() {
    let dir = project();
    let app = app(dir.path());
    let asset =
        balaur_core::assets::load_typed::<Material>(&app.engine, "materials/lit.toml").unwrap();
    let source = balaur_core::project::scene_text(&app.engine, &asset.shader).unwrap();
    let compiled = compile(&asset, &source).unwrap();
    for location in 3..=7 {
        assert!(
            compiled.wgsl.contains(&format!("@location({location})")),
            "location {location} is missing from the vertex contract: {}",
            compiled.wgsl
        );
    }
    assert!(
        compiled.wgsl.contains("instance_index"),
        "a material should be able to ask which copy it is: {}",
        compiled.wgsl
    );
}

/// A material may ask which copy it is drawing, and tint it by that alone.
#[test]
fn a_material_can_read_the_copy_it_is_drawing() {
    let dir = project();
    std::fs::write(
        dir.path().join("shaders/striped.wesl"),
        r"
import package::mesh::{VertexInput, VertexOutput, vertex, copy_index, copy_tint, shade};

struct Stripe { color: vec4<f32> }
@group(3) @binding(0) var<uniform> stripe: Stripe;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    let which: u32 = copy_index(in);
    let tint: vec4<f32> = copy_tint(in);
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return shade(in) * stripe.color;
}
",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("materials/striped.toml"),
        "type = \"material\"\nshader = \"shaders/striped.wesl\"\nparams = { color = \"#ffffff\" }\n",
    )
    .unwrap();
    let app = app(dir.path());
    let asset =
        balaur_core::assets::load_typed::<Material>(&app.engine, "materials/striped.toml").unwrap();
    let source = balaur_core::project::scene_text(&app.engine, &asset.shader).unwrap();
    let compiled = compile(&asset, &source).expect("a shader that reads its copy links");
    assert!(compiled.wgsl.contains("fn vs_main"), "{}", compiled.wgsl);
}

/// A material asks for vertex colours by name, and only then does its
/// pipeline carry the attribute. A material that does not ask is unchanged,
/// which is what keeps the buffer off every other mesh.
#[test]
fn vertex_colours_arrive_only_for_a_material_that_asks() {
    let dir = project();
    let shader = r"
import package::mesh::{VertexInput, VertexOutput, vertex, vertex_color, shade};

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput = vertex(in);
    out.tint = vertex_color(in);
    return out;
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return shade(in);
}
";
    std::fs::write(dir.path().join("shaders/painted.wesl"), shader).unwrap();
    std::fs::write(
        dir.path().join("materials/painted.toml"),
        "type = \"material\"\nshader = \"shaders/painted.wesl\"\nfeatures = { vertex_color = true }\n",
    )
    .unwrap();
    let app = app(dir.path());
    let asset =
        balaur_core::assets::load_typed::<Material>(&app.engine, "materials/painted.toml").unwrap();
    assert!(
        asset.reads_vertex_color(),
        "the feature is read off the asset"
    );
    let source = balaur_core::project::scene_text(&app.engine, &asset.shader).unwrap();
    let compiled = compile(&asset, &source).expect("a painted material links");
    assert!(compiled.vertex_color, "and reaches the compiled material");
    assert!(
        compiled.wgsl.contains("@location(8)"),
        "the per-vertex colour attribute is there: {}",
        compiled.wgsl
    );

    // The lit material never mentions it, so its pipeline has no such slot.
    let plain =
        balaur_core::assets::load_typed::<Material>(&app.engine, "materials/lit.toml").unwrap();
    let plain_source = balaur_core::project::scene_text(&app.engine, &plain.shader).unwrap();
    let plain = compile(&plain, &plain_source).unwrap();
    assert!(!plain.vertex_color);
    assert!(
        !plain.wgsl.contains("@location(8)"),
        "a material that did not ask should not carry the attribute: {}",
        plain.wgsl
    );
}

/// The 2D builtins a ported canvas_item shader reaches for, linked in one
/// unit: the screen behind the object, the frame clock, the vertex the shader
/// displaces, and a texel of each of the two textures.
///
/// One test rather than five: what is being checked is that the contract
/// `sprite.wesl` publishes covers them, and a link either resolves every name
/// or fails naming the one it could not.
const SHADER_BUILTINS: &str = r"
import package::sprite::{
    VertexInput, VertexOutput, place, tint, time,
    sample_albedo, sample_screen, screen_uv, texture_pixel_size, screen_pixel_size,
};

struct Params { amplitude: f32 }
@group(3) @binding(0) var<uniform> params: Params;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    let sway = vec2<f32>(sin(time()) * params.amplitude, 0.0);
    return place(in, sway);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let step = texture_pixel_size();
    let neighbour = sample_albedo(in.uv + step);
    let behind = sample_screen(screen_uv(in.clip_position) + screen_pixel_size());
    return (sample_albedo(in.uv) + neighbour + behind) * tint(in);
}
";

#[test]
fn the_2d_contract_covers_the_builtins_a_canvas_shader_uses() {
    let dir = project();
    std::fs::write(dir.path().join("shaders/builtins.wesl"), SHADER_BUILTINS).unwrap();
    std::fs::write(
        dir.path().join("materials/builtins.toml"),
        "type = \"material\"\nshader = \"shaders/builtins.wesl\"\nfeatures = { screen = true }\nparams = { amplitude = 0.25 }\n",
    )
    .unwrap();
    let app = app(dir.path());

    let asset = balaur_core::assets::load_typed::<Material>(&app.engine, "materials/builtins.toml")
        .unwrap();
    let source = balaur_core::project::scene_text(&app.engine, &asset.shader).unwrap();
    let compiled = compile(&asset, &source).unwrap();

    assert!(compiled.wgsl.contains("fn vs_main"), "{}", compiled.wgsl);
    assert!(
        compiled.wgsl.contains("screen_texture"),
        "`features = {{ screen = true }}` should bind the frame so far: {}",
        compiled.wgsl
    );
}

fn child_of(app: &App, parent: balaur_core::hecs::Entity) -> balaur_core::hecs::Entity {
    scene::spawn_node(&mut app.engine.world_mut(), "Child", parent)
}

fn inherited(app: &App, entity: balaur_core::hecs::Entity) -> String {
    scene::propagate_transforms(&mut app.engine.world_mut(), app.engine.root());
    let world = app.engine.world();
    let global = world.get::<&scene::GlobalAppearance>(entity).unwrap();
    global.material.reference().to_string()
}

#[test]
fn the_material_component_goes_on_a_node_that_draws_nothing() {
    let dir = project();
    let app = app(dir.path());
    let parent = node(&app);
    let child = child_of(&app, parent);
    let table = toml::from_str("source = \"materials/wave.toml\"").unwrap();
    components::add(&app.engine, parent, "material", Some(&table)).unwrap();

    assert_eq!(inherited(&app, child), "materials/wave.toml");
    let got = components::get(&app.engine, parent, "material").unwrap();
    assert_eq!(got["source"].as_str(), Some("materials/wave.toml"));
}

/// The `color` and `tint` split, for materials: a renderable's own names
/// what it draws, and only the component reaches the nodes under it.
#[test]
fn a_sprites_own_material_is_its_alone() {
    let dir = project();
    let app = app(dir.path());
    let parent = node(&app);
    let child = child_of(&app, parent);
    let table =
        toml::from_str("texture = \"art/sprite.png\"\nmaterial = \"materials/wave.toml\"").unwrap();
    components::add(&app.engine, parent, "sprite", Some(&table)).unwrap();

    assert_eq!(inherited(&app, child), "");
    assert!(components::get(&app.engine, parent, "material").is_none());
}

#[test]
fn removing_the_material_component_clears_what_it_named() {
    let dir = project();
    let app = app(dir.path());
    let parent = node(&app);
    let child = child_of(&app, parent);
    let table = toml::from_str("source = \"materials/wave.toml\"").unwrap();
    components::add(&app.engine, parent, "material", Some(&table)).unwrap();
    components::remove(&app.engine, parent, "material").unwrap();

    assert_eq!(inherited(&app, child), "");
    assert!(components::get(&app.engine, parent, "material").is_none());
}

/// Added from the picker with nothing chosen yet, the component stays on
/// the node rather than vanishing because it names no material.
#[test]
fn an_empty_material_component_stays_on_the_node() {
    let dir = project();
    let app = app(dir.path());
    let entity = node(&app);
    components::add(&app.engine, entity, "material", None).unwrap();
    let got = components::get(&app.engine, entity, "material").unwrap();
    assert_eq!(got["source"].as_str(), Some(""));
}

/// `node.set_material` writes the same field, so the inspector shows it.
#[test]
fn a_material_a_script_set_reads_back_as_the_component() {
    let dir = project();
    let app = app(dir.path());
    let entity = node(&app);
    app.engine
        .world()
        .get::<&mut scene::Appearance>(entity)
        .unwrap()
        .material = scene::MaterialId::intern("materials/lit.toml");
    let got = components::get(&app.engine, entity, "material").unwrap();
    assert_eq!(got["source"].as_str(), Some("materials/lit.toml"));
}

#[test]
fn the_contract_a_shader_imports_says_which_nodes_it_draws() {
    use balaur_render::shaders::{Contract, contract};
    assert_eq!(contract(SHADER, &[]), Some(Contract::Sprite));
    assert_eq!(contract(SHADER_3D, &[]), Some(Contract::Mesh));
    let pbr = "import package::common::unpack_mat3;\nimport package::pbr::{shade_pbr};";
    assert_eq!(contract(pbr, &[]), Some(Contract::Mesh));
    assert_eq!(
        contract("import package::post::{frame};", &[]),
        Some(Contract::Post)
    );
    assert_eq!(contract("fn main() {}", &[]), None);
    // A plugin's module decides by what it imports in turn.
    let water = (
        "package::water".to_string(),
        "import package::sprite::{vertex};".to_string(),
    );
    assert_eq!(
        contract("import package::water::{ripple};", &[water]),
        Some(Contract::Sprite)
    );
    let looped = (
        "package::me".to_string(),
        "import package::me::{x};".to_string(),
    );
    assert_eq!(contract("import package::me::{x};", &[looped]), None);
}

/// The editor's case: its engine is rooted elsewhere and names the game's
/// material by absolute path. The material still resolves its files against
/// the game, and an inspector edit saves to the game's own file.
#[test]
fn a_material_named_by_absolute_path_belongs_to_its_own_project() {
    let game = project();
    std::fs::write(
        game.path().join("project.toml"),
        "[application]\nname = \"g\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    let editor = tempfile::tempdir().unwrap();
    let app = app(editor.path());
    let reference = game.path().join("materials/wave.toml");
    let reference = reference.to_string_lossy();

    let texture = balaur_render::material::project_path(&app.engine, &reference, "art/sprite.png");
    assert_eq!(
        texture.as_deref(),
        Some(
            game.path()
                .join("art/sprite.png")
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        balaur_render::material::project_path(&app.engine, "materials/wave.toml", "art/x.png"),
        None,
        "a relative material resolves against the engine as it always did"
    );

    let mut definition = balaur_core::assets::definition(&app.engine, &reference).unwrap();
    definition["params"]["speed"] = toml::Value::Float(9.0);
    balaur_core::assets::save(&app.engine, &reference, &definition).unwrap();
    let written = std::fs::read_to_string(game.path().join("materials/wave.toml")).unwrap();
    assert!(written.contains("speed = 9.0"), "{written}");
    let reread = balaur_core::assets::load_typed::<Material>(&app.engine, &reference).unwrap();
    let speed = reread.params.iter().find(|(name, _)| name == "speed");
    assert_eq!(
        speed.map(|(_, value)| value.clone()),
        Some(balaur_render::material::Param::Float(9.0)),
        "the next load reads what was saved"
    );
}

/// A 2D material binds images of its own beside the node's, and its shader
/// reads them through `package::sprite`.
#[test]
fn a_sprite_material_binds_and_samples_images_of_its_own() {
    let body: toml::Value =
        toml::from_str("shader = \"shaders/dissolve.wesl\"\n[params]\ntexture_2 = \"art/noise.png\"")
            .unwrap();
    let material = balaur_render::material::parse(&body).unwrap();
    assert_eq!(
        material.sprite_textures(),
        vec![None, Some("art/noise.png"), None, None]
    );
    let shader = r"
import package::sprite::{VertexInput, VertexOutput, vertex, sample_albedo, texture_2, sampler_2};

@vertex fn vs_main(in: VertexInput) -> VertexOutput { return vertex(in); }

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let noise = textureSample(texture_2, sampler_2, in.uv).r;
    return sample_albedo(in.uv) * step(0.5, noise);
}
";
    let compiled = balaur_render::material::compile(&material, shader).expect("a slot links");
    assert!(compiled.wgsl.contains("texture_2"), "{}", compiled.wgsl);
}
