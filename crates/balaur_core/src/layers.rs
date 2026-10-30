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

/// The 32 layer numbers, as a layer property's options spell them.
const NUMBERS: [&str; 32] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
    "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32",
];

/// The 32 layer numbers, as a schema's `options` list.
#[must_use]
pub fn options() -> String {
    NUMBERS
        .iter()
        .map(|number| format!("\"{number}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One of the 32 layers, by its number: what a derived component's layers
/// row holds, as `#[property(flags)]` on a `Vec<Layer>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layer(pub u8);

impl Layer {
    /// The bits `layers` set. Empty is every layer where `empty_is_all`, as
    /// a filter means it, and layer 1 otherwise.
    #[must_use]
    pub fn bits(layers: &[Self], empty_is_all: bool) -> u32 {
        let bits = layers
            .iter()
            .fold(0u32, |bits, layer| bits | 1 << (layer.0 - 1));
        if bits != 0 {
            bits
        } else if empty_is_all {
            u32::MAX
        } else {
            1
        }
    }
}

impl crate::components::derived::Property for Layer {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        crate::components::derived::describe_enum(spec, &NUMBERS);
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::String(self.0.to_string())
    }

    fn from_value(value: &toml::Value) -> crate::components::derived::Result<Self> {
        value
            .as_str()
            .and_then(|text| NUMBERS.iter().position(|number| *number == text))
            .and_then(|at| u8::try_from(at + 1).ok())
            .map(Self)
            .ok_or_else(|| crate::components::derived::not_an_option(value, &NUMBERS))
    }
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
