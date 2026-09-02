//! Balaur's shaders, and the WESL linker every shader goes through.
//!
//! Shaders are written in WESL — WGSL plus imports, `@if` variants and dead
//! code elimination — and linked to plain WGSL before a backend compiles
//! them. Linking happens at run time rather than in `build.rs` so that the
//! engine's own shaders and a project's take one path, and here rather than
//! behind the `kiss3d` feature because linking needs no GPU: a shader that
//! does not link is a bug a headless test can catch.

use anyhow::{anyhow, Result};

/// Helpers any shader may `import package::common::…`.
static COMMON: &str = include_str!("shaders/common.wesl");

/// The 2D skinning material's shader.
pub static SKINNED_2D: &str = include_str!("shaders/skinned_2d.wesl");

/// The contract a project's 2D material shader draws against, mounted as
/// `package::sprite`: the uniforms the pipeline binds and the vertex work
/// every such shader would otherwise repeat.
static SPRITE: &str = include_str!("shaders/sprite.wesl");

/// Composes `(module path, source)` pairs into one WGSL translation unit,
/// starting from `root` and keeping only what its entry points reach.
///
/// `package::common` and `package::sprite` are mounted for free.
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
    let mounted = [("package::common", COMMON), ("package::sprite", SPRITE)];
    for (path, source) in mounted.iter().chain(modules) {
        let parsed = path
            .parse()
            .map_err(|e| anyhow!("shader module path `{path}`: {e}"))?;
        resolver.add_module(parsed, (*source).into());
    }
    let mut compiler = wesl::Wesl::new("").set_custom_resolver(resolver);
    compiler.set_options(wesl::CompileOptions {
        // wesl validates only with its `eval` feature; naga validates the
        // linked output at `create_shader_module` either way.
        validate: false,
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

    #[test]
    fn a_shader_that_does_not_parse_is_an_error_not_a_panic() {
        let err = link(&[("package::bad", "fn broken( {")], "package::bad", &[])
            .err()
            .expect("a malformed shader must come back as an error");
        assert!(format!("{err}").contains("package::bad"), "{err}");
    }
}
