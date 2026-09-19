//! `gd.rn`: Godot's Variant semantics as one Rune module.
//!
//! Godot answers `size()`, `is_empty()` and `get()` on strings, arrays,
//! dictionaries and objects alike, and Rune's types do not. Rather than infer
//! a static type for every expression, the emitter routes those calls here and
//! this dispatches on the value.

/// The text written to `gd.rn` beside a converted project, kept as Rune
/// files so they read and lint as Rune.
pub(crate) const SHIM: &str = concat!(include_str!("gd_values.rn"), include_str!("gd_nodes.rn"));

#[cfg(test)]
mod tests {
    /// Every `(gd.name)` the translator writes names a function the shim
    /// defines: one it lacks compiles, and fails only when a game reaches it.
    #[test]
    fn every_shim_call_the_translator_writes_is_defined() {
        let written = [
            include_str!("map.rs"),
            include_str!("emit.rs"),
            include_str!("emit/calls.rs"),
            include_str!("../script.rs"),
            include_str!("../script/members.rs"),
            include_str!("../resource.rs"),
        ];
        let defined: std::collections::BTreeSet<&str> = super::SHIM
            .lines()
            .filter_map(|line| {
                let rest = line
                    .strip_prefix("pub fn ")
                    .or_else(|| line.strip_prefix("pub async fn "))?;
                rest.split('(').next()
            })
            .collect();
        let mut missing = Vec::new();
        for text in written {
            for piece in text.split("(gd.").skip(1) {
                let name: String = piece
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if piece[name.len()..].starts_with(')') && !defined.contains(name.as_str()) {
                    missing.push(name);
                }
            }
        }
        assert!(missing.is_empty(), "the shim defines none of {missing:?}");
    }

    /// The shim is Rune text inside Rust: nothing but running it says it
    /// still compiles, and every converted script requires it.
    #[test]
    fn the_shim_compiles_and_its_vectors_add() {
        let dir = tempfile::tempdir().unwrap();
        let put = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        put(
            "project.toml",
            "[application]\nname = \"shim\"\nmain_scene = \"main.toml\"\n",
        );
        put(
            "main.toml",
            "[[nodes]]\nid = \"probe\"\nname = \"Probe\"\nscript = { source = \"probe.rn\" }\n",
        );
        put("gd.rn", super::SHIM);
        put(
            "probe.rn",
            "pub async fn init(this) {\n\
             \x20   let gd = script::require(\"gd.rn\");\n\
             \x20   let v = (gd.vec2)(1.0, 2.0) + (gd.neg)((gd.vec2)(0.5, 0.5));\n\
             \x20   let waited = (gd.invoke_async1)(#{ \"f\": async |x| x + 1 }, \"f\", 1).await;\n\
             \x20   if (gd.field)(v, \"x\") == 0.5 && (gd.color_of)(\"ff0000\").r == 1.0 && waited == 2\n\
             \x20       && (gd.find)(\"a=b\", \"=\") == 1 && (gd.size)([1, 2]) - 3 == -1 {\n\
             \x20       this.node.set_visible(false);\n\
             \x20   }\n\
             }\n",
        );
        let mut config = balaur::AppConfig::dev(dir.path().to_string_lossy().as_ref());
        config.watch = false;
        let mut app = balaur::standard_app(config).unwrap();
        app.load_project().unwrap();
        app.tick(1.0 / 60.0);
        let world = app.engine.world();
        let probe = balaur_core::scene::find_node(&world, app.engine.root(), "Probe").unwrap();
        assert!(
            !world
                .get::<&balaur_core::scene::Appearance>(probe)
                .unwrap()
                .visible,
            "the probe hid itself only if the shim compiled and did the sums"
        );
    }
}
