//! A layer a node is drawn or lit on, or a camera, light or probe reaches,
//! as a `flags` property holds it: one of the 32 numbers
//! `balaur_core::layers` counts, 1 the lowest bit.

use balaur_core::components::derived::{self, Property, Result};

/// The 32 layer numbers, as a layer property's options.
const LAYERS: [&str; 32] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
    "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32",
];

/// One of the 32 layers, by its number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layer(pub u8);

impl Layer {
    /// The bits `layers` set; none is every layer, which is what a node drawn
    /// by every camera and a camera that draws every node both mean by it.
    #[must_use]
    pub fn mask(layers: &[Self]) -> u32 {
        let bits = layers
            .iter()
            .fold(0u32, |bits, layer| bits | 1 << (layer.0 - 1));
        if bits == 0 { u32::MAX } else { bits }
    }
}

impl Property for Layer {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        derived::describe_enum(spec, &LAYERS);
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::String(self.0.to_string())
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value
            .as_str()
            .and_then(|text| LAYERS.iter().position(|name| *name == text))
            .and_then(|at| u8::try_from(at + 1).ok())
            .map(Self)
            .ok_or_else(|| derived::not_an_option(value, &LAYERS))
    }
}
