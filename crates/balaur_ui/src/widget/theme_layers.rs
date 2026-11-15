//! The layers a theme table paints round its fill, read from the file: a
//! `gradient` in place of it, and `shadows` under and inside it.

use std::rc::Rc;

use egui::Color32;

use super::{Gradient, GradientShape, Shadow, Tokens, color};
use crate::vocabulary::looks as lk;

/// A table's `gradient`: its stops, two at least, and which way it runs.
pub(super) fn gradient_of(body: &toml::Table, tokens: &Tokens, what: &str) -> Option<Rc<Gradient>> {
    let table = body.get(lk::GRADIENT)?.as_table()?;
    let number = |value: &toml::Value| balaur_core::components::as_f64(value).map(|v| v as f32);
    let colours: Vec<Color32> = table
        .get(lk::STOPS)?
        .as_array()?
        .iter()
        .filter_map(|v| color(v, what, &tokens.colors))
        .collect();
    if colours.len() < 2 {
        return None;
    }
    let last = (colours.len() - 1) as f32;
    let places: Vec<f32> = table
        .get(lk::AT)
        .and_then(toml::Value::as_array)
        .filter(|at| at.len() == colours.len())
        .map(|at| at.iter().filter_map(number).collect())
        .filter(|at: &Vec<f32>| at.len() == colours.len())
        .unwrap_or_else(|| (0..colours.len()).map(|i| i as f32 / last).collect());
    let pair = |key: &str, default: [f32; 2]| {
        table
            .get(key)
            .and_then(toml::Value::as_array)
            .and_then(|xy| Some([number(xy.first()?)?, number(xy.get(1)?)?]))
            .unwrap_or(default)
    };
    let shape = if table.get(lk::KIND).and_then(toml::Value::as_str) == Some(lk::RADIAL) {
        GradientShape::Radial {
            center: pair(lk::CENTER, [0.5, 0.5]),
            radius: table.get(lk::RADIUS).and_then(number).unwrap_or(0.5),
        }
    } else {
        GradientShape::Linear {
            angle: table.get(lk::ANGLE).and_then(number).unwrap_or(0.0),
        }
    };
    Some(Rc::new(Gradient {
        shape,
        stops: places.into_iter().zip(colours).collect(),
    }))
}

/// A table's `shadows`, each its colour, and offsets,
/// blur and spread, 0 when left out. An empty list is no shadow, which a
/// state table states to take its entry's away.
pub(super) fn shadows_of(body: &toml::Table, tokens: &Tokens, what: &str) -> Option<Rc<[Shadow]>> {
    let one = |table: &toml::Table| {
        let number = |key: &str| {
            table
                .get(key)
                .and_then(balaur_core::components::as_f64)
                .map_or(0.0, |v| v as f32)
        };
        Some(Shadow {
            x: number(lk::X),
            y: number(lk::Y),
            blur: number(lk::BLUR).max(0.0),
            spread: number(lk::SPREAD),
            color: table
                .get(lk::COLOR)
                .and_then(|v| color(v, what, &tokens.colors))?,
            inset: table
                .get(lk::INSET)
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        })
    };
    let value = body.get(lk::SHADOWS)?;
    let Some(items) = value.as_array() else {
        tracing::warn!(
            "widget theme: {what}'s `shadows` is a list, `shadows = [{{ y = 2, blur = 8, color = \"#00000040\" }}]`"
        );
        return None;
    };
    Some(
        items
            .iter()
            .filter_map(toml::Value::as_table)
            .filter_map(one)
            .collect::<Vec<_>>()
            .into(),
    )
}
