//! Which Gamend server a run talks to: the project names a production server
//! and a local one, and the person at the editor picks between them. The
//! pick is an editor setting, so it can never ship in a pack.

use balaur_core::Engine;
use balaur_core::settings::{self, Scope};
use balaur_script::Value;

/// The settings category every key below lives under.
const CATEGORY: &str = "gamend";

/// Where a new game talks to until it names its own server.
pub(crate) const DEFAULT_URL: &str = "https://gamend.org";

const PROJECT_SCHEMA: &str = r#"
url = { type = "string", default = "https://gamend.org", order = 1, help = "The Gamend server a shipped game talks to." }
local_url = { type = "string", default = "http://localhost:4000", order = 2, help = "A Gamend running on this machine, as `mix dev.start` serves it." }
plugin = { type = "string", default = "", order = 3, help = "The server plugin a game's hooks are called in." }
"#;

const EDITOR_SCHEMA: &str = r#"
target = { type = "enum", default = "production", options = ["production", "local"], order = 4, help = "Which of the project's two servers a game played from this editor talks to. Kept in the editor's own file, so it never ships." }
"#;

/// The two names a target goes by.
pub(crate) const PRODUCTION: &str = "production";
pub(crate) const LOCAL: &str = "local";

pub(crate) fn declare(eng: &Engine) {
    for (scope, schema) in [
        (Scope::Project, PROJECT_SCHEMA),
        (Scope::Editor, EDITOR_SCHEMA),
    ] {
        settings::define_group(
            eng,
            CATEGORY,
            scope,
            &balaur_core::ComponentDef::parse_schema("settings.gamend", schema),
        );
    }
}

fn text(eng: &Engine, key: &str) -> String {
    settings::get(eng, &format!("{CATEGORY}/{key}"))
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The server a `configure()` with no URL points at: the local one when the
/// editor's target says so, the project's own otherwise.
pub(crate) fn url(eng: &Engine) -> String {
    let production = Some(text(eng, "url"))
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string());
    if text(eng, "target") == LOCAL {
        let local = text(eng, "local_url");
        if !local.is_empty() {
            return local;
        }
    }
    production
}

/// `gamend.target()`: both servers, which one is picked, and the plugin.
pub(crate) fn value(eng: &Engine) -> Value {
    let picked = if text(eng, "target") == LOCAL {
        LOCAL
    } else {
        PRODUCTION
    };
    Value::Map(vec![
        (String::from("name"), Value::Str(picked.into())),
        (String::from("url"), Value::Str(url(eng))),
        (String::from("production"), Value::Str(text(eng, "url"))),
        (String::from("local"), Value::Str(text(eng, "local_url"))),
        (String::from("plugin"), Value::Str(text(eng, "plugin"))),
    ])
}
