//! Both halves of every bundled editor theme read at WCAG AA: every role's ink
//! on its own fill, and every token a script paints text with, on every sheet
//! it can land on.

use std::path::{Path, PathBuf};

use balaur_ui::contrast::{AA, color_of, pairs, ratio};
use balaur_ui::palette::complete;

/// The inks a script or a role writes text in.
const INKS: &[&str] = &[
    "text_default",
    "text_muted",
    "text_subtle",
    "primary_text",
    "secondary_text",
    "success_text",
    "warning_text",
    "danger_text",
    "syntax_keyword",
    "syntax_string",
    "syntax_number",
    "syntax_comment",
    "syntax_identifier",
    "syntax_type",
    "syntax_punctuation",
    "node_default",
    "node_2d",
    "node_3d",
    "node_ui",
    "node_physics",
    "node_bone",
    "node_modifier",
];
/// The sheets text lands on. `bg_control_hover` is a hover, and a hover keeps
/// its own ink.
const SHEETS: &[&str] = &["bg_app", "bg_panel", "bg_control"];

const HALVES: [&str; 2] = ["dark", "light"];

fn themes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editor/themes")
}

/// `roles`, or a half as `<pair>/<half>`.
fn file(name: &str) -> toml::Table {
    let text = std::fs::read_to_string(themes_dir().join(format!("{name}.toml"))).unwrap();
    text.parse::<toml::Table>().unwrap()
}

/// Every bundled pair: each folder under `editor/themes`.
fn pairs_bundled() -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(themes_dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

/// `over` written onto `base`, table by table, as the editor's `theme::merge`.
fn merge(base: &toml::Table, over: &toml::Table) -> toml::Table {
    let mut out = base.clone();
    for (key, value) in over {
        let merged = match (
            out.get(key).and_then(toml::Value::as_table),
            value.as_table(),
        ) {
            (Some(below), Some(above)) => toml::Value::Table(merge(below, above)),
            _ => value.clone(),
        };
        out.insert(key.clone(), merged);
    }
    out
}

/// Each bundled half as the editor wears it: the shared roles under its
/// colours, with every token derived.
fn themes() -> Vec<(String, toml::Value)> {
    let roles = file("roles");
    let mut out = Vec::new();
    for pair in pairs_bundled() {
        for half in HALVES {
            let name = format!("{pair}/{half}");
            let doc = complete(&toml::Value::Table(merge(&roles, &file(&name))));
            out.push((name, doc));
        }
    }
    assert!(out.len() >= 2, "the pairs were read: {}", out.len());
    out
}

fn contrast(colors: &toml::Table, a: &str, b: &str) -> f64 {
    ratio(
        color_of(Some(colors), a).unwrap(),
        color_of(Some(colors), b).unwrap(),
    )
}

#[test]
fn every_editor_theme_role_reads_at_aa_contrast() {
    let mut failing = Vec::new();
    for (theme, doc) in themes() {
        let found = pairs(&doc, "bg_panel");
        assert!(
            found.len() > 50,
            "{theme}: the roles were read: {}",
            found.len()
        );
        failing.extend(
            found
                .iter()
                .filter(|pair| pair.ratio < pair.need)
                .map(|pair| {
                    format!(
                        "{theme} {}: {} on {} is {:.2}:1, needs {}",
                        pair.role, pair.ink, pair.fill, pair.ratio, pair.need
                    )
                }),
        );
    }
    assert!(failing.is_empty(), "{}", failing.join("\n"));
}

#[test]
fn every_text_token_reads_at_aa_on_every_sheet() {
    let mut failing = Vec::new();
    for (theme, doc) in themes() {
        let colors = doc["colors"].as_table().unwrap();
        for ink in INKS {
            for sheet in SHEETS {
                let ratio = contrast(colors, ink, sheet);
                if ratio < AA {
                    failing.push(format!("{theme} {ink} on {sheet} is {ratio:.2}:1"));
                }
            }
        }
        for fill in ["primary_fill", "primary_fill_hover"] {
            let ratio = contrast(colors, "text_on_primary", fill);
            if ratio < AA {
                failing.push(format!("{theme} text_on_primary on {fill} is {ratio:.2}:1"));
            }
        }
    }
    assert!(failing.is_empty(), "{}", failing.join("\n"));
}

#[test]
fn a_bundled_theme_states_colours_and_the_roles_file_states_none() {
    for pair in pairs_bundled() {
        for half in HALVES {
            let doc = file(&format!("{pair}/{half}"));
            let keys: Vec<&String> = doc.keys().collect();
            assert_eq!(
                keys,
                ["colors", "type"],
                "{pair}/{half} states only its colours"
            );
        }
    }
    assert!(
        !file("roles").contains_key("colors"),
        "a colour in the shared roles would be the same in both themes"
    );
}

/// N20: a role is `<component>[_<context>][_<emphasis>]`. A state is never a
/// suffix, a role never takes a widget kind's word, and emphasis comes last.
#[test]
fn every_editor_role_is_named_component_context_emphasis() {
    const EMPHASIS: &[&str] = &[
        "primary",
        "secondary",
        "success",
        "warning",
        "danger",
        "quiet",
    ];
    const STATES: &[&str] = &[
        "on", "off", "hover", "active", "focus", "disabled", "checked",
    ];
    let kinds: Vec<&str> = balaur_ui::WIDGET_KINDS
        .iter()
        .map(|(_, word)| *word)
        .collect();
    let roles = file("roles");
    let roles = roles["roles"].as_table().unwrap();
    let mut failing = Vec::new();
    for name in roles.keys() {
        let words: Vec<&str> = name.split('_').collect();
        if kinds.contains(&name.as_str()) {
            failing.push(format!("{name} is a widget kind's word"));
        }
        if words.last().is_some_and(|last| STATES.contains(last)) {
            failing.push(format!("{name} carries a state as a suffix"));
        }
        let early = &words[..words.len() - 1];
        if early.iter().any(|word| EMPHASIS.contains(word)) {
            failing.push(format!("{name} has its emphasis before the end"));
        }
    }
    assert!(roles.len() > 50, "the roles were read: {}", roles.len());
    assert!(failing.is_empty(), "{}", failing.join("\n"));
}
