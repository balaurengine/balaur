//! Every engine shader, linked in each variant it ships in and validated the
//! way wgpu validates it at `create_shader_module`: types, bindings and
//! uniformity. The WESL link alone checks names, so a shader that links can
//! still be one no device accepts.

use crate::shaders::{self, MORPH, link, wgsl};

/// `wgsl` parsed and validated by the naga wgpu itself runs, with the
/// capabilities every backend Balaur draws on offers, then written as GLSL ES.
fn validate(what: &str, wgsl: &str) {
    let module = naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|why| {
        panic!(
            "{what} does not parse: {}\n{wgsl}",
            why.emit_to_string(wgsl)
        )
    });
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|why| {
        panic!(
            "{what} does not validate: {}\n{wgsl}",
            why.emit_to_string(wgsl)
        )
    });
    gles(what, &module, &info);
}

/// Every entry point written the way wgpu's GLES backend writes it on an
/// Android device without Vulkan: GLSL ES 3.10, which lacks what only desktop
/// GL has, such as asking a texture how many mips it holds.
fn gles(what: &str, module: &naga::Module, info: &naga::valid::ModuleInfo) {
    use naga::back::glsl;
    assert_eq!(
        plain_depth_samples(module),
        0,
        "{what} samples a depth texture without a comparison, which GLSL cannot"
    );
    let mut options = glsl::Options::default();
    options.writer_flags |= glsl::WriterFlags::FORCE_POINT_SIZE;
    for entry in &module.entry_points {
        let pipeline = glsl::PipelineOptions {
            shader_stage: entry.stage,
            entry_point: entry.name.clone(),
            multiview: None,
        };
        let mut out = String::new();
        glsl::Writer::new(
            &mut out,
            module,
            info,
            &options,
            &pipeline,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .and_then(|mut writer| writer.write())
        .unwrap_or_else(|why| panic!("{what}'s `{}` is no GLSL ES 3.10: {why}", entry.name));
    }
}

/// How many times `module` samples a depth texture without a comparison. GLSL
/// reads one only through a shadow sampler, so naga writes such a sample and
/// the device's compiler then rejects it.
fn plain_depth_samples(module: &naga::Module) -> usize {
    let is_depth = |expressions: &naga::Arena<naga::Expression>, image| match expressions[image] {
        naga::Expression::GlobalVariable(var) => matches!(
            module.types[module.global_variables[var].ty].inner,
            naga::TypeInner::Image {
                class: naga::ImageClass::Depth { .. },
                ..
            }
        ),
        _ => false,
    };
    module
        .functions
        .iter()
        .map(|(_, function)| function)
        .chain(module.entry_points.iter().map(|entry| &entry.function))
        .map(|function| {
            function
                .expressions
                .iter()
                .filter(|(_, expression)| match expression {
                    naga::Expression::ImageSample {
                        image,
                        depth_ref: None,
                        ..
                    } => is_depth(&function.expressions, *image),
                    _ => false,
                })
                .count()
        })
        .sum()
}

/// A project's shader over `package::mesh` or `package::pbr`, linked as a
/// material links it, with `features`.
fn material(what: &str, source: &str, features: &[(&str, bool)]) -> String {
    let linked = link(
        &[("package::material", source)],
        "package::material",
        features,
    )
    .unwrap_or_else(|why| panic!("{what} does not link: {why:#}"));
    wgsl(&linked).expect("a linked module prints as WGSL")
}

const LAMBERT: &str = r"
import package::mesh::{VertexInput, VertexOutput, vertex, shade};
@vertex fn vs_main(in: VertexInput) -> VertexOutput { return vertex(in); }
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> { return shade(in); }
";

/// Every lobe `package::pbr` has: tint, clear coat, anisotropy and parallax.
const EVERY_LOBE: &str = r"
import package::mesh::{VertexInput, VertexOutput, vertex};
import package::pbr::{parallax_surface, default_parallax, shade_pbr};
@vertex fn vs_main(in: VertexInput) -> VertexOutput { return vertex(in); }
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var p = default_parallax();
    p.relief_steps = 8.0;
    var s = parallax_surface(in, p);
    s.specular_tint = vec3<f32>(1.0, 0.2, 0.2);
    s.clearcoat = 1.0;
    s.clearcoat_roughness = 0.05;
    s.anisotropy = 0.8;
    s.anisotropy_rotation = 0.5;
    return shade_pbr(in, s);
}
";

#[test]
fn a_lambert_material_validates_with_and_without_morph_targets() {
    for morph in [false, true] {
        let wgsl = material("the Lambert material", LAMBERT, &[(MORPH, morph)]);
        assert_eq!(wgsl.contains("morph_positions"), morph, "{wgsl}");
        // Shadows reach the plain look too.
        assert!(wgsl.contains("shadow_atlas"), "{wgsl}");
        validate("the Lambert material", &wgsl);
    }
}

#[test]
fn a_material_using_every_pbr_lobe_validates() {
    for morph in [false, true] {
        let wgsl = material("the every-lobe material", EVERY_LOBE, &[(MORPH, morph)]);
        validate("the every-lobe material", &wgsl);
    }
}

#[test]
fn linking_a_material_again_writes_the_same_wgsl() {
    let first = material("the every-lobe material", EVERY_LOBE, &[]);
    for _ in 0..8 {
        let again = material("the every-lobe material", EVERY_LOBE, &[]);
        assert!(
            again == first,
            "an export packs this text, so a link must not reorder it"
        );
    }
}

#[test]
fn the_default_pbr_look_validates() {
    let source = r"
import package::mesh::{VertexInput, VertexOutput, vertex};
import package::pbr::shade;
@vertex fn vs_main(in: VertexInput) -> VertexOutput { return vertex(in); }
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> { return shade(in); }
";
    validate(
        "the default PBR look",
        &material("pbr", source, &[(MORPH, true)]),
    );
}

#[test]
fn the_skinning_shader_validates_with_and_without_morph_targets() {
    for morph in [false, true] {
        let linked = link(
            &[("package::skinned_3d", shaders::SKINNED_3D)],
            "package::skinned_3d",
            &[(MORPH, morph)],
        )
        .expect("the engine's own shader must link");
        let wgsl = wgsl(&linked).unwrap();
        assert!(wgsl.contains("fn fs_oit"), "{wgsl}");
        validate("the skinning shader", &wgsl);
    }
}

#[test]
fn the_prepass_validates_in_every_variant() {
    for colours in [false, true] {
        for morph in [false, true] {
            let wgsl = shaders::link_prepass(colours, morph).expect("the prepass must link");
            validate("the prepass", &wgsl);
        }
    }
}

#[test]
fn the_imported_gltf_material_validates() {
    for unlit in [false, true] {
        let linked = link(
            &[("package::imported", balaur_core::glb::MATERIAL_SHADER)],
            "package::imported",
            &[
                ("metallic_roughness_map", true),
                ("emissive_map", true),
                ("unlit", unlit),
                (MORPH, true),
            ],
        )
        .expect("the imported material must link");
        validate("the imported glTF material", &wgsl(&linked).unwrap());
    }
}

#[test]
fn a_material_s_transparency_entry_point_validates() {
    let asset =
        crate::material::parse(&toml::from_str("shader = \"shaders/rock.wesl\"").unwrap()).unwrap();
    let wgsl = crate::material::transparent_variant(&asset, LAMBERT, &[], true)
        .expect("a vec4 fragment takes the transparency entry point");
    validate("the transparency variant", &wgsl);
}

#[test]
fn the_2d_shaders_validate() {
    for (name, source) in [
        ("package::skinned_2d", shaders::SKINNED_2D),
        ("package::light2d", shaders::LIGHT_2D),
    ] {
        let linked = link(&[(name, source)], name, &[]).expect("the engine's own shader must link");
        validate(name, &wgsl(&linked).unwrap());
    }
}
