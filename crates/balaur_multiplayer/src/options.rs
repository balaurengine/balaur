//! What a `host` or `join` call asked for, over the project's
//! `[multiplayer]` table.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::handler::opt;
use balaur_script::Value;

use crate::vocabulary::TransportKind;

/// The settings category every key below lives under.
pub(crate) const CATEGORY: &str = "multiplayer";

/// The project's `[multiplayer]` table, as a schema.
pub(crate) const SCHEMA: &str = r#"
transport = { type = "enum", default = "webtransport", options = ["webtransport", "websocket"], order = 10, help = "What a host listens with: QUIC datagrams, or a websocket where UDP is blocked." }
address = { type = "string", default = "127.0.0.1:0", order = 11, help = "Where a host listens. 0.0.0.0 lets other machines in; port 0 takes any free one." }
players = { type = "int", default = 2, min = 0, max = 64, order = 12, help = "How many slots fill before a match starts on its own; 0 waits for the host's `start`." }
scene = { type = "string", default = "", order = 13, help = "The scene a match loads on every machine; empty for the main scene." }
depth = { type = "int", default = 16, min = 4, max = 120, order = 14, help = "Snapshots kept for rollback: how many ticks late an input can still be answered." }
timeout_seconds = { type = "float", default = 5.0, min = 0.5, max = 120.0, order = 15, help = "Seconds a join may take, and a link may go silent, before it is dropped." }
"#;

/// Everything a match is opened with.
#[derive(Clone, Debug)]
pub struct Options {
    pub transport: TransportKind,
    pub address: String,
    pub players: u32,
    pub scene: String,
    pub depth: usize,
    pub timeout: f32,
    pub name: String,
    pub token: String,
    pub cert_hash: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            transport: TransportKind::Webtransport,
            address: String::from("127.0.0.1:0"),
            players: 2,
            scene: String::new(),
            depth: 16,
            timeout: 5.0,
            name: String::from("Player"),
            token: String::new(),
            cert_hash: None,
        }
    }
}

impl Options {
    /// A script's `options` table, each key falling back to the setting.
    ///
    /// # Errors
    /// When a key holds the wrong kind of value, or names no transport.
    pub fn read(eng: &Engine, opts: Option<&Value>) -> Result<Self> {
        let transport_name =
            text(opts, "transport")?.unwrap_or_else(|| setting_text(eng, "transport"));
        let transport = TransportKind::parse(&transport_name)
            .ok_or_else(|| anyhow!("`{transport_name}` is not a transport"))?;
        let scene = text(opts, "scene")?.unwrap_or_else(|| setting_text(eng, "scene"));
        Ok(Self {
            transport,
            address: text(opts, "address")?.unwrap_or_else(|| setting_text(eng, "address")),
            players: count(opts, "players")?
                .unwrap_or_else(|| setting_int(eng, "players").try_into().unwrap_or(2)),
            scene: if scene.is_empty() {
                main_scene(eng)
            } else {
                scene
            },
            depth: count(opts, "depth")?
                .unwrap_or_else(|| setting_int(eng, "depth").try_into().unwrap_or(16))
                .try_into()
                .unwrap_or(16),
            timeout: number(opts, "timeout_seconds")?
                .unwrap_or_else(|| setting_number(eng, "timeout_seconds")),
            name: text(opts, "name")?.unwrap_or_else(|| String::from("Player")),
            token: text(opts, "token")?.unwrap_or_default(),
            cert_hash: text(opts, "cert_hash")?.filter(|hash| !hash.is_empty()),
        })
    }
}

fn text(opts: Option<&Value>, key: &str) -> Result<Option<String>> {
    match opt(opts, key) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Str(s)) => Ok(Some(s.clone())),
        Some(other) => Err(anyhow!(
            "`{key}` should be a string, got {}",
            other.type_name()
        )),
    }
}

fn count(opts: Option<&Value>, key: &str) -> Result<Option<u32>> {
    match opt(opts, key) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Int(n)) => u32::try_from(*n)
            .map(Some)
            .map_err(|_| anyhow!("`{key}` should be a whole number from 0, got {n}")),
        Some(other) => Err(anyhow!(
            "`{key}` should be a whole number, got {}",
            other.type_name()
        )),
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "seconds, far inside f32's range"
)]
fn number(opts: Option<&Value>, key: &str) -> Result<Option<f32>> {
    match opt(opts, key) {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::Num(n)) => Ok(Some(*n as f32)),
        #[allow(clippy::cast_precision_loss, reason = "a count of seconds")]
        Some(Value::Int(n)) => Ok(Some(*n as f32)),
        Some(other) => Err(anyhow!(
            "`{key}` should be a number, got {}",
            other.type_name()
        )),
    }
}

fn setting(eng: &Engine, key: &str) -> Option<toml::Value> {
    balaur_core::settings::get(eng, &format!("{CATEGORY}/{key}"))
}

fn setting_text(eng: &Engine, key: &str) -> String {
    setting(eng, key)
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn setting_int(eng: &Engine, key: &str) -> i64 {
    setting(eng, key).and_then(|v| v.as_integer()).unwrap_or(0)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "seconds, far inside f32's range"
)]
fn setting_number(eng: &Engine, key: &str) -> f32 {
    setting(eng, key)
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
        .unwrap_or(5.0) as f32
}

/// The project's main scene, which a match loads when nothing names another.
fn main_scene(eng: &Engine) -> String {
    eng.try_resource::<balaur_core::project::ProjectManifest>()
        .map(|manifest| manifest.borrow().main_scene.clone())
        .unwrap_or_default()
}
