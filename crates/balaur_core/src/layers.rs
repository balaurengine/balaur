//! The 32 layers a node is on, or a filter takes, as a `flags` property's
//! numbers. Layer 1 is the lowest bit, as Godot counts. A collider's
//! `collision_layer` and a mesh's `render_layers` are this one concept, read
//! and written here so the two cannot drift.

/// The bits the layer numbers under `value` set. Empty is every layer where
/// `empty_is_all`, which a filter means by it, and layer 1 otherwise, which
/// is where a membership starts: the alternative is 32 strings in every scene
/// file that wants the default.
#[must_use]
pub fn bits(value: Option<&toml::Value>, empty_is_all: bool) -> u32 {
    let mut bits = 0u32;
    for name in crate::components::as_flags(value) {
        if let Some(bit) = bit(name.parse::<u32>().ok()) {
            bits |= bit;
        }
    }
    if bits != 0 {
        bits
    } else if empty_is_all {
        u32::MAX
    } else {
        1
    }
}

/// A layer set as the numbers a `flags` property holds; every layer reads
/// back as the empty list, which is how a schema spells "everything".
#[must_use]
pub fn names(bits: u32) -> toml::Value {
    if bits == u32::MAX {
        return toml::Value::Array(Vec::new());
    }
    toml::Value::Array(
        (0..32)
            .filter(|bit| bits & (1 << bit) != 0)
            .map(|bit| toml::Value::String((bit + 1).to_string()))
            .collect(),
    )
}

/// The bit a layer number sets, for a number from 1 to 32.
#[must_use]
pub fn bit(layer: Option<u32>) -> Option<u32> {
    layer.filter(|l| (1..=32).contains(l)).map(|l| 1 << (l - 1))
}

/// The 32 layer numbers, as a schema's `options` list.
#[must_use]
pub fn options() -> String {
    (1..=32)
        .map(|i| format!("\"{i}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A `flags` row of layers, every one by default: what a filter or a node
/// drawn by every camera states.
#[must_use]
pub fn row_of_all(description: &str, group: &str) -> String {
    format!(
        r#"{{ type = "flags", default = [], options = [{}], description = "{description}; empty is every layer", group = "{group}" }}"#,
        options()
    )
}
