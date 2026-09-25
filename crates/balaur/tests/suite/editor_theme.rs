//! The editor's two themes read at WCAG AA: every role's ink on its own fill,
//! and every token a script paints text with, on every sheet it can land on.

use std::path::Path;

use balaur_ui::contrast::{AA, color_of, pairs, ratio};

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

/// Two tokens' contrast, each looked up in `[colors]`.
fn contrast(colors: &toml::Table, a: &str, b: &str) -> f64 {
    ratio(
        color_of(Some(colors), a).unwrap(),
        color_of(Some(colors), b).unwrap(),
    )
}

#[test]
fn every_editor_theme_role_reads_at_aa_contrast() {
    for (theme, doc) in themes() {
        let doc = toml::Value::Table(doc);
        let failing: Vec<String> = pairs(&doc, "panel")
            .iter()
            .filter(|pair| pair.ratio < pair.need)
            .map(|pair| {
                format!(
                    "{theme} {}: {} on {} is {:.2}:1, needs {}",
                    pair.role, pair.ink, pair.fill, pair.ratio, pair.need
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
                let ratio = contrast(colors, ink, sheet);
                if ratio < AA {
                    failing.push(format!("{theme} {ink} on {sheet} is {ratio:.2}:1"));
                }
            }
        }
        for fill in ["accent_fill", "accent"] {
            let ratio = contrast(colors, "on_accent", fill);
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
