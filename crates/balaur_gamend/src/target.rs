//! Which Gamend server a run talks to: the project names a production server
//! and a local one. `GAMEND_URL` in the environment wins, the person at the
//! editor may pick, and otherwise a run from the sources talks to the local
//! one. The pick is an editor setting, so it can never ship in a pack.

use crate::vocabulary::words;
use balaur_core::Engine;
use balaur_core::settings::{self, Scope};
use balaur_script::Value;

const LOCAL_URL: &str = "local_url";
const PLUGIN: &str = "plugin";

/// The settings category every key below lives under.
const CATEGORY: &str = "gamend";

/// Where a new game talks to until it names its own server.
pub(crate) const DEFAULT_URL: &str = "https://gamend.org";

const PROJECT_SCHEMA: &str = r#"
url = { type = "string", default = "https://gamend.org", order = 1, help = "The Gamend server a shipped game talks to." }
local_url = { type = "string", default = "http://localhost:4000", order = 2, help = "A Gamend running on this machine, as `mix dev.start` serves it." }
plugin = { type = "string", default = "", order = 3, help = "The server plugin a game's hooks are called in." }
"#;

// `cloud_token` is what `/editor/connect` on gamend.org handed the dock; the
// settings screen draws a `secret` setting as dots.
const EDITOR_SCHEMA: &str = r#"
server = { type = "enum", default = "local", options = ["production", "local"], order = 4, help = "Which of the project's two servers a game played from this editor talks to. Kept in the editor's own file, so it never ships." }
cloud = { type = "string", default = "https://gamend.org", order = 5, help = "Where your Gamend Cloud account is: the Gamend dock lists your servers from it." }
cloud_token = { type = "string", default = "", secret = true, order = 6, help = "What the Gamend dock got when you connected it to your Gamend Cloud account. Kept in the editor's own file; empty disconnects it." }
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

/// `GAMEND_URL`, for a run an environment points somewhere else. A browser has
/// no environment.
fn from_environment() -> Option<String> {
    #[cfg(target_family = "wasm")]
    return None;
    #[cfg(not(target_family = "wasm"))]
    std::env::var("GAMEND_URL")
        .ok()
        .filter(|url| !url.is_empty())
}

/// Whether this run talks to the local server: what the editor's setting says,
/// and otherwise whether it runs from the sources rather than a pack.
fn is_local(eng: &Engine) -> bool {
    settings::stated(eng, &format!("{CATEGORY}/server")).map_or_else(
        || balaur_core::facts::platform(eng).dev,
        |picked| picked.as_str() == Some(LOCAL),
    )
}

/// The server a run talks to until a script names another.
pub(crate) fn url(eng: &Engine) -> String {
    if let Some(named) = from_environment() {
        return named;
    }
    let local = text(eng, LOCAL_URL);
    if is_local(eng) && !local.is_empty() {
        return local;
    }
    Some(text(eng, words::target::URL))
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

/// `gamend.server()`: both servers, which one is picked, and the plugin.
pub(crate) fn value(eng: &Engine) -> Value {
    let picked = if is_local(eng) { LOCAL } else { PRODUCTION };
    Value::Map(vec![
        (String::from("name"), Value::Str(picked.into())),
        (String::from(words::target::URL), Value::Str(url(eng))),
        (
            String::from(PRODUCTION),
            Value::Str(text(eng, words::target::URL)),
        ),
        (String::from(LOCAL), Value::Str(text(eng, LOCAL_URL))),
        (String::from(PLUGIN), Value::Str(text(eng, PLUGIN))),
    ])
}

#[cfg(test)]
mod tests {
    use balaur_core::{App, AppConfig};

    /// A run from the sources talks to the local server until the editor picks.
    #[test]
    fn a_run_from_the_sources_talks_to_the_local_server_until_someone_picks() {
        if std::env::var_os("GAMEND_URL").is_some() {
            return;
        }
        let app = App::new(AppConfig::bare(".")).unwrap();
        super::declare(&app.engine);
        let text =
            |value: &str| balaur_core::components::derived::toml::Value::String(value.into());
        let set = |key: &str, value: &str| {
            balaur_core::settings::set(&app.engine, &format!("gamend/{key}"), text(value));
        };
        set("url", "https://play.example");
        set("local_url", "http://localhost:4000");
        assert_eq!(super::url(&app.engine), "http://localhost:4000");
        set("server", super::PRODUCTION);
        assert_eq!(super::url(&app.engine), "https://play.example");
    }
}
