//! `project.godot` as a `project.toml`.
//!
//! What carries across is what both engines have: the name, the main scene,
//! the window, the locales and the input map. An autoload has no equivalent
//! here and is reported rather than invented; so is every key neither engine
//! shares.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use anyhow::{Context, Result};

use crate::godot::{Document, Value};

/// A converted project, and what would not convert.
pub(crate) struct Converted {
    pub project_toml: String,
    /// One line per thing that did not carry, for the import report.
    pub notes: Vec<String>,
}

/// Read a `project.godot` and lay out the `project.toml` beside it.
///
/// `uids` resolves `uid://` references to project-relative paths; an empty
/// map is fine and leaves them reported instead.
pub(crate) fn convert(document: &Document, uids: &BTreeMap<String, String>) -> Result<Converted> {
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
    // Godot readies a child before its parent, and its scripts count on it.
    writeln!(out, "init_order = \"children_first\"")?;
    if let Some(splash) = get("application", "boot_splash/image")
        .as_ref()
        .and_then(Value::as_str)
    {
        let path = resolve(splash, uids, &mut notes);
        if !path.is_empty() {
            writeln!(out, "splash = {}", quote(&path))?;
        }
    }

    window(document, &mut out, &mut notes)?;
    if let Some(theme) = get("gui", "theme/custom").as_ref().and_then(Value::as_str) {
        let path = resolve(theme, uids, &mut notes);
        if let Some(godot) = path.strip_suffix(".tres") {
            writeln!(out, "\n[ui]")?;
            writeln!(
                out,
                "theme = {}",
                quote(&crate::godot::theme::theme_path(&format!(
                    "{godot}.tres"
                )))
            )?;
        }
    }
    locale(document, &mut out)?;
    actions(document, &mut out, &mut notes)?;

    for section in document.each("autoload") {
        for (key, _) in &section.fields {
            notes.push(format!(
                "autoload `{key}`: balaur has no autoload; give the script to a node in a scene loaded first"
            ));
        }
    }
    Ok(Converted {
        project_toml: out,
        notes,
    })
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
    let text = std::fs::read_to_string(root.join(&path)).ok()?;
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
            .map(|p| p.strip_prefix("res://").unwrap_or(p).to_string())
    })
}

/// `res://a/b.tscn` and `uid://xyz` as the path a balaur project would use.
/// A scene keeps its stem and takes `.toml`; everything else keeps its name.
fn resolve(reference: &str, uids: &BTreeMap<String, String>, notes: &mut Vec<String>) -> String {
    let path = if let Some(rest) = reference.strip_prefix("res://") {
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
    match path.strip_suffix(".tscn") {
        Some(stem) => format!("{stem}.toml"),
        None => path,
    }
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
            key_name(code).map(std::string::ToString::to_string)
        }
        "InputEventMouseButton" => match int("button_index")? {
            1 => Some("mouse:left".to_string()),
            2 => Some("mouse:right".to_string()),
            3 => Some("mouse:middle".to_string()),
            _ => None,
        },
        "InputEventJoypadButton" => {
            pad_button(int("button_index")?).map(|name| format!("gamepad:{name}"))
        }
        "InputEventJoypadMotion" => {
            let axis = pad_axis(int("axis")?)?;
            let half = event
                .field("axis_value")
                .and_then(Value::as_f64)
                .map_or("", |v| if v < 0.0 { "-" } else { "+" });
            Some(format!("axis:{axis}{half}"))
        }
        _ => None,
    }
}

/// Godot's `Key` as balaur's key name. `SPECIAL` is `1 << 22`, which is why
/// the arrows and the function keys sit so far above the printable range.
fn key_name(code: i64) -> Option<&'static str> {
    const SPECIAL: i64 = 1 << 22;
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = [
        "Key0", "Key1", "Key2", "Key3", "Key4", "Key5", "Key6", "Key7", "Key8", "Key9",
    ];
    const FUNCTION: [&str; 12] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    ];
    match code {
        32 => Some("Space"),
        48..=57 => DIGITS.get((code - 48) as usize).copied(),
        65..=90 => LETTERS.get((code - 65) as usize).copied(),
        39 => Some("Apostrophe"),
        44 => Some("Comma"),
        45 => Some("Minus"),
        46 => Some("Period"),
        47 => Some("Slash"),
        59 => Some("Semicolon"),
        61 => Some("Equals"),
        91 => Some("LBracket"),
        92 => Some("Backslash"),
        93 => Some("RBracket"),
        96 => Some("Grave"),
        _ => match code - SPECIAL {
            1 => Some("Escape"),
            2 => Some("Tab"),
            4 => Some("Back"),
            5 | 6 => Some("Return"),
            7 => Some("Insert"),
            8 => Some("Delete"),
            13 => Some("Home"),
            14 => Some("End"),
            15 => Some("Left"),
            16 => Some("Up"),
            17 => Some("Right"),
            18 => Some("Down"),
            19 => Some("PageUp"),
            20 => Some("PageDown"),
            21 => Some("LShift"),
            22 => Some("LControl"),
            24 => Some("LAlt"),
            n @ 28..=39 => FUNCTION.get((n - 28) as usize).copied(),
            _ => None,
        },
    }
}

/// Godot's `JoyButton` as balaur's pad name. Godot numbers the face buttons
/// by position, and `X` is the west one.
fn pad_button(index: i64) -> Option<&'static str> {
    match index {
        0 => Some("South"),
        1 => Some("East"),
        2 => Some("West"),
        3 => Some("North"),
        4 => Some("Select"),
        5 => Some("Mode"),
        6 => Some("Start"),
        7 => Some("LeftThumb"),
        8 => Some("RightThumb"),
        9 => Some("LeftTrigger"),
        10 => Some("RightTrigger"),
        11 => Some("DPadUp"),
        12 => Some("DPadDown"),
        13 => Some("DPadLeft"),
        14 => Some("DPadRight"),
        _ => None,
    }
}

/// Godot's `JoyAxis` as balaur's axis name.
fn pad_axis(index: i64) -> Option<&'static str> {
    match index {
        0 => Some("LeftStickX"),
        1 => Some("LeftStickY"),
        2 => Some("RightStickX"),
        3 => Some("RightStickY"),
        4 => Some("LeftZ"),
        5 => Some("RightZ"),
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
pub(crate) fn uid_index(root: &std::path::Path) -> Result<BTreeMap<String, String>> {
    let mut index = BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let entries = std::fs::read_dir(&dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .flatten();
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
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
    Ok(index)
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
    let text = std::fs::read_to_string(path).ok()?;
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
    use super::{convert, key_name, pad_button};
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

[internationalization]

locale/translations=PackedStringArray("res://lang/en.en.translation", "res://lang/ro.ro.translation")
"#;

    fn converted() -> super::Converted {
        let document = parse(PROJECT).expect("the project parses");
        convert(&document, &BTreeMap::new()).expect("it converts")
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
        assert_eq!(actions["jump"][1].as_str(), Some("gamepad:South"));
        assert_eq!(
            actions["move_x"][0].as_str(),
            Some("axis:LeftStickX+"),
            "a joypad motion keeps the half its value names"
        );
        assert_eq!(actions["move_x"][1].as_str(), Some("mouse:left"));
        assert_eq!(
            doc["application"]["init_order"].as_str(),
            Some("children_first"),
            "scripts init in Godot's _ready order"
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
    fn an_autoload_is_reported_rather_than_invented() {
        let notes = converted().notes;
        assert!(notes.iter().any(|n| n.contains("ThemeEvents")), "{notes:?}");
    }

    #[test]
    fn the_key_and_pad_tables_agree_with_godots_numbering() {
        assert_eq!(key_name(32), Some("Space"));
        assert_eq!(key_name(87), Some("W"));
        assert_eq!(key_name(48), Some("Key0"));
        assert_eq!(key_name(4_194_319), Some("Left"));
        assert_eq!(key_name(4_194_322), Some("Down"));
        assert_eq!(key_name(4_194_332), Some("F1"));
        assert_eq!(key_name(1), None);
        // Godot numbers the face buttons by position, so its `X` is the west
        // one and its `Y` the north one.
        assert_eq!(pad_button(2), Some("West"));
        assert_eq!(pad_button(3), Some("North"));
        assert_eq!(pad_button(13), Some("DPadLeft"));
    }
}
