//! Balaur's shaders, and the WESL linker every shader goes through.
//!
//! Shaders are written in WESL — WGSL plus imports, `@if` variants and dead
//! code elimination — and linked to plain WGSL before a backend compiles
//! them. Linking happens at run time rather than in `build.rs` so that the
//! engine's own shaders and a project's take one path, and here rather than
//! behind the `kiss3d` feature because linking needs no GPU: a shader that
//! does not link is a bug a headless test can catch.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_plugin::Registry;

/// Helpers any shader may `import package::common::…`.
static COMMON: &str = include_str!("shaders/common.wesl");

/// The 2D skinning material's shader.
pub static SKINNED_2D: &str = include_str!("shaders/skinned_2d.wesl");

/// The 3D skinning material's shader.
pub static SKINNED_3D: &str = include_str!("shaders/skinned_3d.wesl");

/// The 2D light map's shader: the lights, the shadow polygons that mask
/// them, and the full-screen draw that multiplies the frame by the result.
pub static LIGHT_2D: &str = include_str!("shaders/light2d.wesl");

/// The contract a project's 2D material shader draws against, mounted as
/// `package::sprite`: the uniforms the pipeline binds and the vertex work
/// every such shader would otherwise repeat.
pub(crate) static SPRITE: &str = include_str!("shaders/sprite.wesl");

/// The 3D counterpart, mounted as `package::mesh`: the same uniforms in three
/// dimensions, plus the scene's lights and fog.
pub(crate) static MESH: &str = include_str!("shaders/mesh.wesl");

/// The physically based surface, mounted as `package::pbr`: a material
/// importing it shades with GGX over the same lights `package::mesh` collects.
pub(crate) static PBR: &str = include_str!("shaders/pbr.wesl");

/// The geometry pass every 3D material draws before its colour one. The
/// engine's own, not a project's: the pass wants the shape, not the paint.
pub static PREPASS: &str = include_str!("shaders/prepass.wesl");

/// The prepass as WGSL, for a material whose vertices do or do not carry a
/// colour — the one thing that changes its vertex attributes.
pub fn link_prepass(vertex_color: bool) -> Result<String> {
    let linked = link(
        &[("package::prepass", PREPASS)],
        "package::prepass",
        &[(crate::material::VERTEX_COLOR, vertex_color)],
    )?;
    wgsl(&linked)
}

/// What a channel view draws: one entry point per channel, chosen by feature.
/// What a `camera.post` material imports: the frame, and the triangle that
/// covers the screen with it.
pub static POST: &str = include_str!("shaders/post.wesl");

/// The finishing passes the engine ships as `camera.post` materials:
/// vignette, chromatic aberration, grain and pixelation, one variant each.
pub static FINISH: &str = include_str!("shaders/finish.wesl");

pub static CHANNEL: &str = include_str!("shaders/channel.wesl");

/// The 2D counterpart of [`CHANNEL`].
pub static CHANNEL_2D: &str = include_str!("shaders/channel2d.wesl");

/// The channels [`CHANNEL`] can draw, in the order a menu lists them.
pub const CHANNELS: &[&str] = &["albedo", "normals", "uv", "depth"];

/// The most lights one frame sends a 3D material.
pub(crate) const MAX_LIGHTS: usize = 16;

/// The most reflection probes one frame sends, the fork's own cap.
pub(crate) const MAX_PROBES: usize = 8;

/// The most bones one skinned mesh or polygon may name. 128 `mat4` is 8 KB,
/// which keeps a palette inside the 16 KB uniform every adapter guarantees.
pub(crate) const MAX_JOINTS: usize = 128;

/// The limits above as WESL's `constants` module, so a shader sizes its arrays
/// with `import constants::MAX_LIGHTS` from the same number the buffer uses.
fn constants_module() -> String {
    use std::fmt::Write as _;
    let limits = [
        ("MAX_LIGHTS", MAX_LIGHTS),
        ("MAX_PROBES", MAX_PROBES),
        ("MAX_JOINTS", MAX_JOINTS),
    ];
    let mut module = String::new();
    for (name, value) in limits {
        let _ = writeln!(module, "public const {name}: u32 = {value}u;");
    }
    module
}

/// Shader modules a plugin added, mounted beside the engine's own.
///
/// Ordered, not hashed: a link is over the same modules in the same order
/// every run, whoever registered them.
#[derive(Default)]
pub struct ShaderModules(pub Vec<(String, String)>);

/// The contract modules a material's shader imports; which one says what
/// it draws. See [`contract`].
pub(crate) const SPRITE_MODULE: &str = "package::sprite";
pub(crate) const MESH_MODULE: &str = "package::mesh";
pub(crate) const PBR_MODULE: &str = "package::pbr";
pub(crate) const POST_MODULE: &str = "package::post";

/// Which pipeline a shader was written for, read off the contract it imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contract {
    /// `package::sprite`: a 2D node.
    Sprite,
    /// `package::mesh` or `package::pbr`: a 3D node.
    Mesh,
    /// `package::post`: a pass over the frame, never a node.
    Post,
}

impl std::fmt::Display for Contract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Contract::Sprite => "2D",
            Contract::Mesh => "3D",
            Contract::Post => "a post-process pass",
        })
    }
}

/// The contract `source` imports, following a plugin's module into its own
/// imports. `None` names none, and links as it would have.
#[must_use]
pub fn contract(source: &str, modules: &[(String, String)]) -> Option<Contract> {
    contract_within(source, modules, 0)
}

fn contract_within(source: &str, modules: &[(String, String)], depth: u32) -> Option<Contract> {
    // Deep enough for any honest chain, and a stop for one that imports itself.
    if depth > 8 {
        return None;
    }
    for line in source.lines() {
        let Some(path) = line.trim_start().strip_prefix("import ") else {
            continue;
        };
        let module = path
            .split("::")
            .take(2)
            .map(|segment| segment.trim().trim_end_matches(';'))
            .collect::<Vec<_>>()
            .join("::");
        let found = match module.as_str() {
            SPRITE_MODULE => Some(Contract::Sprite),
            MESH_MODULE | PBR_MODULE => Some(Contract::Mesh),
            POST_MODULE => Some(Contract::Post),
            _ => modules
                .iter()
                .find(|(name, _)| *name == module)
                .and_then(|(_, text)| contract_within(text, modules, depth + 1)),
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Whether a material written against `found` draws on a node of `wanted`,
/// warning when not. An inherited material reaches nodes nobody named it on,
/// so a mismatch keeps the built-in material rather than failing a pipeline.
#[cfg(feature = "window")]
pub(crate) fn fits(reference: &str, found: Option<Contract>, wanted: Contract) -> bool {
    match found {
        Some(found) if found != wanted => {
            // Once per material and dimension: a reload empties the cache that
            // would otherwise have remembered it.
            let key = format!("{wanted}:{reference}");
            if balaur_core::logbuf::first_time("shader contract", &key) {
                tracing::warn!(
                    material = reference,
                    "the material's shader draws {found}, so a {wanted} node keeps the built-in one"
                );
            }
            false
        }
        _ => true,
    }
}

/// Make `source` importable as `path` — `package::water`, say.
///
/// For a plugin shipping shader code of its own: a project's material imports
/// it exactly as it imports `package::sprite`.
pub fn register_shader_module(reg: &mut Registry<'_>, path: &str, source: &str) {
    let entry = (path.to_string(), source.to_string());
    if let Some(modules) = reg.engine().try_resource::<ShaderModules>() {
        modules.borrow_mut().0.push(entry);
        return;
    }
    reg.insert_resource(ShaderModules(vec![entry]));
}

/// What plugins registered, for a caller about to link.
pub fn plugin_modules(eng: &Engine) -> Vec<(String, String)> {
    eng.try_resource::<ShaderModules>()
        .map_or_else(Vec::new, |modules| modules.borrow().0.clone())
}

/// Composes `(module path, source)` pairs into one WGSL translation unit,
/// starting from `root` and keeping only what its entry points reach.
///
/// `package::common`, `package::sprite`, `package::mesh` and `constants` are
/// mounted for free.
/// `features` toggles `@if(name)`.
/// Errors name the line the author wrote rather than the linked output's, so
/// a project's shader can say where it broke. The result carries the syntax
/// tree as well as the text, which is what `material` reads its fields from.
pub fn link(
    modules: &[(&str, &str)],
    root: &str,
    features: &[(&str, bool)],
) -> Result<wesl::CompileResult> {
    let mut resolver = wesl::resolver::VirtualResolver::new();
    let mounted = [
        ("package::common", COMMON),
        (SPRITE_MODULE, SPRITE),
        (MESH_MODULE, MESH),
        (PBR_MODULE, PBR),
        (POST_MODULE, POST),
    ];
    for (path, source) in mounted.iter().chain(modules) {
        let parsed = path
            .parse()
            .map_err(|e| anyhow!("shader module path `{path}`: {e}"))?;
        resolver.add_module(parsed, (*source).into());
    }
    let constants = "constants"
        .parse()
        .map_err(|e| anyhow!("shader constants module: {e}"))?;
    resolver.add_module(constants, constants_module().into());
    let mut options = wesl::CompileOptions {
        // Catches a call to a name nothing declares, which otherwise reaches
        // naga and so needs a GPU to find. It does not check types; naga
        // still does that at `create_shader_module`.
        validate: true,
        sourcemap: true,
        ..Default::default()
    };
    for (name, on) in features {
        options.features.set(name, *on);
    }
    let root_path = root
        .parse()
        .map_err(|e| anyhow!("shader root module `{root}`: {e}"))?;
    wesl::Compiler::new_with_resolver(options, resolver)
        .compile_module(&root_path)
        .map_err(|e| anyhow!("linking {root}: {e}"))
}

/// The linked WGSL a backend compiles, lowered: constants folded, branches
/// they decide dropped, and WESL's own `@const` gone.
///
/// The attribute is what lets [`eval_floats`] call a function, so it has to
/// survive linking; WGSL has no such thing, so it must not survive this.
///
/// # Errors
/// If a constant expression does not evaluate.
pub fn wgsl(linked: &wesl::CompileResult) -> Result<String> {
    let mut unit = linked.syntax.clone();
    wesl::pass::lower(&mut unit).map_err(|e| anyhow!("lowering the linked shader: {e}"))?;
    Ok(unit.to_string())
}

/// Evaluate a WGSL expression against a linked shader, as floats.
///
/// The functions it may call are the ones the shader marks `@const`, so a
/// shader helper is testable the way a Rust function is — no GPU, which is
/// the only kind of test this project's CI can run.
pub fn eval_floats(linked: &wesl::CompileResult, expression: &str) -> Result<Vec<f32>> {
    let mut result = linked
        .eval(expression)
        .map_err(|e| anyhow!("evaluating `{expression}`: {e}"))?;
    let bytes = result
        .to_buffer()
        .ok_or_else(|| anyhow!("`{expression}` is not a value with a byte layout"))?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mesh_prepass_links_with_and_without_vertex_colours() {
        for colours in [false, true] {
            let wgsl = link_prepass(colours).expect("the engine's own shader must link");
            assert!(wgsl.contains("fn vs_main"), "{wgsl}");
            assert!(wgsl.contains("fn fs_main"), "{wgsl}");
            // The four targets the screen-space passes read.
            assert!(wgsl.contains("@location(3)"), "{wgsl}");
            assert_eq!(
                wgsl.contains("vertex_tint"),
                colours,
                "the colour attribute belongs only to the variant that asked"
            );
        }
    }

    /// The prepass measures geometry, and a material's own values live in a
    /// group it does not bind. Binding one would be a pipeline that cannot be
    /// built from the material's layout.
    #[test]
    fn the_mesh_prepass_binds_only_the_frame_and_the_object() {
        let wgsl = link_prepass(false).expect("the engine's own shader must link");
        assert!(wgsl.contains("@group(0)"), "{wgsl}");
        assert!(wgsl.contains("@group(1)"), "{wgsl}");
        assert!(!wgsl.contains("@group(2)"), "{wgsl}");
        assert!(!wgsl.contains("@group(3)"), "{wgsl}");
    }

    /// The equirectangular convention the fork builds its skies and probes in.
    /// A direction that reads the wrong texel is a sky rotated or flipped, and
    /// no test with a GPU in it would be cheap enough to catch that.
    #[test]
    fn a_direction_reads_the_environment_where_the_fork_wrote_it() {
        let linked = surroundings();
        let at = |x: f32, y: f32, z: f32| {
            eval_floats(&linked, &format!("at(vec3<f32>({x:?}, {y:?}, {z:?}))")).unwrap()
        };
        let up = at(0.0, 1.0, 0.0);
        assert!(up[1].abs() < 1e-5, "straight up is the top row: {up:?}");
        let down = at(0.0, -1.0, 0.0);
        assert!(
            (down[1] - 1.0).abs() < 1e-5,
            "straight down is the bottom row: {down:?}"
        );
        let front = at(1.0, 0.0, 0.0);
        assert!(
            (front[0] - 0.5).abs() < 1e-5 && (front[1] - 0.5).abs() < 1e-5,
            "+x is the middle of the horizon: {front:?}"
        );
        let behind = at(-1.0, 0.0, 0.0);
        assert!(
            behind[0] < 1e-5 || (behind[0] - 1.0).abs() < 1e-5,
            "-x is the seam, either edge of it: {behind:?}"
        );
    }

    /// Reflections brighten at a glancing angle and a rough surface reflects
    /// less than a smooth one. Both come out of `env_brdf`'s fit, and a sign
    /// slipped in it would light every metal in the engine wrongly.
    #[test]
    fn the_environment_reflects_more_at_a_glance_and_less_when_rough() {
        let linked = surroundings();
        let brdf = |roughness: f32, ndotv: f32| {
            eval_floats(&linked, &format!("reflected({roughness:?}, {ndotv:?})")).unwrap()[0]
        };
        let head_on = brdf(0.1, 1.0);
        let glancing = brdf(0.1, 0.05);
        assert!(
            glancing > head_on,
            "a glancing view reflects more: {glancing} vs {head_on}"
        );
        let smooth = brdf(0.05, 0.5);
        let rough = brdf(0.9, 0.5);
        assert!(
            smooth > rough,
            "a smooth surface reflects more than a rough one: {smooth} vs {rough}"
        );
    }

    /// What `balaur import` writes for a glTF material links against the
    /// contract, in every combination of maps the file may have named. The
    /// shader lives in `balaur_core`, which has no linker; this is the only
    /// place the two meet.
    #[test]
    fn the_imported_gltf_material_links_with_any_maps() {
        for metallic_roughness in [false, true] {
            for emissive in [false, true] {
                let linked = link(
                    &[("package::imported", balaur_core::glb::MATERIAL_SHADER)],
                    "package::imported",
                    &[
                        ("metallic_roughness_map", metallic_roughness),
                        ("emissive_map", emissive),
                    ],
                )
                .unwrap_or_else(|why| {
                    panic!("the imported material must link ({metallic_roughness}, {emissive}): {why:#}")
                });
                let wgsl = wgsl(&linked).unwrap();
                assert!(wgsl.contains("fn fs_main"), "{wgsl}");
                // Glass is a runtime branch on a constant, not a variant, so
                // every combination carries the refraction path.
                assert!(wgsl.contains("transmission"), "{wgsl}");
            }
        }
    }

    /// Each finishing pass links on its own, with one variant reaching the
    /// entry point and the other three stripped.
    #[test]
    fn every_finishing_pass_links_to_one_variant() {
        for name in crate::vocabulary::words::FINISHES {
            let features: Vec<(&str, bool)> = crate::vocabulary::words::FINISHES
                .iter()
                .map(|other| (*other, other == name))
                .collect();
            let wgsl = link(&[("package::finish", FINISH)], "package::finish", &features)
                .map_or_else(
                    |why| panic!("the '{name}' pass must link: {why:#}"),
                    |linked| wgsl(&linked).unwrap(),
                );
            assert!(wgsl.contains("fn fs_main"), "{name}: {wgsl}");
            assert_eq!(
                wgsl.matches("fn finish").count(),
                1,
                "{name} must leave one variant: {wgsl}"
            );
        }
    }

    /// Two variants at once is a shader with two `finish` functions, which is
    /// a link error rather than a silent pick.
    #[test]
    fn two_finishing_passes_at_once_do_not_link() {
        let features = [("vignette", true), ("grain", true)];
        assert!(
            link(&[("package::finish", FINISH)], "package::finish", &features).is_err(),
            "one pass per material, or the entry point is ambiguous"
        );
    }

    /// A model that mirrors a UV shell -- most of them, because it halves the
    /// texture -- has the opposite handedness on one side. Taking the
    /// bitangent as `cross(n, t)` gives both sides the same one, and the two
    /// halves of a mirrored wall then light differently with a seam down the
    /// join.
    #[test]
    fn a_mirrored_uv_shell_flips_the_bitangent() {
        let linked = surroundings();
        let axis = |column: i32, duv1: [f32; 2], duv2: [f32; 2]| {
            let call = format!(
                "frame_axis({column}, vec2<f32>({:?}, {:?}), vec2<f32>({:?}, {:?}))",
                duv1[0], duv1[1], duv2[0], duv2[1]
            );
            eval_floats(&linked, &call).unwrap()
        };
        let plain = axis(1, [1.0, 0.0], [0.0, 1.0]);
        let mirrored = axis(1, [1.0, 0.0], [0.0, -1.0]);
        assert!(
            (plain[1] - 1.0).abs() < 1e-5,
            "v runs up on a plain shell: {plain:?}"
        );
        assert!(
            (mirrored[1] + 1.0).abs() < 1e-5,
            "and down on a mirrored one: {mirrored:?}"
        );
        // The tangent is what the old frame got right, so it must not move.
        let across = axis(0, [1.0, 0.0], [0.0, -1.0]);
        assert!((across[0] - 1.0).abs() < 1e-5, "{across:?}");
    }

    /// Degenerate UVs leave the geometric normal alone rather than dividing
    /// by nothing and lighting the surface with a NaN.
    #[test]
    fn a_surface_with_no_uvs_keeps_the_normal_it_had() {
        let linked = surroundings();
        let call = "frame_axis(2, vec2<f32>(0.0, 0.0), vec2<f32>(0.0, 0.0))";
        let normal = eval_floats(&linked, call).unwrap();
        assert!((normal[2] - 1.0).abs() < 1e-5, "{normal:?}");
        let tangent = eval_floats(
            &linked,
            "frame_axis(0, vec2<f32>(0.0, 0.0), vec2<f32>(0.0, 0.0))",
        )
        .unwrap();
        assert!(tangent.iter().all(|v| v.abs() < 1e-5), "{tangent:?}");
    }

    #[test]
    fn the_skinning_shader_links() {
        let wgsl = link(
            &[("package::skinned_2d", SKINNED_2D)],
            "package::skinned_2d",
            &[],
        )
        .expect("the engine's own shader must link")
        .to_string();
        assert!(wgsl.contains("fn vs_main"), "{wgsl}");
        assert!(wgsl.contains("fn fs_main"), "{wgsl}");
    }

    /// The GPU path has to agree with `skeleton::blend_3d`, which is what a
    /// digest sees; `blend` is `@const` so the two can be compared here,
    /// without a GPU.
    #[test]
    fn the_skinning_blend_matches_the_cpu_reference() {
        let linked = link(&[("package::s", SKINNED_3D)], "package::s", &[])
            .expect("the engine's own shader must link");
        let identity = "mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), \
             vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), \
             vec4<f32>(0.0, 0.0, 0.0, 1.0))";
        let shift = "mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), \
             vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), \
             vec4<f32>(2.0, 0.0, 0.0, 1.0))";
        let point = "vec4<f32>(1.0, 0.0, 0.0, 1.0)";
        let half = format!(
            "blend({shift}, {identity}, {identity}, {identity}, \
             vec4<f32>(0.5, 0.5, 0.0, 0.0), {point})"
        );
        let blended = eval_floats(&linked, &half).unwrap();
        // Half of x + 2, half of x: the midpoint at x = 2.
        assert!((blended[0] - 2.0).abs() < 1e-6, "{blended:?}");

        let unweighted = format!(
            "blend({shift}, {shift}, {shift}, {shift}, \
             vec4<f32>(0.0, 0.0, 0.0, 0.0), {point})"
        );
        let left = eval_floats(&linked, &unweighted).unwrap();
        assert!(
            (left[0] - 1.0).abs() < 1e-6,
            "weights summing to zero must leave the vertex alone: {left:?}"
        );
    }

    /// The light map's three pipelines share one module, so a link that
    /// dropped an entry point would fail at pipeline creation, on a GPU.
    #[test]
    fn the_light_map_shader_keeps_every_entry_point() {
        let wgsl = link(&[("package::light2d", LIGHT_2D)], "package::light2d", &[])
            .expect("the engine's own shader must link")
            .to_string();
        for entry in [
            "fn vs_light",
            "fn fs_light",
            "fn vs_shadow",
            "fn fs_shadow",
            "fn vs_composite",
            "fn fs_composite",
        ] {
            assert!(wgsl.contains(entry), "{entry} was dropped: {wgsl}");
        }
    }

    #[test]
    fn an_imported_helper_arrives_in_the_output() {
        let wgsl = link(
            &[("package::skinned_2d", SKINNED_2D)],
            "package::skinned_2d",
            &[],
        )
        .unwrap()
        .to_string();
        assert!(
            !wgsl.contains("import "),
            "imports must be resolved away: {wgsl}"
        );
        assert!(wgsl.contains("mat3x3<f32>(a.xyz, b.xyz, c.xyz)"), "{wgsl}");
    }

    /// A shader that exists to be asserted on. The wrappers are declared at
    /// the root because an imported name is mangled and a root one is not,
    /// and `@const` is what lets `eval_floats` call them.
    const PROBE: &str = r"
import package::mesh::{Light, contribution, VertexInput, VertexOutput, vertex};

@const fn directional(direction: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    var light: Light;
    light.position_kind = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    light.direction_radius = vec4<f32>(direction, 0.0);
    light.color_intensity = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    return contribution(light, vec3<f32>(0.0), normal);
}

@const fn point(position: vec3<f32>, radius: f32) -> vec3<f32> {
    var light: Light;
    light.position_kind = vec4<f32>(position, 1.0);
    light.direction_radius = vec4<f32>(0.0, 0.0, 0.0, radius);
    light.color_intensity = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    return contribution(light, vec3<f32>(0.0), vec3<f32>(0.0, 1.0, 0.0));
}

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main() -> @location(0) vec4<f32> {
    let a = directional(vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0));
    return vec4<f32>(a + point(vec3<f32>(0.0, 1.0, 0.0), 4.0), 1.0);
}
";

    fn probe() -> wesl::CompileResult {
        link(&[("package::probe", PROBE)], "package::probe", &[])
            .expect("the probe shader must link")
    }

    /// The same trick for what a surface reads off its surroundings: linking
    /// keeps only what an entry point reaches, so the helpers under test are
    /// wrapped and called.
    const SURROUNDINGS: &str = r"
import package::mesh::{equirect_uv, tangent_frame, VertexInput, VertexOutput, vertex};
import package::pbr::{env_brdf};

@const fn at(d: vec3<f32>) -> vec2<f32> {
    return equirect_uv(d);
}

@const fn reflected(roughness: f32, ndotv: f32) -> vec3<f32> {
    return env_brdf(vec3<f32>(0.04, 0.04, 0.04), roughness, ndotv);
}

// A flat surface in the xy plane, with the UV derivatives handed in: the
// tangent and the bitangent the frame solves for.
@const fn frame_axis(column: i32, duv1: vec2<f32>, duv2: vec2<f32>) -> vec3<f32> {
    let frame = tangent_frame(
        vec3<f32>(0.0, 0.0, 1.0),
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        duv1,
        duv2,
    );
    return frame[column];
}

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main() -> @location(0) vec4<f32> {
    let uv = at(vec3<f32>(1.0, 0.0, 0.0));
    let axis = frame_axis(1, vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0));
    return vec4<f32>(reflected(0.5, 0.5) + vec3<f32>(uv, 0.0) + axis, 1.0);
}
";

    fn surroundings() -> wesl::CompileResult {
        link(
            &[("package::surroundings", SURROUNDINGS)],
            "package::surroundings",
            &[],
        )
        .expect("the surroundings probe must link")
    }

    #[test]
    fn the_const_attribute_never_reaches_the_output() {
        // WGSL has no `@const`; a shader carrying one is one naga rejects.
        let linked = probe();
        assert!(linked.to_string().contains("@const"), "linking keeps it");
        assert!(
            !wgsl(&linked).unwrap().contains("@const"),
            "the output drops it"
        );
    }

    /// A light's intensity is radiance, so Lambert's `1/π` is what turns it
    /// into reflected colour. Without it the engine's own default intensity
    /// of 3.0 draws white.
    #[test]
    fn a_light_straight_on_contributes_its_colour_over_pi() {
        let lit = eval_floats(
            &probe(),
            "directional(vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(0.0, 1.0, 0.0))",
        )
        .unwrap();
        let expected = 1.0 / std::f32::consts::PI;
        for channel in lit {
            assert!((channel - expected).abs() < 1e-6, "{channel} != {expected}");
        }
    }

    #[test]
    fn a_light_behind_the_surface_contributes_nothing() {
        let lit = eval_floats(
            &probe(),
            "directional(vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0))",
        )
        .unwrap();
        assert_eq!(lit, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_point_light_past_its_radius_contributes_nothing() {
        let lit = eval_floats(&probe(), "point(vec3<f32>(0.0, 10.0, 0.0), 4.0)").unwrap();
        assert_eq!(lit, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_point_light_inside_its_radius_contributes_something() {
        let lit = eval_floats(&probe(), "point(vec3<f32>(0.0, 1.0, 0.0), 4.0)").unwrap();
        assert!(lit[0] > 0.0, "{lit:?}");
    }

    #[test]
    fn a_material_cannot_import_a_contract_s_private_helper() {
        let source = "import package::pbr::distribution;
        @fragment fn fs_main() -> @location(0) vec4<f32> {
            return vec4<f32>(distribution(0.5, 0.5));
        }";
        let err = link(&[("package::m", source)], "package::m", &[])
            .err()
            .expect("a private helper must not link into a material");
        assert!(format!("{err:#}").contains("private"), "{err:#}");
    }

    #[test]
    fn a_shader_sizes_its_arrays_from_the_engine_s_own_limits() {
        let source = "import constants::{MAX_LIGHTS, MAX_PROBES, MAX_JOINTS};
        @const fn limits() -> vec3<f32> {
            return vec3<f32>(f32(MAX_LIGHTS), f32(MAX_PROBES), f32(MAX_JOINTS));
        }
        @fragment fn fs_main() -> @location(0) vec4<f32> {
            return vec4<f32>(limits(), 1.0);
        }";
        let linked = link(&[("package::m", source)], "package::m", &[]).unwrap();
        let expected = [MAX_LIGHTS, MAX_PROBES, MAX_JOINTS].map(|n| n as f32);
        assert_eq!(eval_floats(&linked, "limits()").unwrap(), expected);
    }

    #[test]
    fn a_call_to_a_name_nothing_declares_is_caught_without_a_gpu() {
        let source = "@fragment fn fs_main() -> @location(0) vec4<f32> {
            return vec4<f32>(nonesuch(1.0));
        }";
        let err = link(&[("package::bad", source)], "package::bad", &[])
            .err()
            .expect("validation must reject a call to nothing");
        assert!(format!("{err:#}").contains("nonesuch"), "{err:#}");
    }

    #[test]
    fn every_channel_links_to_one_entry_point() {
        for channel in CHANNELS {
            let features: Vec<(&str, bool)> = CHANNELS.iter().map(|c| (*c, c == channel)).collect();
            let unit = link(&[("package::c", CHANNEL)], "package::c", &features)
                .unwrap_or_else(|why| panic!("channel `{channel}`: {why:#}"));
            let wgsl = wgsl(&unit).unwrap();
            assert_eq!(
                wgsl.matches("fn fs_main").count(),
                1,
                "channel `{channel}` kept more than one fragment stage: {wgsl}"
            );
        }
    }

    #[test]
    fn every_channel_links_in_2d_too() {
        for channel in CHANNELS {
            let features: Vec<(&str, bool)> = CHANNELS.iter().map(|c| (*c, c == channel)).collect();
            let unit = link(&[("package::c", CHANNEL_2D)], "package::c", &features)
                .unwrap_or_else(|why| panic!("channel `{channel}`: {why:#}"));
            assert_eq!(
                wgsl(&unit).unwrap().matches("fn fs_main").count(),
                1,
                "channel `{channel}` kept more than one fragment stage"
            );
        }
    }

    #[test]
    fn a_channel_draws_what_its_name_says() {
        let features: Vec<(&str, bool)> = CHANNELS.iter().map(|c| (*c, *c == "normals")).collect();
        let wgsl =
            wgsl(&link(&[("package::c", CHANNEL)], "package::c", &features).unwrap()).unwrap();
        assert!(wgsl.contains("normalize"), "{wgsl}");
        assert!(!wgsl.contains("exp("), "the depth channel came too: {wgsl}");
    }

    #[test]
    fn a_shader_that_does_not_parse_is_an_error_not_a_panic() {
        let err = link(&[("package::bad", "fn broken(")], "package::bad", &[])
            .err()
            .expect("a malformed shader must come back as an error");
        assert!(format!("{err}").contains("package::bad"), "{err}");
    }
}
