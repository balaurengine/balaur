//! The rows an `inspector` fills: one a property, by the datatype its spec
//! names, each control paired with what an edit on it means.

use balaur_core::Engine;
use balaur_script::Value;
use smol_str::SmolStr;

use super::{Asked, Cursor, Heard, Means, Step};
use crate::vocabulary::{inspect as i, keys as k, pool as p, words as w};

/// A row's height where its property says nothing, which is the pool's.
const ROW_H: f64 = 24.0;
/// How much shorter than its row a chip is.
const CHIP_INSET: f64 = 4.0;
/// A chip that adds or takes away an entry.
const CHIP_W: i64 = 22;
/// A colour's swatch, beside the channels written out.
const SWATCH_W: i64 = 34;
/// The words a row label writes in capitals.
const ACRONYMS: &[&str] = &[
    "fov", "hdr", "id", "ik", "lod", "rgb", "rgba", "srgb", "ui", "uv",
];
const ADD: &str = "+";
const REMOVE: &str = "\u{00d7}";
const NONE: &str = "\u{2014} none \u{2014}";
const EMPTY: &str = "\u{2014} empty \u{2014}";
const EMBEDDED: &str = "\u{2014} embedded \u{2014}";
/// What a reference to an inline definition starts with.
const INLINE: &str = "#!";

/// One row of the form: its spec as the pool takes it, and what an edit on
/// each of its controls means.
pub(super) struct Row {
    pub(super) spec: Value,
    pub(super) heard: Vec<Option<Heard>>,
}

/// One control and what an edit on it means.
type Control = (Value, Option<(Cursor, Means)>);

/// The rows one inspector is building.
pub(super) struct Form<'a> {
    eng: &'a Engine,
    asked: &'a Asked,
    host: u64,
    pub(super) rows: Vec<Row>,
    /// A control the next row carries after its own: the chip that takes an
    /// entry out of its list, on the first row the entry draws.
    after: Option<Control>,
}

impl<'a> Form<'a> {
    pub(super) fn new(eng: &'a Engine, asked: &'a Asked, host: u64) -> Self {
        Self {
            eng,
            asked,
            host,
            rows: Vec::new(),
            after: None,
        }
    }

    /// Every property `options` names, or every one the schema declares.
    pub(super) fn component(
        &mut self,
        schema: &toml::Value,
        data: &toml::Value,
        options: &[SmolStr],
    ) {
        let Some(specs) = schema.as_table() else {
            return;
        };
        let names: Vec<String> = if options.is_empty() {
            sorted_keys(specs)
        } else {
            options.iter().map(ToString::to_string).collect()
        };
        for name in names {
            let Some(spec) = specs.get(&name) else {
                continue;
            };
            let value = data
                .get(&name)
                .or_else(|| spec.get(i::DEFAULT))
                .cloned()
                .unwrap_or_else(|| toml::Value::String(String::new()));
            if flag(spec, i::READONLY) {
                self.readonly(&name, spec, &value);
            } else {
                self.value(&Cursor::top(&name), spec, &value);
            }
        }
    }

    fn push(&mut self, entries: Vec<(&str, Value)>, controls: Vec<Control>) {
        let mut controls = controls;
        if let Some(after) = self.after.take() {
            controls.push(after);
        }
        let mut heard = Vec::with_capacity(controls.len());
        let mut specs = Vec::with_capacity(controls.len());
        for (spec, means) in controls {
            specs.push(spec);
            heard.push(means.map(|(at, means)| Heard {
                host: self.host,
                at,
                means,
            }));
        }
        let mut entries: Vec<(String, Value)> = entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect();
        entries.push((p::CONTROLS.into(), Value::List(specs)));
        self.rows.push(Row {
            spec: Value::Map(entries),
            heard,
        });
    }

    /// A row labelled for the cursor, with its tooltip.
    fn labelled(&mut self, at: &Cursor, spec: &toml::Value, controls: Vec<Control>) {
        self.push(
            vec![
                (p::LABEL, Value::text(&at.label)),
                (k::TOOLTIP, Value::text(tip(&at.label, spec))),
            ],
            controls,
        );
    }

    /// A row as tall as a control the pool sizes by hand, whose controls sit
    /// apart as a row of verbs does.
    fn tall(&mut self, at: &Cursor, spec: &toml::Value, controls: Vec<Control>) {
        self.push(
            vec![
                (p::LABEL, Value::text(&at.label)),
                (k::TOOLTIP, Value::text(tip(&at.label, spec))),
                (k::HEIGHT, Value::Num(self.row_h())),
                (p::SLOT_ROLE, Value::text(i::LAYOUT_ACTIONS)),
            ],
            controls,
        );
    }

    fn row_h(&self) -> f64 {
        if self.asked.row_height > 0.0 {
            f64::from(self.asked.row_height)
        } else {
            ROW_H
        }
    }

    fn chip_h(&self) -> f64 {
        self.row_h() - CHIP_INSET
    }

    /// One value at a cursor, drawn by what its spec says it is. A list, a
    /// map and a record call back into this for what they hold.
    fn value(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value) {
        match text_of(spec, i::TYPE) {
            i::FLOAT => self.number(at, spec, value, false),
            i::INT => self.number(at, spec, value, true),
            i::BOOL => self.labelled(
                at,
                spec,
                vec![(
                    control(&[
                        (k::KIND, Value::text(w::CHECKBOX)),
                        (k::TEXT, Value::text("")),
                        (k::CHECKED, Value::Bool(value.as_bool() == Some(true))),
                    ]),
                    Some((at.clone(), Means::Bool)),
                )],
            ),
            i::ENUM => self.labelled(
                at,
                spec,
                vec![(
                    control(&[
                        (k::KIND, Value::text(w::DROPDOWN)),
                        (k::GROW, Value::Int(1)),
                        (k::TEXT, Value::text(as_text(value))),
                        (k::OPTIONS, strings_of(spec.get(i::OPTIONS))),
                    ]),
                    Some((at.clone(), Means::Choice)),
                )],
            ),
            i::VEC2 => self.vector(at, spec, value, 2),
            i::VEC3 => self.vector(at, spec, value, 3),
            i::VEC4 => self.vector(at, spec, value, 4),
            i::COLOR => self.color(at, spec, value),
            i::ASSET => self.asset(at, spec, value),
            i::FLAGS => self.flags(at, spec, value),
            i::LIST => self.list(at, spec, value),
            i::MAP => self.map(at, spec, value),
            i::RECORD => self.record(at, spec, value),
            _ => self.text(at, spec, value),
        }
    }

    fn number(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value, int: bool) {
        let degrees = !int && text_of(spec, i::UNIT) == i::DEGREES;
        let held = balaur_core::components::as_f64(value).unwrap_or(0.0);
        let shown = if degrees { held.to_degrees() } else { held };
        let speed = number_of(spec, i::STEP).unwrap_or(if int { 1.0 } else { 0.02 });
        let means = Means::Number {
            degrees,
            int,
            min: number_of(spec, i::MIN),
            max: number_of(spec, i::MAX),
        };
        self.labelled(
            at,
            spec,
            vec![(
                control(&[
                    (k::KIND, Value::text(w::NUMBER_FIELD)),
                    (k::GROW, Value::Int(1)),
                    (k::VALUE, Value::Num(shown)),
                    (k::DRAG_SPEED, Value::Num(speed)),
                ]),
                Some((at.clone(), means)),
            )],
        );
    }

    /// A vector: one number a channel, lettered.
    fn vector(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value, count: usize) {
        let degrees = text_of(spec, i::UNIT) == i::DEGREES;
        let held = channels(value, count, 0.0);
        let speed = number_of(spec, i::STEP).unwrap_or(0.02);
        let controls = (0..count)
            .map(|slot| {
                let shown = if degrees {
                    held[slot].to_degrees()
                } else {
                    held[slot]
                };
                (
                    control(&[
                        (k::KIND, Value::text(w::NUMBER_FIELD)),
                        (k::GROW, Value::Int(1)),
                        (k::VALUE, Value::Num(shown)),
                        (k::DRAG_SPEED, Value::Num(speed)),
                        (k::PREFIX, Value::text(["x", "y", "z", "w"][slot])),
                    ]),
                    Some((
                        at.clone(),
                        Means::Channel {
                            slot,
                            count,
                            degrees,
                        },
                    )),
                )
            })
            .collect();
        self.labelled(at, spec, controls);
    }

    /// A swatch that opens the picker, and the channels written out beside
    /// it: a colour is the one vector nobody reads as numbers.
    fn color(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value) {
        let held = channels(value, 4, 1.0);
        let text = held
            .iter()
            .map(|c| format!("{c:.2}"))
            .collect::<Vec<_>>()
            .join(" ");
        let picked = match value.as_array() {
            Some(items) => Value::List(
                items
                    .iter()
                    .map(|c| Value::Num(balaur_core::components::as_f64(c).unwrap_or(0.0)))
                    .collect(),
            ),
            None => Value::List(Vec::new()),
        };
        self.labelled(
            at,
            spec,
            vec![
                (
                    control(&[
                        (k::KIND, Value::text(w::COLOR_PICKER)),
                        (k::WIDTH, Value::Int(SWATCH_W)),
                        (k::PICKED_COLOR, picked),
                    ]),
                    Some((at.clone(), Means::Color)),
                ),
                (
                    control(&[
                        (k::KIND, Value::text(w::LABEL)),
                        (k::GROW, Value::Int(1)),
                        (k::TEXT, Value::text(text)),
                        (k::ROLE, Value::text(i::TEXT_CAPTION)),
                    ]),
                    None,
                ),
            ],
        );
    }

    /// A set drawn from the schema's `options`: one pill an option, filled
    /// where it is held.
    fn flags(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value) {
        let held = texts(value);
        let controls = texts(
            spec.get(i::OPTIONS)
                .unwrap_or(&toml::Value::Array(Vec::new())),
        )
        .into_iter()
        .map(|name| {
            let on = held.contains(&name);
            (
                control(&[
                    (k::KIND, Value::text(w::BUTTON)),
                    (k::TEXT, Value::text(&name)),
                    (k::ROLE, Value::text(i::CHIP_FLAG)),
                    (k::CHECKED, Value::Bool(on)),
                ]),
                Some((at.clone(), Means::Flag(name))),
            )
        })
        .collect();
        self.labelled(at, spec, controls);
    }

    /// A reference, or an inline definition, with the verb that turns one
    /// into the other where the asset type keeps its files somewhere.
    fn asset(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value) {
        let asset = text_of(spec, i::ASSET).to_owned();
        let reference = self.shown(&as_text(value));
        let inline = reference.starts_with(INLINE);
        let dir = if asset.is_empty() {
            String::new()
        } else {
            balaur_core::assets::directory(self.eng, &asset)
        };
        let mut controls = vec![if inline {
            (
                control(&[
                    (k::KIND, Value::text(w::LABEL)),
                    (k::ROLE, Value::text(i::TEXT_CODE_QUIET)),
                    (k::GROW, Value::Int(1)),
                    (k::TEXT, Value::text(EMBEDDED)),
                ]),
                None,
            )
        } else {
            (
                control(&[
                    (k::KIND, Value::text(w::TEXT_FIELD)),
                    (k::GROW, Value::Int(1)),
                    (k::TEXT, Value::text(&reference)),
                    (k::PLACEHOLDER, Value::text(NONE)),
                ]),
                Some((at.clone(), Means::Text { submit: true })),
            )
        }];
        if !dir.is_empty() {
            let (text, tooltip, verb) = if inline {
                (
                    "save to file",
                    format!("Write this {asset} to {dir}/ and reference it"),
                    i::SAVE,
                )
            } else {
                (
                    "embed",
                    "Copy this file's definition onto the node".to_owned(),
                    i::EMBED,
                )
            };
            controls.push((
                control(&[
                    (k::KIND, Value::text(w::BUTTON)),
                    (k::ROLE, Value::text(i::ACTION_DOCK_SECONDARY)),
                    (k::TEXT, Value::text(text)),
                    (k::TOOLTIP, Value::text(tooltip)),
                ]),
                Some((
                    at.clone(),
                    Means::Verb {
                        verb,
                        reference,
                        asset,
                    },
                )),
            ));
        }
        self.tall(at, spec, controls);
    }

    /// Anything with no editor of its own: its text, written as it is typed.
    fn text(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value) {
        let shown = self.shown(&as_text(value));
        self.labelled(
            at,
            spec,
            vec![(
                control(&[
                    (k::KIND, Value::text(w::TEXT_FIELD)),
                    (k::GROW, Value::Int(1)),
                    (k::ROLE, Value::text(i::INPUT)),
                    (k::TEXT, Value::text(shown)),
                    (k::PLACEHOLDER, Value::text(EMPTY)),
                ]),
                Some((at.clone(), Means::Text { submit: false })),
            )],
        );
    }

    /// A list: what it holds and how many, then a row an entry, each drawn
    /// by the editor its `of` asks for and each removable.
    fn list(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value) {
        let of = spec.get(i::OF).cloned().unwrap_or_else(empty_table);
        let held = value.as_array().cloned().unwrap_or_default();
        let note = format!("{} \u{b7} {}", type_of(&of), held.len());
        let fresh = fresh_of(&of);
        self.holder(at, spec, note, Some(Means::Add { fresh, key: None }));
        for (n, entry) in held.iter().enumerate() {
            let inner = at.step(Step::Index(n), n.to_string());
            self.removable(at, Step::Index(n));
            self.value(&inner, &of, entry);
            self.after = None;
        }
    }

    /// A map: the same, with each row labelled by its key.
    fn map(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value) {
        let of = spec.get(i::OF).cloned().unwrap_or_else(empty_table);
        let held = value.as_table().cloned().unwrap_or_default();
        let numbered = text_of(spec, i::KEY) == i::INT;
        let note = format!("{} \u{b7} {}", type_of(&of), held.len());
        let key = fresh_key(&held, numbered);
        let fresh = fresh_of(&of);
        self.holder(
            at,
            spec,
            note,
            Some(Means::Add {
                fresh,
                key: Some(key),
            }),
        );
        for key in sorted_keys(&held) {
            let inner = at.step(Step::Key(key.clone()), key.clone());
            self.removable(at, Step::Key(key.clone()));
            self.value(&inner, &of, &held[&key]);
            self.after = None;
        }
    }

    /// A record: a row a declared field, each of its own type. The fields
    /// are the schema's, so there is nothing to add or take away.
    fn record(&mut self, at: &Cursor, spec: &toml::Value, value: &toml::Value) {
        let fields = spec
            .get(i::FIELDS)
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        let names = sorted_keys(&fields);
        let note = match spec.get(i::CLASS).and_then(toml::Value::as_str) {
            Some(class) => format!("{class} \u{b7} {}", names.len()),
            None => format!("{} fields", names.len()),
        };
        self.holder(at, spec, note, None);
        for name in names {
            let of = &fields[&name];
            let inner = at.step(Step::Key(name.clone()), label_of(&name));
            let now = value
                .get(&name)
                .or_else(|| of.get(i::DEFAULT))
                .cloned()
                .unwrap_or_else(|| toml::Value::String(String::new()));
            self.value(&inner, of, &now);
        }
    }

    /// What a list, a map or a record says before its rows, and the chip that
    /// adds an entry where it takes one.
    fn holder(&mut self, at: &Cursor, spec: &toml::Value, note: String, add: Option<Means>) {
        let mut controls = vec![(
            control(&[
                (k::KIND, Value::text(w::LABEL)),
                (k::GROW, Value::Int(1)),
                (k::TEXT, Value::text(note)),
                (k::ROLE, Value::text(i::TEXT_CAPTION)),
            ]),
            None,
        )];
        if let Some(add) = add {
            controls.push((self.chip(ADD, "Add one"), Some((at.clone(), add))));
        }
        self.labelled(at, spec, controls);
    }

    /// The chip the next row carries, taking `step` out of the holder at
    /// `at`.
    fn removable(&mut self, at: &Cursor, step: Step) {
        self.after = Some((
            self.chip(REMOVE, "Take this one out"),
            Some((at.clone(), Means::Remove(step))),
        ));
    }

    fn chip(&self, text: &str, tooltip: &str) -> Value {
        control(&[
            (k::KIND, Value::text(w::BUTTON)),
            (k::TEXT, Value::text(text)),
            (k::ROLE, Value::text(i::CHIP)),
            (k::WIDTH, Value::Int(CHIP_W)),
            (k::HEIGHT, Value::Num(self.chip_h())),
            (k::TOOLTIP, Value::text(tooltip)),
        ])
    }

    /// Engine output: a write here would be overwritten the next frame, so
    /// it is a plain value with nothing to focus.
    fn readonly(&mut self, name: &str, spec: &toml::Value, value: &toml::Value) {
        let label = label_of(name);
        self.push(
            vec![
                (p::LABEL, Value::text(&label)),
                (k::TOOLTIP, Value::text(tip(&label, spec))),
            ],
            vec![
                (
                    control(&[
                        (k::KIND, Value::text(w::LABEL)),
                        (k::ROLE, Value::text(i::TEXT_CODE_QUIET)),
                        (k::GROW, Value::Int(1)),
                        (k::TRUNCATE, Value::Bool(true)),
                        (k::TEXT, Value::text(readonly_text(spec, value))),
                    ]),
                    None,
                ),
                (
                    control(&[
                        (k::KIND, Value::text(w::LABEL)),
                        (k::ROLE, Value::text(i::TEXT_HINT)),
                        (k::TEXT, Value::text("read-only")),
                    ]),
                    None,
                ),
            ],
        );
    }

    /// A value as the reader knows it: what the script says it is spelled
    /// as, else its path from `relative_to`.
    fn shown(&self, value: &str) -> String {
        for alias in &self.asked.aliases {
            if let Some((held, shown)) = alias.split_once('\u{1f}')
                && held == value
            {
                return shown.to_owned();
            }
        }
        if !self.asked.relative_to.is_empty()
            && let Some(rest) = value
                .strip_prefix(self.asked.relative_to.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
        {
            return rest.to_owned();
        }
        value.to_owned()
    }
}

/// A control's spec as the pool takes it.
fn control(entries: &[(&str, Value)]) -> Value {
    Value::Map(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    )
}

pub(super) fn empty_table() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

fn sorted_keys(table: &toml::map::Map<String, toml::Value>) -> Vec<String> {
    let mut keys: Vec<String> = table.keys().cloned().collect();
    keys.sort();
    keys
}

fn text_of<'a>(spec: &'a toml::Value, key: &str) -> &'a str {
    spec.get(key).and_then(toml::Value::as_str).unwrap_or("")
}

fn number_of(spec: &toml::Value, key: &str) -> Option<f64> {
    spec.get(key).and_then(balaur_core::components::as_f64)
}

fn flag(spec: &toml::Value, key: &str) -> bool {
    spec.get(key).and_then(toml::Value::as_bool) == Some(true)
}

fn type_of(of: &toml::Value) -> &str {
    match text_of(of, i::TYPE) {
        "" => "?",
        named => named,
    }
}

/// What a new entry starts as: the spec's own default.
fn fresh_of(of: &toml::Value) -> toml::Value {
    of.get(i::DEFAULT)
        .cloned()
        .unwrap_or_else(|| toml::Value::String(String::new()))
}

/// A key no entry of this map holds yet, so adding twice adds two.
fn fresh_key(held: &toml::map::Map<String, toml::Value>, numbered: bool) -> String {
    let named = |n: usize| match (numbered, n) {
        (true, n) => n.to_string(),
        (false, 0) => "key".to_owned(),
        (false, n) => format!("key{n}"),
    };
    // One more candidate than there are entries is always free.
    (0..=held.len())
        .map(named)
        .find(|key| !held.contains_key(key))
        .unwrap_or_default()
}

/// The first `count` numbers of a vector, `fallback` where it holds fewer.
pub(super) fn channels(value: &toml::Value, count: usize, fallback: f64) -> Vec<f64> {
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    (0..count)
        .map(|n| {
            items
                .get(n)
                .and_then(balaur_core::components::as_f64)
                .unwrap_or(fallback)
        })
        .collect()
}

pub(super) fn texts(value: &toml::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn strings_of(value: Option<&toml::Value>) -> Value {
    Value::List(
        value
            .map(texts)
            .unwrap_or_default()
            .into_iter()
            .map(Value::Str)
            .collect(),
    )
}

/// A value as a field shows it: text as itself, and no text for a list or a
/// table, which a field is not where one is edited.
fn as_text(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => text.clone(),
        toml::Value::Array(_) | toml::Value::Table(_) => String::new(),
        toml::Value::Float(f) => format!("{f:?}"),
        other => other.to_string(),
    }
}

/// A schema key as a row label: `edge_radius` reads "Edge radius". The key
/// itself is untouched, so a label never becomes the name of anything.
pub(super) fn label_of(prop: &str) -> String {
    let words: Vec<String> = prop
        .split('_')
        .map(|word| {
            if ACRONYMS.contains(&word) {
                word.to_uppercase()
            } else {
                word.to_owned()
            }
        })
        .collect();
    let joined = words.join(" ");
    let mut letters = joined.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => joined,
    }
}

/// A row's label, and what its spec says it is under it.
fn tip(label: &str, spec: &toml::Value) -> String {
    match text_of(spec, i::DESCRIPTION) {
        "" => label.to_owned(),
        description => format!("{label}\n{description}"),
    }
}

/// A readonly value written out: a number to its `decimals`, a vector's
/// channels joined.
fn readonly_text(spec: &toml::Value, value: &toml::Value) -> String {
    let degrees = text_of(spec, i::UNIT) == i::DEGREES;
    let shown = |v: f64| if degrees { v.to_degrees() } else { v };
    match text_of(spec, i::TYPE) {
        i::FLOAT => {
            // The places the editor has always written: past four is two.
            let decimals = match number_of(spec, i::DECIMALS).map(|d| d as i64) {
                Some(d @ (0 | 1 | 3 | 4)) => d as usize,
                _ => 2,
            };
            let held = balaur_core::components::as_f64(value).unwrap_or(0.0);
            format!("{:.*}", decimals, shown(held))
        }
        i::BOOL => (value.as_bool() == Some(true)).to_string(),
        datatype @ (i::VEC2 | i::VEC3 | i::COLOR) => {
            let (count, fallback) = match datatype {
                i::COLOR => (4, 1.0),
                i::VEC2 => (2, 0.0),
                _ => (3, 0.0),
            };
            channels(value, count, fallback)
                .into_iter()
                .map(|c| format!("{:.2}", shown(c)))
                .collect::<Vec<_>>()
                .join(", ")
        }
        _ => as_text(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_capitalises_its_first_word_and_its_acronyms() {
        assert_eq!(label_of("edge_radius"), "Edge radius");
        assert_eq!(label_of("uv_scale"), "UV scale");
        assert_eq!(label_of("fov"), "FOV");
        assert_eq!(label_of(""), "");
    }

    #[test]
    fn a_new_map_key_is_one_no_entry_holds() {
        let mut held = toml::map::Map::new();
        assert_eq!(fresh_key(&held, false), "key");
        held.insert("key".into(), toml::Value::Integer(1));
        held.insert("key1".into(), toml::Value::Integer(1));
        assert_eq!(fresh_key(&held, false), "key2");
        held.insert("0".into(), toml::Value::Integer(1));
        assert_eq!(fresh_key(&held, true), "1");
    }

    #[test]
    fn a_readonly_vector_reads_as_its_channels_and_a_float_to_its_decimals() {
        let spec: toml::Value = toml::from_str("type = \"vec2\"").unwrap();
        let value = toml::Value::Array(vec![1.0.into(), 2.5.into()]);
        assert_eq!(readonly_text(&spec, &value), "1.00, 2.50");
        let spec: toml::Value = toml::from_str("type = \"float\"\ndecimals = 1").unwrap();
        assert_eq!(readonly_text(&spec, &toml::Value::Float(0.25)), "0.2");
    }
}
