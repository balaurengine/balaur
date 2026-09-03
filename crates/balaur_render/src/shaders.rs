//! Balaur's shaders, and the WESL linker every shader goes through.
//!
//! Shaders are written in WESL — WGSL plus imports, `@if` variants and dead
//! code elimination — and linked to plain WGSL before a backend compiles
//! them. Linking happens at run time rather than in `build.rs` so that the
//! engine's own shaders and a project's take one path, and here rather than
//! behind the `kiss3d` feature because linking needs no GPU: a shader that
//! does not link is a bug a headless test can catch.

use anyhow::{anyhow, Result};
use balaur_core::{App, Engine};

/// Helpers any shader may `import package::common::…`.
static COMMON: &str = include_str!("shaders/common.wesl");

/// The 2D skinning material's shader.
pub static SKINNED_2D: &str = include_str!("shaders/skinned_2d.wesl");

/// The contract a project's 2D material shader draws against, mounted as
/// `package::sprite`: the uniforms the pipeline binds and the vertex work
/// every such shader would otherwise repeat.
pub(crate) static SPRITE: &str = include_str!("shaders/sprite.wesl");

/// The 3D counterpart, mounted as `package::mesh`: the same uniforms in three
/// dimensions, plus the scene's lights and fog.
pub(crate) static MESH: &str = include_str!("shaders/mesh.wesl");

/// What a channel view draws: one entry point per channel, chosen by feature.
pub static CHANNEL: &str = include_str!("shaders/channel.wesl");

/// The 2D counterpart of [`CHANNEL`].
pub static CHANNEL_2D: &str = include_str!("shaders/channel2d.wesl");

/// The channels [`CHANNEL`] can draw, in the order a menu lists them.
pub const CHANNELS: &[&str] = &["albedo", "normals", "uv", "depth"];

/// Shader modules a plugin added, mounted beside the engine's own.
///
/// Ordered, not hashed: a link is over the same modules in the same order
/// every run, whoever registered them.
#[derive(Default)]
pub struct ShaderModules(pub Vec<(String, String)>);

/// Make `source` importable as `path` — `package::water`, say.
///
/// For a plugin shipping shader code of its own: a project's material imports
/// it exactly as it imports `package::sprite`.
pub fn register_shader_module(app: &mut App, path: &str, source: &str) {
    let entry = (path.to_string(), source.to_string());
    if let Some(modules) = app.engine.try_resource::<ShaderModules>() {
        modules.borrow_mut().0.push(entry);
        return;
    }
    app.engine.insert_resource(ShaderModules(vec![entry]));
}

/// What plugins registered, for a caller about to link.
pub fn plugin_modules(eng: &Engine) -> Vec<(String, String)> {
    eng.try_resource::<ShaderModules>()
        .map_or_else(Vec::new, |modules| modules.borrow().0.clone())
}

/// Composes `(module path, source)` pairs into one WGSL translation unit,
/// starting from `root` and keeping only what its entry points reach.
///
/// `package::common`, `package::sprite` and `package::mesh` are mounted for
/// free.
/// `features` toggles `@if(name)`.
/// Errors name the line the author wrote rather than the linked output's, so
/// a project's shader can say where it broke. The result carries the syntax
/// tree as well as the text, which is what `material` reads its fields from.
pub fn link(
    modules: &[(&str, &str)],
    root: &str,
    features: &[(&str, bool)],
) -> Result<wesl::CompileResult> {
    let mut resolver = wesl::VirtualResolver::new();
    let mounted = [
        ("package::common", COMMON),
        ("package::sprite", SPRITE),
        ("package::mesh", MESH),
    ];
    for (path, source) in mounted.iter().chain(modules) {
        let parsed = path
            .parse()
            .map_err(|e| anyhow!("shader module path `{path}`: {e}"))?;
        resolver.add_module(parsed, (*source).into());
    }
    let mut compiler = wesl::Wesl::new("").set_custom_resolver(resolver);
    compiler.set_options(wesl::CompileOptions {
        // Catches a call to a name nothing declares, which otherwise reaches
        // naga and so needs a GPU to find. It does not check types; naga
        // still does that at `create_shader_module`.
        validate: true,
        ..Default::default()
    });
    compiler.use_sourcemap(true);
    for (name, on) in features {
        compiler.set_feature(name, *on);
    }
    let root_path = root
        .parse()
        .map_err(|e| anyhow!("shader root module `{root}`: {e}"))?;
    compiler
        .compile(&root_path)
        .map_err(|e| anyhow!("linking {root}: {e}"))
}

/// The linked WGSL a backend compiles, with WESL's own `@const` dropped.
///
/// The attribute is what lets [`eval_floats`] call a function, so it has to
/// survive linking; WGSL has no such thing, so it must not survive this.
pub fn wgsl(linked: &wesl::CompileResult) -> String {
    let mut unit = linked.syntax.clone();
    for declaration in &mut unit.global_declarations {
        if let wesl::syntax::GlobalDeclaration::Function(function) = declaration.node_mut() {
            function
                .attributes
                .retain(|a| !matches!(a.node(), wesl::syntax::Attribute::Const));
        }
    }
    unit.to_string()
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

    #[test]
    fn the_const_attribute_never_reaches_the_output() {
        // WGSL has no `@const`; a shader carrying one is one naga rejects.
        let linked = probe();
        assert!(linked.to_string().contains("@const"), "linking keeps it");
        assert!(!wgsl(&linked).contains("@const"), "the output drops it");
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
            let wgsl = wgsl(&unit);
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
                wgsl(&unit).matches("fn fs_main").count(),
                1,
                "channel `{channel}` kept more than one fragment stage"
            );
        }
    }

    #[test]
    fn a_channel_draws_what_its_name_says() {
        let features: Vec<(&str, bool)> = CHANNELS.iter().map(|c| (*c, *c == "normals")).collect();
        let wgsl = wgsl(&link(&[("package::c", CHANNEL)], "package::c", &features).unwrap());
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
