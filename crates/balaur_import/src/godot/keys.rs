//! Godot's input names as balaur's: its `Key` codes and `KEY_*` constants,
//! its joypad buttons and axes, and its mouse buttons.

use balaur::input::{EITHER_SIDE, KEYS, PAD_AXIS_NAMES, PAD_BUTTON_NAMES};

/// Godot's `KEY_SPECIAL`: every key that types no character sits above it.
const SPECIAL: i64 = 1 << 22;

/// The keys whose Godot name or code is not the balaur one spelled the same
/// way: `KEY_<name>`, its `Key` code, and the balaur constant.
const NAMED: &[(&str, i64, &str)] = &[
    ("SPACE", 32, "KEY_SPACE"),
    ("APOSTROPHE", 39, "KEY_APOSTROPHE"),
    ("COMMA", 44, "KEY_COMMA"),
    ("MINUS", 45, "KEY_MINUS"),
    ("PERIOD", 46, "KEY_PERIOD"),
    ("SLASH", 47, "KEY_SLASH"),
    ("SEMICOLON", 59, "KEY_SEMICOLON"),
    ("EQUAL", 61, "KEY_EQUAL"),
    ("BRACKETLEFT", 91, "KEY_LEFT_BRACKET"),
    ("BACKSLASH", 92, "KEY_BACKSLASH"),
    ("BRACKETRIGHT", 93, "KEY_RIGHT_BRACKET"),
    ("QUOTELEFT", 96, "KEY_BACKQUOTE"),
    ("ESCAPE", SPECIAL | 1, "KEY_ESCAPE"),
    ("TAB", SPECIAL | 2, "KEY_TAB"),
    ("BACKSPACE", SPECIAL | 4, "KEY_BACKSPACE"),
    ("ENTER", SPECIAL | 5, "KEY_ENTER"),
    ("KP_ENTER", SPECIAL | 6, "KEY_NUMPAD_ENTER"),
    ("INSERT", SPECIAL | 7, "KEY_INSERT"),
    ("DELETE", SPECIAL | 8, "KEY_DELETE"),
    ("PAUSE", SPECIAL | 9, "KEY_PAUSE"),
    ("PRINT", SPECIAL | 10, "KEY_PRINT_SCREEN"),
    ("HOME", SPECIAL | 13, "KEY_HOME"),
    ("END", SPECIAL | 14, "KEY_END"),
    ("LEFT", SPECIAL | 15, "KEY_LEFT"),
    ("UP", SPECIAL | 16, "KEY_UP"),
    ("RIGHT", SPECIAL | 17, "KEY_RIGHT"),
    ("DOWN", SPECIAL | 18, "KEY_DOWN"),
    ("PAGEUP", SPECIAL | 19, "KEY_PAGE_UP"),
    ("PAGEDOWN", SPECIAL | 20, "KEY_PAGE_DOWN"),
    ("SHIFT", SPECIAL | 21, "KEY_SHIFT"),
    ("CTRL", SPECIAL | 22, "KEY_CONTROL"),
    ("META", SPECIAL | 23, "KEY_META"),
    ("ALT", SPECIAL | 24, "KEY_ALT"),
    ("CAPSLOCK", SPECIAL | 25, "KEY_CAPS_LOCK"),
    ("NUMLOCK", SPECIAL | 26, "KEY_NUM_LOCK"),
    ("SCROLLLOCK", SPECIAL | 27, "KEY_SCROLL_LOCK"),
    ("MENU", SPECIAL | 66, "KEY_CONTEXT_MENU"),
    ("KP_MULTIPLY", SPECIAL | 129, "KEY_NUMPAD_MULTIPLY"),
    ("KP_DIVIDE", SPECIAL | 130, "KEY_NUMPAD_DIVIDE"),
    ("KP_SUBTRACT", SPECIAL | 131, "KEY_NUMPAD_SUBTRACT"),
    ("KP_PERIOD", SPECIAL | 132, "KEY_NUMPAD_PERIOD"),
    ("KP_ADD", SPECIAL | 133, "KEY_NUMPAD_ADD"),
];

/// Godot's `F1`: the function keys run on from it.
const F1: i64 = SPECIAL | 28;
/// Godot's `KP_0`: the keypad digits run on from it.
const KP_0: i64 = SPECIAL | 134;

/// A balaur key constant by name, as the static the engine keeps.
fn constant(name: &str) -> Option<&'static str> {
    KEYS.iter()
        .map(|(constant, _)| *constant)
        .chain(EITHER_SIDE.iter().map(|(constant, _, _)| *constant))
        .find(|constant| *constant == name)
}

/// The balaur constant for Godot's `KEY_<godot>`.
pub(crate) fn key_constant(godot: &str) -> Option<&'static str> {
    let single = godot.len() == 1 && godot.chars().all(|c| c.is_ascii_alphanumeric());
    if single || function_key(godot) {
        return constant(&format!("KEY_{godot}"));
    }
    if let Some(digit) = godot.strip_prefix("KP_").filter(|d| d.len() == 1) {
        return constant(&format!("KEY_NUMPAD_{digit}"));
    }
    NAMED
        .iter()
        .find(|(name, _, _)| *name == godot)
        .and_then(|(_, _, balaur)| constant(balaur))
}

fn function_key(godot: &str) -> bool {
    godot
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| n >= 1)
}

/// The balaur constant for a Godot `Key` code, as `project.godot` stores it.
pub(crate) fn key_code_constant(code: i64) -> Option<&'static str> {
    match code {
        48..=57 | 65..=90 => constant(&format!("KEY_{}", char::from(code as u8))),
        c if (F1..F1 + 24).contains(&c) => constant(&format!("KEY_F{}", c - F1 + 1)),
        c if (KP_0..KP_0 + 10).contains(&c) => constant(&format!("KEY_NUMPAD_{}", c - KP_0)),
        _ => NAMED
            .iter()
            .find(|(_, godot, _)| *godot == code)
            .and_then(|(_, _, balaur)| constant(balaur)),
    }
}

/// A balaur key constant's value: the key code a binding and `key_down` take.
pub(crate) fn key_value(constant: &str) -> Option<&'static str> {
    KEYS.iter()
        .find(|(name, _)| *name == constant)
        .map(|(_, value)| *value)
        .or_else(|| {
            EITHER_SIDE
                .iter()
                .find(|(name, _, _)| *name == constant)
                .map(|(_, value, _)| *value)
        })
}

/// Godot's `JoyButton` as balaur's gamepad button. Godot numbers the face
/// buttons by position, and its `X` is the west one.
pub(crate) fn pad_button(index: i64) -> Option<&'static str> {
    let name = match index {
        0 => "south",
        1 => "east",
        2 => "west",
        3 => "north",
        4 => "back",
        5 => "guide",
        6 => "start",
        7 => "left_stick",
        8 => "right_stick",
        9 => "left_shoulder",
        10 => "right_shoulder",
        11 => "dpad_up",
        12 => "dpad_down",
        13 => "dpad_left",
        14 => "dpad_right",
        _ => return None,
    };
    PAD_BUTTON_NAMES.iter().copied().find(|known| *known == name)
}

/// Godot's `JoyAxis` as balaur's gamepad axis.
pub(crate) fn pad_axis(index: i64) -> Option<&'static str> {
    let name = match index {
        0 => "left_x",
        1 => "left_y",
        2 => "right_x",
        3 => "right_y",
        4 => "left_trigger",
        5 => "right_trigger",
        _ => return None,
    };
    PAD_AXIS_NAMES.iter().copied().find(|known| *known == name)
}

/// Godot's stick y axes read down as +1; balaur's read up as +1.
pub(crate) fn pad_axis_flipped(index: i64) -> bool {
    matches!(index, 1 | 3)
}

/// Godot's `MouseButton` as the word a `mouse:` binding takes.
pub(crate) fn mouse_button(index: i64) -> Option<&'static str> {
    Some(match index {
        1 => "left",
        2 => "right",
        3 => "middle",
        8 => "back",
        9 => "forward",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_godot_key_is_a_balaur_key() {
        for (godot, code, balaur) in NAMED {
            assert_eq!(key_constant(godot), Some(*balaur), "KEY_{godot}");
            assert_eq!(key_code_constant(*code), Some(*balaur), "code of KEY_{godot}");
        }
    }

    #[test]
    fn letters_digits_function_and_keypad_keys_are_their_codes() {
        assert_eq!(key_code_constant(65).and_then(key_value), Some("KeyA"));
        assert_eq!(key_code_constant(49).and_then(key_value), Some("Digit1"));
        assert_eq!(key_code_constant(F1 + 2).and_then(key_value), Some("F3"));
        assert_eq!(key_code_constant(KP_0 + 5).and_then(key_value), Some("Numpad5"));
        assert_eq!(key_constant("KP_5"), Some("KEY_NUMPAD_5"));
        assert_eq!(key_constant("META").and_then(key_value), Some("Meta"));
        assert_eq!(key_constant("PREFIX"), None, "a script's own KEY_ constant is not a key");
    }

    #[test]
    fn the_tables_agree_with_godots_numbering() {
        let code = |godot: i64| key_code_constant(godot).and_then(key_value);
        assert_eq!(code(32), Some("Space"));
        assert_eq!(code(87), Some("KeyW"));
        assert_eq!(code(48), Some("Digit0"));
        assert_eq!(code(4_194_319), Some("ArrowLeft"));
        assert_eq!(code(4_194_322), Some("ArrowDown"));
        assert_eq!(code(4_194_332), Some("F1"));
        assert_eq!(code(1), None);
        // Godot numbers the face buttons by position: its `X` is the west one.
        assert_eq!(pad_button(2), Some("west"));
        assert_eq!(pad_button(3), Some("north"));
        assert_eq!(pad_button(13), Some("dpad_left"));
    }

    #[test]
    fn every_pad_index_godot_has_is_a_balaur_name() {
        assert!((0..15).all(|i| pad_button(i).is_some()));
        assert!((0..6).all(|i| pad_axis(i).is_some()));
    }
}
