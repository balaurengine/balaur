//! Localization: `strings/<locale>.toml`, one file per language.
//!
//! ```toml
//! # strings/en.toml
//! "menu.play" = "Play"
//! "menu.items" = { one = "{n} item", other = "{n} items" }
//! ```
//!
//! `strings.tr("menu.play")` reads the current locale, falls back to the
//! project's fallback locale, and failing both answers with the key itself —
//! visible in the game rather than blank, because a missing string is a bug
//! to notice and an empty label is a bug to miss.

use std::cell::RefCell;
use std::collections::BTreeMap;

use anyhow::Result;

use crate::engine::Engine;

/// What `project.toml` says about languages.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocaleConfig {
    /// The locale a fresh run starts in.
    pub default: String,
    /// Where a key missing from the current locale is looked for next. One
    /// step, not a chain: two hops means two files to keep in mind, and the
    /// second one is always the language the game was written in.
    pub fallback: String,
}

impl Default for LocaleConfig {
    fn default() -> Self {
        Self {
            default: "en".to_string(),
            fallback: "en".to_string(),
        }
    }
}

impl LocaleConfig {
    #[must_use]
    pub fn load(eng: &Engine) -> Self {
        #[derive(serde::Deserialize)]
        struct Manifest {
            #[serde(default)]
            locale: LocaleConfig,
        }
        let Some(source) = crate::project::manifest_source(eng) else {
            return Self::default();
        };
        match toml::from_str::<Manifest>(&source) {
            Ok(manifest) => manifest.locale,
            Err(err) => {
                tracing::warn!("project.toml [locale]: {err}; using the defaults");
                Self::default()
            }
        }
    }
}

/// The loaded catalogues and which one is current.
#[derive(Default)]
pub struct Strings {
    current: String,
    fallback: String,
    /// Locale name to its catalogue, read once and kept.
    loaded: RefCell<BTreeMap<String, Catalogue>>,
    /// Where the catalogues are read from, when that is not the project root.
    /// See [`set_root`].
    root: Option<std::path::PathBuf>,
    ready: bool,
}

/// One locale's strings, as read from its file.
#[derive(Default)]
struct Catalogue {
    entries: BTreeMap<String, Entry>,
}

/// A string, or the plural forms of one.
enum Entry {
    One(String),
    /// Keyed by CLDR category: `one`, `few`, `many`, `other`, `zero`.
    Plural(BTreeMap<String, String>),
}

/// Which plural category a count falls in, for the languages named here.
///
/// Not the whole CLDR table: that is a generated artefact of some size, and
/// what a game needs is the handful of shapes its own translators write in.
/// A language nobody listed gets the English rule, which is also the rule for
/// most of the languages in the table.
fn category(language: &str, n: i64) -> &'static str {
    let abs = n.unsigned_abs();
    match language {
        // One form for everything: no count changes the wording.
        "ja" | "zh" | "ko" | "vi" | "th" | "id" => "other",
        // Romanian: 1 is one; 0 and 2..=19 (and the same mod 100) are few.
        "ro" | "mo" => {
            if n == 1 {
                "one"
            } else if abs == 0 || (1..=19).contains(&(abs % 100)) {
                "few"
            } else {
                "other"
            }
        }
        // The Slavic shape: 1 but not 11; 2-4 but not 12-14; the rest many.
        "ru" | "uk" | "be" | "pl" | "hr" | "sr" | "bs" => {
            let (ten, hundred) = (abs % 10, abs % 100);
            if ten == 1 && hundred != 11 {
                "one"
            } else if (2..=4).contains(&ten) && !(12..=14).contains(&hundred) {
                "few"
            } else {
                "many"
            }
        }
        // French counts 0 and 1 alike.
        "fr" | "pt" => {
            if abs <= 1 {
                "one"
            } else {
                "other"
            }
        }
        _ => {
            if n == 1 {
                "one"
            } else {
                "other"
            }
        }
    }
}

/// `en-GB` is English for the purpose of counting.
fn language_of(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or(locale)
}

/// Read a locale's file. A locale with no file is an empty catalogue rather
/// than an error: a game may ship one language ahead of the rest.
fn read(eng: &Engine, locale: &str) -> Catalogue {
    let path = format!("strings/{locale}.toml");
    let root = eng.resource::<Strings>().borrow().root.clone();
    let read = match &root {
        Some(root) => crate::files::backend(eng)
            .read(&root.join(&path))
            .and_then(|b| String::from_utf8(b).map_err(anyhow::Error::from)),
        None => crate::project::scene_text(eng, &path),
    };
    let Ok(source) = read else {
        return Catalogue::default();
    };
    let parsed: toml::Value = match toml::from_str(&source) {
        Ok(parsed) => parsed,
        Err(err) => {
            tracing::warn!("{path}: {err}; that locale reads as empty");
            return Catalogue::default();
        }
    };
    let mut catalogue = Catalogue::default();
    let Some(table) = parsed.as_table() else {
        return catalogue;
    };
    for (key, value) in table {
        let entry = match value {
            toml::Value::String(text) => Entry::One(text.clone()),
            toml::Value::Table(forms) => Entry::Plural(
                forms
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect(),
            ),
            other => {
                tracing::warn!("{path}: '{key}' is a {}, not a string", other.type_str());
                continue;
            }
        };
        catalogue.entries.insert(key.clone(), entry);
    }
    catalogue
}

/// Make sure the catalogues exist and the locale is the project's default.
fn ensure_ready(eng: &Engine) {
    let strings = eng.resource::<Strings>();
    if strings.borrow().ready {
        return;
    }
    let config = LocaleConfig::load(eng);
    let mut strings = strings.borrow_mut();
    strings.current.clone_from(&config.default);
    strings.fallback = config.fallback;
    strings.ready = true;
}

/// The locale in force.
pub fn locale(eng: &Engine) -> String {
    ensure_ready(eng);
    eng.resource::<Strings>().borrow().current.clone()
}

/// Switch locale. Takes effect on the next `tr`, which for a widget showing a
/// key means the next frame.
pub fn set_locale(eng: &Engine, locale: &str) {
    ensure_ready(eng);
    eng.resource::<Strings>().borrow_mut().current = locale.to_string();
}

/// Read the catalogues from `root` instead of the project root, and forget
/// the ones already read.
///
/// For a host running a project other than its own: the editor's project root
/// is the editor's, so without this every `text_key` in a played scene draws
/// as its own key. An empty `root` puts it back on the project.
pub fn set_root(eng: &Engine, root: &str) {
    let strings = eng.resource::<Strings>();
    let mut strings = strings.borrow_mut();
    strings.root = if root.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(root))
    };
    strings.loaded.borrow_mut().clear();
}

/// Every locale the project ships a file for, in name order.
pub fn locales(eng: &Engine) -> Vec<String> {
    let mut out = Vec::new();
    let root = {
        let strings = eng.resource::<Strings>();
        let strings = strings.borrow();
        strings.root.clone()
    }
    .unwrap_or_else(|| {
        eng.try_resource::<crate::project::ProjectRoot>()
            .map(|r| r.borrow().0.clone())
            .unwrap_or_default()
    });
    {
        for (name, _) in crate::files::backend(eng).list(&root.join("strings")) {
            if let Some(locale) = name.strip_suffix(".toml") {
                out.push(locale.to_string());
            }
        }
    }
    out.sort();
    out
}

/// The string for `key` in the current locale, with `args` interpolated.
///
/// `args` may carry an `n`, which also picks the plural form. A key nothing
/// has a string for comes back as itself.
pub fn tr(eng: &Engine, key: &str, args: &[(String, balaur_script::Value)]) -> String {
    ensure_ready(eng);
    let (current, fallback) = {
        let strings = eng.resource::<Strings>();
        let strings = strings.borrow();
        (strings.current.clone(), strings.fallback.clone())
    };
    let count = args
        .iter()
        .find(|(name, _)| name == "n")
        .and_then(|(_, v)| match v {
            balaur_script::Value::Int(n) => Some(*n),
            balaur_script::Value::Num(n) => Some(*n as i64),
            _ => None,
        });
    let text = lookup(eng, &current, key, count)
        .or_else(|| lookup(eng, &fallback, key, count))
        // The key itself: visible in the game, which is how a missing string
        // gets noticed rather than showing as a blank label.
        .unwrap_or_else(|| key.to_string());
    interpolate(&text, args)
}

/// One locale's answer for a key, if it has one.
fn lookup(eng: &Engine, locale: &str, key: &str, count: Option<i64>) -> Option<String> {
    // Read outside every borrow: a catalogue comes from the pack or the disk,
    // and that path reaches the script host.
    let missing = {
        let strings = eng.resource::<Strings>();
        let strings = strings.borrow();
        let loaded = strings.loaded.borrow();
        !loaded.contains_key(locale)
    };
    if missing {
        let catalogue = read(eng, locale);
        let strings = eng.resource::<Strings>();
        let strings = strings.borrow();
        strings
            .loaded
            .borrow_mut()
            .insert(locale.to_string(), catalogue);
    }
    let strings = eng.resource::<Strings>();
    let strings = strings.borrow();
    let loaded = strings.loaded.borrow();
    match loaded.get(locale)?.entries.get(key)? {
        Entry::One(text) => Some(text.clone()),
        Entry::Plural(forms) => {
            let wanted = category(language_of(locale), count.unwrap_or(0));
            forms.get(wanted).or_else(|| forms.get("other")).cloned()
        }
    }
}

/// `{name}` becomes the argument called `name`. A placeholder nothing was
/// passed for is left as it is, so a translator sees the hole.
fn interpolate(text: &str, args: &[(String, balaur_script::Value)]) -> String {
    if args.is_empty() || !text.contains('{') {
        return text.to_string();
    }
    let mut out = text.to_string();
    for (name, value) in args {
        let shown = match value {
            balaur_script::Value::Str(s) => s.clone(),
            balaur_script::Value::Int(n) => n.to_string(),
            balaur_script::Value::Num(n) => format!("{n}"),
            balaur_script::Value::Bool(b) => b.to_string(),
            other => other.type_name().to_string(),
        };
        out = out.replace(&format!("{{{name}}}"), &shown);
    }
    out
}

/// Forget every catalogue, so the next `tr` re-reads the files. What the
/// watcher calls when a strings file is saved.
pub fn reload(eng: &Engine) -> Result<()> {
    if let Some(strings) = eng.try_resource::<Strings>() {
        strings.borrow().loaded.borrow_mut().clear();
    }
    Ok(())
}
