//! Godot's translation CSVs as `strings/<locale>.toml`.
//!
//! A `.csv` whose `.import` names the `csv_translation` importer is a table:
//! the first column holds the keys, every other column a locale, and a column
//! whose header starts with `_` is a comment Godot skips. Balaur keeps one
//! flat file per locale, so every table's column for a locale lands in that
//! locale's file. The compiled `.translation` beside each is not read: the
//! CSV is its source and says everything it does.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Result, bail};
use balaur_plugin::toml;

/// Every locale's strings, and every key any of them holds.
#[derive(Default)]
pub(crate) struct Strings {
    pub locales: BTreeMap<String, BTreeMap<String, String>>,
    pub keys: BTreeSet<String>,
    pub notes: Vec<String>,
}

impl Strings {
    /// Each locale's file, as `(project path, text)`.
    pub(crate) fn files(&self) -> Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        for (locale, table) in &self.locales {
            let mut doc = toml::Table::new();
            for (key, text) in table {
                doc.insert(key.clone(), toml::Value::String(text.clone()));
            }
            let mut text = String::from("# Converted from Godot translation CSVs by `balaur import`.\n");
            text.push_str(&toml::to_string(&toml::Value::Table(doc))?);
            out.push((format!("strings/{locale}.toml"), text));
        }
        Ok(out)
    }
}

/// Read every translation CSV in `files` (project-relative) under `root`.
pub(crate) fn convert(root: &Path, files: &[String]) -> Strings {
    let mut strings = Strings::default();
    for relative in files.iter().filter(|f| f.ends_with(".csv")) {
        let Some(settings) = translation_settings(&root.join(format!("{relative}.import"))) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(root.join(relative)) else {
            strings.notes.push(format!("{relative}: would not read"));
            continue;
        };
        match table(&text, settings.delimiter) {
            Ok(rows) => add(&mut strings, &rows, settings.unescape),
            Err(why) => strings.notes.push(format!("{relative}: {why}")),
        }
    }
    strings
}

struct Settings {
    delimiter: char,
    unescape: bool,
}

/// The import settings, when the file is a translation table at all.
fn translation_settings(import: &Path) -> Option<Settings> {
    let text = std::fs::read_to_string(import).ok()?;
    let document = crate::import_godot::parse(&text).ok()?;
    let remap = document.first("remap")?;
    if remap.field("importer")?.as_str()? != "csv_translation" {
        return None;
    }
    let params = document.first("params");
    let param = |key: &str| params.and_then(|p| p.field(key));
    // Godot's delimiter is an enum: comma, semicolon, tab.
    let delimiter = match param("delimiter").and_then(crate::import_godot::Value::as_i64) {
        Some(1) => ';',
        Some(2) => '\t',
        _ => ',',
    };
    let unescape = param("unescape_translations") != Some(&crate::import_godot::Value::Bool(false));
    Some(Settings {
        delimiter,
        unescape,
    })
}

fn add(strings: &mut Strings, rows: &[Vec<String>], unescape: bool) {
    let Some((header, body)) = rows.split_first() else {
        return;
    };
    let columns: Vec<(usize, &str)> = header
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, name)| !name.starts_with('_') && !name.trim().is_empty())
        .map(|(i, name)| (i, name.trim()))
        .collect();
    for row in body {
        let Some(key) = row.first().filter(|k| !k.is_empty()) else {
            continue;
        };
        strings.keys.insert(key.clone());
        for (index, locale) in &columns {
            let Some(text) = row.get(*index).filter(|t| !t.is_empty()) else {
                continue;
            };
            let text = if unescape { unescaped(text) } else { text.clone() };
            strings
                .locales
                .entry((*locale).to_string())
                .or_default()
                .insert(key.clone(), text);
        }
    }
}

/// Godot's `c_unescape`: the backslash escapes a translator types.
fn unescaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// RFC 4180: a quoted field may hold the delimiter, a newline, and `""` for
/// a quote.
fn table(text: &str, delimiter: char) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                other => field.push(other),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c if c == delimiter => row.push(std::mem::take(&mut field)),
            other => field.push(other),
        }
    }
    if quoted {
        bail!("a quoted field never closes");
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::{Strings, add, table, unescaped};

    #[test]
    fn a_table_splits_on_its_delimiter_and_keeps_quoted_commas_and_newlines() {
        let rows = table("keys,en\n\"a, b\",\"one\ntwo\"\nc,\"say \"\"hi\"\"\"\n", ',').unwrap();
        assert_eq!(rows[1], vec!["a, b", "one\ntwo"]);
        assert_eq!(rows[2], vec!["c", "say \"hi\""]);
    }

    /// Godot skips a column whose header starts with `_`, which is how this
    /// game carries its category and its English source beside each locale.
    #[test]
    fn underscore_columns_are_comments_and_every_other_is_a_locale() {
        let rows = table("keys,_category,_en,ro\nCoins,Profile,Coins,Monede\n", ',').unwrap();
        let mut strings = Strings::default();
        add(&mut strings, &rows, true);
        assert_eq!(strings.locales.keys().collect::<Vec<_>>(), vec!["ro"]);
        assert_eq!(strings.locales["ro"]["Coins"], "Monede");
        assert!(strings.keys.contains("Coins"));
    }

    #[test]
    fn a_translators_escapes_become_the_characters_they_name() {
        assert_eq!(unescaped(r"one\ntwo\ttab \\ done"), "one\ntwo\ttab \\ done");
    }
}
