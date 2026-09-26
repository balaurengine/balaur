//! `project.godot` as a `project.toml`.
//!
//! What carries across is what both engines have: the name, the main scene,
//! the window, the locales and the input map. A script autoload becomes a
//! node of the main scene; every key neither engine shares is reported.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::Result;

use crate::godot::{Document, Value, keys};

/// A converted project, and what would not convert.
pub(crate) struct Converted {
    pub project_toml: String,
    /// `godot_settings.rn`: every plain setting `project.godot` holds, which
    /// a converted `ProjectSettings.get` reads.
    pub settings_module: String,
    /// One line per thing that did not carry, for the import report.
    pub notes: Vec<String>,
}

/// Read a `project.godot` and lay out the `project.toml` beside it.
///
/// `uids` resolves `uid://` references to project-relative paths; an empty
/// map is fine and leaves them reported instead.
pub(crate) fn convert(
    document: &Document,
    uids: &BTreeMap<String, String>,
    ignore: &[String],
) -> Result<Converted> {
    let mut notes = Vec::new();
    let mut out = String::new();
    let get = |section: &str, key: &str| {
        document
            .first(section)
            .and_then(|s| s.field(key))
            .map(std::borrow::ToOwned::to_owned)
    };

    let name = get("application", "config/name")
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or("game")
        .to_string();
    let main = get("application", "run/main_scene");
    let main_scene = main
        .as_ref()
        .and_then(Value::as_str)
        .map_or_else(String::new, |r| resolve(r, uids, &mut notes));
    writeln!(out, "[application]")?;
    writeln!(out, "name = {}", quote(&name))?;
    writeln!(out, "main_scene = {}", quote(&main_scene))?;
    if let Some(splash) = get("application", "boot_splash/image")
        .as_ref()
        .and_then(Value::as_str)
    {
        let path = resolve(splash, uids, &mut notes);
        if !path.is_empty() {
            writeln!(out, "splash = {}", quote(&path))?;
        }
    }

    if !ignore.is_empty() {
        let patterns: Vec<String> = ignore.iter().map(|p| quote(p)).collect();
        writeln!(out, "ignore = [{}]", patterns.join(", "))?;
    }

    window(document, &mut out, &mut notes)?;
    if let Some(theme) = get("gui", "theme/custom").as_ref().and_then(Value::as_str) {
        let path = resolve(theme, uids, &mut notes);
        if let Some(godot) = path.strip_suffix(".tres") {
            writeln!(out, "\n[ui]")?;
            writeln!(
                out,
                "theme = {}",
                quote(&crate::godot::theme::theme_path(&format!("{godot}.tres")))
            )?;
        }
    }
    locale(document, &mut out)?;
    actions(document, &mut out, &mut notes)?;

    for section in document.each("autoload") {
        for (key, value) in &section.fields {
            // A script autoload becomes a node under the main scene's root;
            // a scene autoload has no node of its own to become.
            if !value.as_str().is_some_and(crate::godot::exports::is_script) {
                notes.push(format!(
                    "autoload `{key}`: a scene autoload has no node here; give it to the main scene"
                ));
            }
        }
    }
    Ok(Converted {
        project_toml: out,
        settings_module: settings_module(document),
        notes,
    })
}

/// What every preset in `export_presets.cfg` leaves out, as the patterns of
/// `application/ignore`: what no build of the game ships. Godot's `*` also
/// crosses a `/`, and a pattern with no `/` names a file anywhere.
pub(crate) fn shared_exclusions(presets: &Document) -> Vec<String> {
    let lists: Vec<Vec<String>> = presets
        .sections
        .iter()
        .filter(|section| {
            section.kind.starts_with("preset.") && !section.kind.ends_with(".options")
        })
        .map(|section| {
            let filter = section
                .field("exclude_filter")
                .and_then(Value::as_str)
                .unwrap_or_default();
            filter
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(crate::godot::relative_path)
                .map(|p| {
                    // The file the import writes for a scene or a script.
                    use crate::godot::scene::{scene_path, script_path};
                    let converted = script_path(&scene_path(p));
                    let pattern = converted.replace('*', "**").replace("****", "**");
                    if pattern.contains('/') {
                        pattern
                    } else {
                        format!("**/{pattern}")
                    }
                })
                .collect()
        })
        .collect();
    let Some((first, rest)) = lists.split_first() else {
        return Vec::new();
    };
    first
        .iter()
        .filter(|pattern| rest.iter().all(|list| list.contains(pattern)))
        .cloned()
        .collect()
}

/// Every string, number and bool `project.godot` sets, under Godot's own
/// `section/key` path, as a Rune module the shim's `project_setting` asks.
fn settings_module(document: &Document) -> String {
    let mut arms = String::new();
    for section in &document.sections {
        for (key, value) in &section.fields {
            let literal = match value {
                Value::Bool(b) => b.to_string(),
                Value::Int(n) => n.to_string(),
                Value::Float(f) if f.is_finite() => format!("{f:?}"),
                Value::Str(text) | Value::Name(text) => crate::godot::gdscript::quoted(text),
                _ => continue,
            };
            let path = crate::godot::gdscript::quoted(&format!("{}/{key}", section.kind));
            let _ = writeln!(arms, "        {path} => {literal},");
        }
    }
    format!(
        "// Written by `balaur import` from project.godot: what `ProjectSettings`\n\
         // answered there.\n\npub fn setting(path) {{\n    match path {{\n{arms}        _ => (),\n    }}\n}}\n"
    )
}

/// The font file `gui/theme/custom_font` names, project-relative: the file
/// itself, or the `base_font` of a FontVariation saved as a `.tres`.
pub(crate) fn custom_font(
    document: &Document,
    uids: &BTreeMap<String, String>,
    root: &std::path::Path,
) -> Option<String> {
    let reference = document
        .first("gui")?
        .field("theme/custom_font")?
        .as_str()?;
    let path = resolve(reference, uids, &mut Vec::new());
    if !crate::godot::files::has_extension(&path, "tres") {
        return Some(path).filter(|p| !p.is_empty());
    }
    let text = crate::godot::io::text(&root.join(&path)).ok()?;
    let variation = crate::godot::parse(&text).ok()?;
    let id = variation
        .first("resource")?
        .field("base_font")?
        .call("ExtResource")?
        .first()?
        .as_str()?
        .to_string();
    let section = variation
        .each("ext_resource")
        .find(|s| s.attr_str("id") == Some(id.as_str()))?;
    let by_uid = section.attr_str("uid").and_then(|u| uids.get(u)).cloned();
    by_uid.or_else(|| {
        section
            .attr_str("path")
            .map(|p| crate::godot::relative_path(p).to_string())
    })
}

/// `res://a/b.tscn` and `uid://xyz` as the path a balaur project would use.
/// A scene keeps its stem and takes `.toml`; everything else keeps its name.
fn resolve(reference: &str, uids: &BTreeMap<String, String>, notes: &mut Vec<String>) -> String {
    let path = if let Some(rest) = reference.strip_prefix(crate::godot::RES) {
        rest.to_string()
    } else if reference.starts_with("uid://") {
        if let Some(path) = uids.get(reference) {
            path.clone()
        } else {
            notes.push(format!("`{reference}` names no file in this project"));
            return String::new();
        }
    } else {
        reference.to_string()
    };
    crate::godot::scene::scene_path(&path)
}

fn window(document: &Document, out: &mut String, notes: &mut Vec<String>) -> Result<()> {
    let Some(display) = document.first("display") else {
        return Ok(());
    };
    let number = |key: &str| display.field(key).and_then(Value::as_i64);
    let width = number("window/size/viewport_width");
    let height = number("window/size/viewport_height");
    // Godot's ScreenOrientation: the even ones are landscape, the odd ones
    // portrait, and 6 is the sensor deciding, which is balaur's `any`.
    let orientation = number("window/handheld/orientation").map(|mode| match mode {
        1 | 3 | 5 => "portrait",
        0 | 2 | 4 => "landscape",
        _ => "any",
    });
    // Godot's Window.Mode: 0 and 1 open a window (1 minimized, which balaur
    // does not start in), 2 maximized, 3 borderless fullscreen, 4 exclusive.
    let mode = number("window/size/mode").map(|mode| match mode {
        2 => "maximized",
        3 => "fullscreen",
        4 => "exclusive",
        _ => "windowed",
    });
    if width.is_none() && height.is_none() && orientation.is_none() && mode.is_none() {
        return Ok(());
    }
    writeln!(out, "\n[window]")?;
    if let Some(width) = width {
        writeln!(out, "width = {width}")?;
    }
    if let Some(height) = height {
        writeln!(out, "height = {height}")?;
    }
    if let Some(mode) = mode {
        writeln!(out, "mode = {}", quote(mode))?;
    }
    if let Some(orientation) = orientation {
        writeln!(out, "orientation = {}", quote(orientation))?;
    }
    if display.field("window/stretch/mode").is_some() {
        notes.push(
            "`window/stretch` has no equivalent: a balaur widget anchors and a camera zooms"
                .to_string(),
        );
    }
    Ok(())
}

/// `[locale]`, from the `.translation` files the project lists. Each is named
/// `<locale>.<locale>.translation`, so the locale is the file's first stem.
fn locale(document: &Document, out: &mut String) -> Result<()> {
    let Some(section) = document.first("internationalization") else {
        return Ok(());
    };
    let Some(list) = section.field("locale/translations") else {
        return Ok(());
    };
    let mut locales: Vec<String> = Vec::new();
    for item in list.numbers().map_or_else(
        || {
            list.call("PackedStringArray")
                .or_else(|| list.as_array())
                .unwrap_or_default()
        },
        |_| &[],
    ) {
        let Some(path) = item.as_str() else { continue };
        let file = path.rsplit('/').next().unwrap_or(path);
        if let Some(tag) = file.split('.').next().filter(|t| !t.is_empty())
            && !locales.iter().any(|l| l == tag)
        {
            locales.push(tag.to_string());
        }
    }
    if locales.is_empty() {
        return Ok(());
    }
    let default = if locales.iter().any(|l| l == "en") {
        "en"
    } else {
        &locales[0]
    };
    writeln!(out, "\n[locale]")?;
    writeln!(out, "default = {}", quote(default))?;
    writeln!(out, "fallback = {}", quote(default))?;
    Ok(())
}

/// `[input.actions]`, from the `Object(InputEvent…)` list each action holds.
fn actions(document: &Document, out: &mut String, notes: &mut Vec<String>) -> Result<()> {
    let Some(section) = document.first("input") else {
        return Ok(());
    };
    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for (action, value) in &section.fields {
        let Value::Dict(pairs) = value else { continue };
        let events = pairs
            .iter()
            .find(|(key, _)| key.as_str() == Some("events"))
            .and_then(|(_, v)| v.as_array())
            .unwrap_or_default();
        let mut bindings = Vec::new();
        for event in events {
            match binding(event) {
                Some(text) if !bindings.contains(&text) => bindings.push(text),
                Some(_) => {}
                None => notes.push(format!(
                    "action `{action}`: one event has no balaur binding and was dropped"
                )),
            }
        }
        if !bindings.is_empty() {
            rows.push((action.clone(), bindings));
        }
    }
    if rows.is_empty() {
        return Ok(());
    }
    writeln!(out, "\n[input.actions]")?;
    for (action, bindings) in rows {
        let list: Vec<String> = bindings.iter().map(|b| quote(b)).collect();
        writeln!(out, "{action} = [{}]", list.join(", "))?;
    }
    Ok(())
}

/// One `Object(InputEvent…)` as a balaur binding string.
fn binding(event: &Value) -> Option<String> {
    let Value::Object { class, .. } = event else {
        return None;
    };
    let int = |key: &str| event.field(key).and_then(Value::as_i64);
    match class.as_str() {
        "InputEventKey" => {
            // `physical_keycode` is the layout-independent one Godot prefers,
            // and is what balaur's names are: a key by where it sits.
            let code = int("physical_keycode")
                .filter(|c| *c != 0)
                .or_else(|| int("keycode"))?;
            keys::key_code_constant(code)
                .and_then(keys::key_value)
                .map(str::to_string)
        }
        "InputEventMouseButton" => {
            keys::mouse_button(int("button_index")?).map(|name| format!("mouse:{name}"))
        }
        "InputEventJoypadButton" => {
            keys::pad_button(int("button_index")?).map(|name| format!("gamepad:{name}"))
        }
        "InputEventJoypadMotion" => {
            let index = int("axis")?;
            let axis = keys::pad_axis(index)?;
            let flipped = keys::pad_axis_flipped(index);
            let half = event
                .field("axis_value")
                .and_then(Value::as_f64)
                .map_or("", |v| if (v < 0.0) == flipped { "+" } else { "-" });
            Some(format!("axis:{axis}{half}"))
        }
        _ => None,
    }
}

/// A TOML string. `toml` writes these itself, but the whole file here is one
/// string built in order, and a document rebuilt to write it would lose that.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Every `uid://…` a project's files declare, as the path that declared it.
///
/// A `.tscn` states its own in `[gd_scene uid=…]`, a `.tres` in
/// `[gd_resource uid=…]`, and a script's sits in the `.uid` file beside it.
pub(crate) fn uid_index(root: &std::path::Path) -> BTreeMap<String, String> {
    let mut index = BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for (name, is_dir) in crate::godot::io::list(&dir) {
            let path = dir.join(&name);
            if is_dir {
                // `.godot` is the editor's own cache and holds a copy of every
                // import, which would double the index and win the ties.
                if !name.starts_with('.') {
                    dirs.push(path);
                }
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            let Some(uid) = uid_of(&path) else {
                continue;
            };
            let owner = relative
                .strip_suffix(".uid")
                .or_else(|| relative.strip_suffix(".import"))
                .unwrap_or(&relative);
            index.insert(uid, owner.to_string());
        }
    }
    index
}

/// The `uid://…` one file declares, from its `.uid` body or its own header.
///
/// Only the first line is parsed: it is the whole header, and reading on
/// would find the uid of every `[ext_resource]` the file names instead.
fn uid_of(path: &std::path::Path) -> Option<String> {
    let extension = path.extension()?.to_str()?;
    if !matches!(extension, "uid" | "tscn" | "tres" | "import") {
        return None;
    }
    let text = crate::godot::io::text(path).ok()?;
    if extension == "uid" {
        let uid = text.trim();
        return uid.starts_with("uid://").then(|| uid.to_string());
    }
    // An `.import` states its file's uid as a field of `[remap]`, where a
    // scene and a resource state their own as an attribute of the header.
    if extension == "import" {
        let document = crate::godot::parse(&text).ok()?;
        let uid = document.first("remap")?.field("uid")?;
        return uid.as_str().map(std::string::ToString::to_string);
    }
    let header = crate::godot::parse(text.lines().next()?).ok()?;
    Some(header.sections.first()?.attr_str("uid")?.to_string())
}

#[cfg(test)]
mod tests {
    use super::{convert, shared_exclusions};
    use crate::godot::parse;
    use std::collections::BTreeMap;

    const PROJECT: &str = r#"config_version=5

[application]

config/name="Pirates"
run/main_scene="res://scenes/world.tscn"

[autoload]

ThemeEvents="*res://scripts/theme.gd"

[display]

window/size/viewport_width=840
window/size/viewport_height=1920
window/size/mode=2
window/handheld/orientation=6

[input]

jump={
"deadzone": 0.2,
"events": [Object(InputEventKey,"physical_keycode":32,"keycode":0,"script":null)
, Object(InputEventJoypadButton,"button_index":0,"script":null)
]
}
move_x={
"deadzone": 0.5,
"events": [Object(InputEventJoypadMotion,"axis":0,"axis_value":1.0,"script":null)
, Object(InputEventMouseButton,"button_index":1,"script":null)
]
}
move_up={
"deadzone": 0.5,
"events": [Object(InputEventJoypadMotion,"axis":1,"axis_value":-1.0,"script":null)
]
}

[internationalization]

locale/translations=PackedStringArray("res://lang/en.en.translation", "res://lang/ro.ro.translation")
"#;

    fn converted() -> super::Converted {
        let document = parse(PROJECT).expect("the project parses");
        convert(&document, &BTreeMap::new(), &[]).expect("it converts")
    }

    #[test]
    fn a_project_carries_its_name_scene_window_locale_and_actions() {
        let out = converted().project_toml;
        let doc: toml::Value = toml::from_str(&out).unwrap_or_else(|e| panic!("{e}\n{out}"));
        assert_eq!(doc["application"]["name"].as_str(), Some("Pirates"));
        assert_eq!(
            doc["application"]["main_scene"].as_str(),
            Some("scenes/world.toml"),
            "a `.tscn` reference becomes the `.toml` beside it"
        );
        assert_eq!(doc["window"]["width"].as_integer(), Some(840));
        assert_eq!(doc["locale"]["default"].as_str(), Some("en"));
        let actions = &doc["input"]["actions"];
        assert_eq!(
            actions["jump"].as_array().map(std::vec::Vec::len),
            Some(2),
            "{actions:?}"
        );
        assert_eq!(actions["jump"][0].as_str(), Some("Space"));
        assert_eq!(actions["jump"][1].as_str(), Some("gamepad:south"));
        assert_eq!(
            actions["move_x"][0].as_str(),
            Some("axis:left_x+"),
            "a joypad motion keeps the half its value names"
        );
        assert_eq!(actions["move_x"][1].as_str(), Some("mouse:left"));
        assert_eq!(
            actions["move_up"][0].as_str(),
            Some("axis:left_y+"),
            "Godot's stick up is -1 and balaur's is +1"
        );
    }

    /// Maximized is not fullscreen, and the sensor deciding is not portrait.
    /// Both were mapped wrong on the first pass and are the reason this test
    /// checks the modes rather than the width.
    #[test]
    fn a_maximized_sensor_window_is_neither_fullscreen_nor_portrait() {
        let out = converted();
        let doc: toml::Value = toml::from_str(&out.project_toml).unwrap();
        assert_eq!(doc["window"]["mode"].as_str(), Some("maximized"));
        assert_eq!(doc["window"]["orientation"].as_str(), Some("any"));
    }

    #[test]
    fn a_script_autoload_is_a_node_of_the_main_scene_and_needs_no_note() {
        let notes = converted().notes;
        assert!(
            !notes.iter().any(|n| n.contains("ThemeEvents")),
            "{notes:?}"
        );
    }

    /// Only what every preset leaves out is not the game's: a web build that
    /// drops its audio packs still ships them to a phone.
    #[test]
    fn what_every_export_preset_excludes_is_ignored_and_nothing_else() {
        let presets = parse(
            "[preset.0]\n\nname=\"Web\"\nexclude_filter=\"docs/*,packs/audio/*,data/countries/*.json,teaser.tscn\"\n\n\
             [preset.0.options]\n\nx=1\n\n\
             [preset.1]\n\nname=\"iOS\"\nexclude_filter=\"data/countries/*.json, docs/*,notes.txt,teaser.tscn\"\n",
        )
        .expect("the presets parse");
        let ignore = shared_exclusions(&presets);
        let document = parse(PROJECT).expect("the project parses");
        let out = convert(&document, &BTreeMap::new(), &ignore).expect("it converts");
        let doc: toml::Value = toml::from_str(&out.project_toml).expect("the project is TOML");
        let patterns: Vec<&str> = doc["application"]["ignore"]
            .as_array()
            .expect("an ignore list")
            .iter()
            .filter_map(toml::Value::as_str)
            .collect();
        assert!(balaur_core::ignore::ignored(
            &ignore,
            "data/countries/ro/borders.json"
        ));
        assert!(!balaur_core::ignore::ignored(
            &ignore,
            "packs/audio/sea.ogg"
        ));
        assert!(
            balaur_core::ignore::ignored(&ignore, "scenes/teaser.toml"),
            "a Godot scene's pattern names the scene the import wrote"
        );
        assert_eq!(
            patterns,
            ["docs/**", "data/countries/**.json", "**/teaser.toml"]
        );
    }
}
