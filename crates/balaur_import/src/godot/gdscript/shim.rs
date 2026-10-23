//! `gd.bs`: Godot's Variant semantics as one module.
//!
//! Godot answers `size()`, `is_empty()` and `get()` on strings, arrays,
//! dictionaries and objects alike. Where the emitter does not know a value's
//! type it routes the call here, and this dispatches on the value.

/// The text written to `gd.bs` beside a converted project.
pub(crate) const SHIM: &str = include_str!("gd.bs");

/// Each shim function's parameter types, read off its declaration.
fn signatures() -> &'static std::collections::BTreeMap<String, Vec<String>> {
    static SIGNATURES: std::sync::OnceLock<std::collections::BTreeMap<String, Vec<String>>> =
        std::sync::OnceLock::new();
    SIGNATURES.get_or_init(|| {
        SHIM.lines()
            .filter_map(|line| {
                let rest = line
                    .strip_prefix("pub fn ")
                    .or_else(|| line.strip_prefix("pub async fn "))?;
                let (name, rest) = rest.split_once('(')?;
                let (params, _) = rest.split_once(')')?;
                let types = params
                    .split(',')
                    .filter(|p| !p.trim().is_empty())
                    .map(|p| {
                        p.split_once(':')
                            .map_or("any", |(_, t)| t.trim())
                            .to_string()
                    })
                    .collect();
                Some((name.to_string(), types))
            })
            .collect()
    })
}

/// `args` filled to every parameter `verb` declares: what Godot let the call
/// leave out is nil, or the zero of a typed parameter.
pub(crate) fn padded(verb: &str, args: &mut Vec<String>) {
    let Some(types) = signatures().get(verb) else {
        return;
    };
    while args.len() < types.len() {
        let zero = match types[args.len()].as_str() {
            "int" => "0",
            "float" => "0.0",
            "bool" => "false",
            "str" => "\"\"",
            _ => "nil",
        };
        args.push(zero.into());
    }
}

/// The script a tween's callback step runs, written beside the shim.
pub(crate) const CALLBACK: &str = include_str!("gd_callback.bs");

#[cfg(test)]
mod tests {
    /// Every `gd::name(` the translator writes names a function the shim
    /// defines: one it lacks fails the converted script's compile.
    #[test]
    fn every_shim_call_the_translator_writes_is_defined() {
        let written = [
            include_str!("map.rs"),
            include_str!("map/controls.rs"),
            include_str!("map/events.rs"),
            include_str!("map/globals.rs"),
            include_str!("map/widgets.rs"),
            include_str!("emit.rs"),
            include_str!("emit/calls.rs"),
            include_str!("emit/ops.rs"),
            include_str!("emit/writes.rs"),
            include_str!("../script.rs"),
            include_str!("../script/class.rs"),
            include_str!("../script/funcs.rs"),
            include_str!("../script/input.rs"),
            include_str!("../script/members.rs"),
            include_str!("../script/draw.rs"),
            include_str!("../script/collision.rs"),
            include_str!("../resource.rs"),
        ];
        let defined = defined();
        let mut missing = std::collections::BTreeSet::new();
        for text in written {
            for piece in text.split("gd::").skip(1) {
                let name: String = piece
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty()
                    && piece[name.len()..].starts_with('(')
                    && !defined.contains(&name)
                {
                    missing.insert(name);
                }
            }
        }
        assert!(missing.is_empty(), "the shim defines none of {missing:?}");
    }

    fn defined() -> std::collections::BTreeSet<String> {
        super::SHIM
            .lines()
            .filter_map(|line| {
                let rest = line
                    .strip_prefix("pub fn ")
                    .or_else(|| line.strip_prefix("pub async fn "))?;
                Some(rest.split('(').next()?.to_string())
            })
            .collect()
    }

    /// The records the translator writes are read by the shim under the
    /// same keys.
    #[test]
    fn the_shim_reads_the_record_keys_the_translator_writes() {
        use crate::godot::gdscript::map;
        for (name, key) in [
            ("TARGET_KEY", map::BOUND_OWNER),
            ("METHOD_KEY", map::BOUND_METHOD),
            ("ARGS_KEY", map::BOUND_ARGS),
            ("TAKES_KEY", map::CALL_TAKES),
            ("CLASS_KEY", crate::godot::script::CLASS_KEY),
        ] {
            let line = format!("pub const {name}: str = \"{key}\";");
            assert!(super::SHIM.contains(&line), "the shim has no `{line}`");
        }
    }

    /// The shim compiles against the engine, beside a project's dispatcher
    /// and settings, as every converted script links it.
    #[test]
    fn the_shim_compiles() {
        let classes =
            crate::godot::exports::classes_module(&crate::godot::exports::Classes::default(), &[]);
        let settings = "pub fn setting(path: str) -> any {\n    nil\n}\n".to_string();
        let files = [
            ("gd.bs", super::SHIM.to_string()),
            ("classes.bs", classes),
            ("godot_settings.bs", settings),
        ];
        let errors = crate::godot::gdscript::check::errors(&files, "gd.bs");
        assert!(errors.is_empty(), "{errors:#?}");
    }
}
