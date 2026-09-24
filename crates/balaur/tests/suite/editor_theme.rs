//! The editor's two themes read at WCAG AA: every role's ink on its own fill,
//! and every token a script paints text with, on every sheet it can land on.

use std::path::Path;

const AA: f64 = 4.5;
const AA_LARGE: f64 = 3.0;

/// The inks a script or a role writes text in.
const INKS: &[&str] = &[
    "text",
    "dim",
    "faint",
    "accent",
    "sage",
    "warn",
    "danger",
    "k_key",
    "k_str",
    "k_num",
    "k_com",
    "k_fn",
    "k_type",
    "k_punc",
    "node_plain",
    "node_2d",
    "node_3d",
    "node_ui",
    "node_phys",
    "node_bone",
    "modifier",
];
/// The sheets text lands on. `raised` is a hover, and a hover keeps its own ink.
const SHEETS: &[&str] = &["bg", "panel", "sunken"];

fn themes() -> Vec<(&'static str, toml::Table)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor/themes");
    ["dark", "light"]
        .into_iter()
        .map(|name| {
            let text = std::fs::read_to_string(dir.join(format!("{name}.toml"))).unwrap();
            (name, text.parse::<toml::Table>().unwrap())
        })
        .collect()
}

fn luminance(hex: &str) -> f64 {
    let channel = |at: usize| {
        let c = f64::from(u8::from_str_radix(&hex[at..at + 2], 16).unwrap()) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            libm::pow((c + 0.055) / 1.055, 2.4)
        }
    };
    0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// A colour as `#rrggbb`: a token looked up, or the literal a role states.
fn hex<'a>(colors: &'a toml::Table, value: &'a str) -> &'a str {
    colors
        .get(value)
        .and_then(toml::Value::as_str)
        .unwrap_or(value)
}

/// What one role table and the state tables under it draw: the ink, the fill
/// it sits on, and the ratio the size asks for. A state table inherits from
/// the table above it; a role with no fill is drawn on `panel`.
fn role_pairs(
    name: &str,
    table: &toml::Table,
    base: &(String, String, f64),
    out: &mut Vec<(String, String, String, f64)>,
) {
    let get = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    };
    let fill = get("fill").unwrap_or_else(|| base.0.clone());
    let ink = get("color").unwrap_or_else(|| base.1.clone());
    let size = table
        .get("size")
        .and_then(toml::Value::as_float)
        .or_else(|| {
            table
                .get("size")
                .and_then(toml::Value::as_integer)
                .map(|n| n as f64)
        });
    let strong = table
        .get("strong")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let large = size.is_some_and(|s| s >= 18.0 || (s >= 14.0 && strong));
    let need = if large { AA_LARGE } else { base.2.min(AA) };
    if !ink.is_empty() {
        out.push((name.to_owned(), ink.clone(), fill.clone(), need));
    }
    for (key, value) in table {
        if let Some(state) = value.as_table() {
            role_pairs(
                &format!("{name}.{key}"),
                state,
                &(fill.clone(), ink.clone(), need),
                out,
            );
        }
    }
}

#[test]
fn every_editor_theme_role_reads_at_aa_contrast() {
    for (theme, doc) in themes() {
        let colors = doc["colors"].as_table().unwrap();
        let mut pairs = Vec::new();
        for (name, role) in doc["roles"].as_table().unwrap() {
            role_pairs(
                name,
                role.as_table().unwrap(),
                &("panel".into(), String::new(), AA),
                &mut pairs,
            );
        }
        let failing: Vec<String> = pairs
            .iter()
            .filter(|(_, ink, fill, need)| contrast(hex(colors, ink), hex(colors, fill)) < *need)
            .map(|(name, ink, fill, need)| {
                format!(
                    "{theme} {name}: {ink} on {fill} is {:.2}:1, needs {need}",
                    contrast(hex(colors, ink), hex(colors, fill))
                )
            })
            .collect();
        assert!(failing.is_empty(), "{}", failing.join("\n"));
    }
}

#[test]
fn every_text_token_reads_at_aa_on_every_sheet() {
    for (theme, doc) in themes() {
        let colors = doc["colors"].as_table().unwrap();
        let mut failing = Vec::new();
        for ink in INKS {
            for sheet in SHEETS {
                let ratio = contrast(hex(colors, ink), hex(colors, sheet));
                if ratio < AA {
                    failing.push(format!("{theme} {ink} on {sheet} is {ratio:.2}:1"));
                }
            }
        }
        for fill in ["accent_fill", "accent"] {
            let ratio = contrast(hex(colors, "on_accent"), hex(colors, fill));
            if ratio < AA {
                failing.push(format!("{theme} on_accent on {fill} is {ratio:.2}:1"));
            }
        }
        assert!(failing.is_empty(), "{}", failing.join("\n"));
    }
}

#[test]
fn both_editor_themes_state_the_same_keys() {
    fn keys(prefix: &str, table: &toml::Table, out: &mut Vec<String>) {
        for (key, value) in table {
            let path = format!("{prefix}{key}");
            match value.as_table() {
                Some(inner) => keys(&format!("{path}."), inner, out),
                None => out.push(path),
            }
        }
    }
    let mut sets = themes().into_iter().map(|(_, doc)| {
        let mut out = Vec::new();
        keys("", &doc, &mut out);
        out.retain(|key| key != "dark");
        out
    });
    let (dark, light) = (sets.next().unwrap(), sets.next().unwrap());
    assert_eq!(dark, light);
}
