//! Named input actions: what a game asks about instead of a key.
//!
//! A project declares its actions in `project.toml`; scripts ask by name, and
//! the raw snapshot stays underneath. That layering is the point for replay:
//! a recording holds the keys and pads that were pressed, and the actions are
//! derived from them again on the way back, so a recording does not go stale
//! when a binding changes.
//!
//! ```toml
//! [input.actions]
//! jump = ["Space", "gamepad:South"]
//! move_x = ["keys:A,D", "axis:LeftStickX"]
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use balaur_core::Engine;
use balaur_script::Bindings;

use crate::InputSnapshot;
use crate::gamepad::GamepadState;

/// Below this an axis reads as idle, so a worn stick does not hold an action
/// down forever.
const DEADZONE: f32 = 0.15;

/// At or past this magnitude an action counts as pressed. Half of full throw:
/// far enough that a resting stick never trips it, near enough that a player
/// pushing in a direction always does.
const PRESSED: f32 = 0.5;

/// One source of an action's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Binding {
    /// A key, spelled as `input.KEY_*` spells it: `"Space"`, `"KeyA"`.
    Key(String),
    /// `"mouse:left"`, `"mouse:right"`, `"mouse:middle"`.
    Mouse(usize),
    /// `"gamepad:South"`, using the `input.PAD_*` spelling.
    Pad(String),
    /// `"axis:LeftStickX"`, and the half-axes `"axis:LeftStickY+"` and
    /// `"axis:LeftStickY-"` for a direction that should read as one action.
    Axis { name: String, half: Half },
    /// `"keys:A,D"` — two keys as one axis, the first negative.
    KeyPair(String, String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Half {
    Both,
    Positive,
    Negative,
}

impl Binding {
    /// Parse one binding string, or say why it is not one.
    fn parse(text: &str) -> Result<Self, String> {
        let Some((kind, rest)) = text.split_once(':') else {
            return if crate::is_known_key(text) {
                Ok(Self::Key(text.to_string()))
            } else {
                Err(format!("'{text}' is not a key name"))
            };
        };
        match kind {
            "mouse" => match rest {
                "left" => Ok(Self::Mouse(0)),
                "right" => Ok(Self::Mouse(1)),
                "middle" => Ok(Self::Mouse(2)),
                other => Err(format!("'{other}' is not a mouse button")),
            },
            "gamepad" => {
                if crate::PAD_BUTTON_NAMES.contains(&rest) {
                    Ok(Self::Pad(rest.to_string()))
                } else {
                    Err(format!("'{rest}' is not a gamepad button"))
                }
            }
            "axis" => {
                let (name, half) = match rest.strip_suffix('+') {
                    Some(name) => (name, Half::Positive),
                    None => match rest.strip_suffix('-') {
                        Some(name) => (name, Half::Negative),
                        None => (rest, Half::Both),
                    },
                };
                if crate::PAD_AXIS_NAMES.contains(&name) {
                    Ok(Self::Axis {
                        name: name.to_string(),
                        half,
                    })
                } else {
                    Err(format!("'{name}' is not a gamepad axis"))
                }
            }
            "keys" => {
                let Some((low, high)) = rest.split_once(',') else {
                    return Err(format!("'{rest}' needs two key names, as 'A,D'"));
                };
                let (low, high) = (low.trim(), high.trim());
                for key in [low, high] {
                    if !crate::is_known_key(key) {
                        return Err(format!("'{key}' is not a key name"));
                    }
                }
                Ok(Self::KeyPair(low.to_string(), high.to_string()))
            }
            other => Err(format!("'{other}:' is not a binding kind")),
        }
    }

    /// This binding's contribution this frame, in -1..1.
    fn value(&self, keys: &InputSnapshot, pads: &GamepadState) -> f32 {
        match self {
            Self::Key(key) => f32::from(u8::from(keys.is_down(key))),
            Self::Mouse(button) => f32::from(u8::from(keys.is_mouse_down(*button))),
            Self::Pad(button) => {
                f32::from(u8::from(pads.pads().iter().any(|pad| pad.is_down(button))))
            }
            Self::Axis { name, half } => {
                // Any pad, because a game with one player does not care which
                // controller the value came from.
                let raw = pads
                    .pads()
                    .iter()
                    .map(|pad| pad.axis(name))
                    .fold(
                        0.0_f32,
                        |best, v| {
                            if v.abs() > best.abs() { v } else { best }
                        },
                    );
                let raw = if raw.abs() < DEADZONE { 0.0 } else { raw };
                match half {
                    Half::Both => raw,
                    Half::Positive => raw.max(0.0),
                    Half::Negative => (-raw).max(0.0),
                }
            }
            Self::KeyPair(low, high) => {
                f32::from(u8::from(keys.is_down(high))) - f32::from(u8::from(keys.is_down(low)))
            }
        }
    }

    /// The spelling this binding was parsed from, for `input.bindings` and
    /// for the file rebindings are saved to.
    fn text(&self) -> String {
        match self {
            Self::Key(key) => key.clone(),
            Self::Mouse(0) => String::from("mouse:left"),
            Self::Mouse(1) => String::from("mouse:right"),
            Self::Mouse(_) => String::from("mouse:middle"),
            Self::Pad(button) => format!("gamepad:{button}"),
            Self::Axis { name, half } => match half {
                Half::Both => format!("axis:{name}"),
                Half::Positive => format!("axis:{name}+"),
                Half::Negative => format!("axis:{name}-"),
            },
            Self::KeyPair(low, high) => format!("keys:{low},{high}"),
        }
    }
}

/// One action's value, this frame and last, so an edge is a comparison rather
/// than a special case per binding kind — a stick pushed past the threshold
/// fires `just_pressed` exactly as a key does.
#[derive(Clone, Copy, Default)]
struct ActionState {
    value: f32,
    previous: f32,
}

/// Every action a project declares, with what each is bound to now.
///
/// Loaded on the first tick rather than at plugin build: the manifest is read
/// when the project loads, which is after every plugin has been added.
#[derive(Default)]
pub struct InputActions {
    bound: BTreeMap<String, Vec<Binding>>,
    state: BTreeMap<String, ActionState>,
    /// Actions the player rebound, kept apart from the project's so
    /// `reset_bindings` has something to go back to.
    overrides: BTreeMap<String, Vec<Binding>>,
    loaded: bool,
}

impl InputActions {
    /// Action names, in a stable order.
    pub fn names(&self) -> Vec<String> {
        self.bound.keys().cloned().collect()
    }

    /// What `name` is bound to now, as the strings a project file would use.
    pub fn bindings(&self, name: &str) -> Vec<String> {
        self.bound
            .get(name)
            .map(|list| list.iter().map(Binding::text).collect())
            .unwrap_or_default()
    }

    /// -1..1, the contribution furthest from rest. A key beats a resting
    /// stick; a stick pushed hard beats a key that is not down.
    pub fn value(&self, name: &str) -> f32 {
        self.state.get(name).map_or(0.0, |s| s.value)
    }

    pub fn is_pressed(&self, name: &str) -> bool {
        self.value(name).abs() >= PRESSED
    }

    pub fn just_pressed(&self, name: &str) -> bool {
        self.state
            .get(name)
            .is_some_and(|s| s.value.abs() >= PRESSED && s.previous.abs() < PRESSED)
    }

    pub fn just_released(&self, name: &str) -> bool {
        self.state
            .get(name)
            .is_some_and(|s| s.value.abs() < PRESSED && s.previous.abs() >= PRESSED)
    }

    pub fn is_declared(&self, name: &str) -> bool {
        self.bound.contains_key(name)
    }

    /// Declare the project's actions outright, replacing what was declared
    /// before and keeping the player's own rebindings on top.
    ///
    /// For a host running a project other than its own — the editor, whose
    /// `project.toml` is the editor's and not the game's, so without this
    /// every action a played game asks for reads zero.
    pub(crate) fn declare(&mut self, actions: BTreeMap<String, Vec<Binding>>) {
        self.bound = actions;
        for (name, bindings) in &self.overrides {
            self.bound.insert(name.clone(), bindings.clone());
        }
        self.loaded = true;
    }

    /// Rebind one action, replacing every binding it had. An unparseable
    /// string is refused so a rebinding screen can say so.
    pub fn rebind(&mut self, name: &str, bindings: &[String]) -> Result<(), String> {
        let parsed = bindings
            .iter()
            .map(|b| Binding::parse(b))
            .collect::<Result<Vec<_>, _>>()?;
        self.overrides.insert(name.to_string(), parsed.clone());
        self.bound.insert(name.to_string(), parsed);
        Ok(())
    }

    /// Drop every rebinding, going back to what the project declared.
    fn clear_overrides(&mut self, project: &BTreeMap<String, Vec<Binding>>) {
        self.overrides.clear();
        self.bound = project.clone();
    }
}

/// Recompute every action from this frame's raw input.
///
/// Runs in `Stage::First`, after the gamepad poll and after a replay has
/// restored the recorded snapshot — which is what makes a replayed action
/// identical to the one that was played, edges included.
pub(crate) fn tick(eng: &Engine) {
    let actions = eng.resource::<InputActions>();
    ensure_loaded(eng);
    let keys = eng.resource::<InputSnapshot>();
    let pads = eng.resource::<GamepadState>();
    let keys = keys.borrow();
    let pads = pads.borrow();
    let mut actions = actions.borrow_mut();
    let InputActions { bound, state, .. } = &mut *actions;
    for (name, bindings) in bound.iter() {
        let value = bindings
            .iter()
            .map(|b| b.value(&keys, &pads))
            .fold(
                0.0_f32,
                |best, v| if v.abs() > best.abs() { v } else { best },
            );
        let slot = state.entry(name.clone()).or_default();
        slot.previous = slot.value;
        slot.value = value;
    }
}

/// Load the project's table and the player's rebindings over it, once.
///
/// Lazy because the manifest is read when the project loads, which is after
/// every plugin has been built — and because a recording's header may have
/// put a table here first, in which case there is nothing to load.
fn ensure_loaded(eng: &Engine) {
    let actions = eng.resource::<InputActions>();
    if actions.borrow().loaded {
        return;
    }
    let loaded = load(eng);
    let mut actions = actions.borrow_mut();
    actions.bound = loaded;
    actions.loaded = true;
    apply_saved_rebindings(eng, &mut actions);
}

/// What a recording carries, so a replay derives its actions from the
/// bindings that were in force when it was made.
///
/// Every binding, not only the rebound ones: the project's table can change
/// between recording and replay too.
fn capture_bindings(eng: &Engine) -> serde_json::Value {
    ensure_loaded(eng);
    let actions = eng.resource::<InputActions>();
    let actions = actions.borrow();
    let table: BTreeMap<String, Vec<String>> = actions
        .bound
        .keys()
        .map(|name| (name.clone(), actions.bindings(name)))
        .collect();
    serde_json::to_value(table).unwrap_or(serde_json::Value::Null)
}

/// Put a recording's bindings in front of the session, and mark the table
/// loaded so nothing overwrites them with this machine's.
fn restore_bindings(eng: &Engine, value: &serde_json::Value) {
    let Ok(table) = serde_json::from_value::<BTreeMap<String, Vec<String>>>(value.clone()) else {
        tracing::warn!("the recording's input bindings did not parse; using this project's");
        return;
    };
    let actions = eng.resource::<InputActions>();
    let mut actions = actions.borrow_mut();
    actions.bound.clear();
    actions.overrides.clear();
    actions.state.clear();
    actions.loaded = true;
    for (name, bindings) in table {
        if let Err(why) = actions.rebind(&name, &bindings) {
            tracing::warn!("recorded binding for '{name}': {why}");
        }
    }
    // `rebind` files everything as an override; a replay's table is not the
    // player's, so it must not be written back to their file.
    actions.overrides.clear();
}

/// Register the header seam. Called from the plugin's `build`.
pub(crate) fn add_replay_setup(reg: &mut balaur_plugin::Registry<'_>) {
    reg.add_replay_setup("input_bindings", capture_bindings, restore_bindings);
}

/// The `[input.actions]` table of the project's manifest.
///
/// A binding that does not parse is reported and dropped; an action with no
/// usable binding still exists, reading zero, because a game asking for it
/// should get a neutral answer rather than a crash.
fn load(eng: &Engine) -> BTreeMap<String, Vec<Binding>> {
    #[derive(serde::Deserialize, Default)]
    struct InputTable {
        #[serde(default)]
        actions: BTreeMap<String, Vec<String>>,
    }
    #[derive(serde::Deserialize)]
    struct Manifest {
        #[serde(default)]
        input: InputTable,
    }
    let Some(source) = balaur_core::project::manifest_source(eng) else {
        return BTreeMap::new();
    };
    let declared = match toml::from_str::<Manifest>(&source) {
        Ok(manifest) => manifest.input.actions,
        Err(err) => {
            tracing::warn!("project.toml [input.actions]: {err}; no actions declared");
            return BTreeMap::new();
        }
    };
    let mut out = BTreeMap::new();
    for (name, bindings) in declared {
        let mut parsed = Vec::with_capacity(bindings.len());
        for text in &bindings {
            match Binding::parse(text) {
                Ok(binding) => parsed.push(binding),
                Err(why) => tracing::warn!("action '{name}': {why}"),
            }
        }
        out.insert(name, parsed);
    }
    out
}

/// Where a player's rebindings live: beside their saves, not in the project.
fn bindings_path(eng: &Engine) -> PathBuf {
    balaur_core::engine_api::user_data_dir_of(eng).join("input.toml")
}

fn apply_saved_rebindings(eng: &Engine, actions: &mut InputActions) {
    let path = bindings_path(eng);
    let Ok(bytes) = balaur_core::files::backend(eng).read(&path) else {
        return;
    };
    let source = String::from_utf8_lossy(&bytes);
    let saved: BTreeMap<String, Vec<String>> = match toml::from_str(&source) {
        Ok(saved) => saved,
        Err(err) => {
            tracing::warn!("{}: {err}; ignoring the saved bindings", path.display());
            return;
        }
    };
    for (name, bindings) in saved {
        if let Err(why) = actions.rebind(&name, &bindings) {
            tracing::warn!("saved binding for '{name}': {why}");
        }
    }
}

/// Write every rebinding to the user data directory. Called after each
/// `input.bind`, because a rebinding screen that loses an edit on a crash is
/// worse than a file write per click.
fn save_rebindings(eng: &Engine, actions: &InputActions) {
    let table: BTreeMap<String, Vec<String>> = actions
        .overrides
        .iter()
        .map(|(name, list)| (name.clone(), list.iter().map(Binding::text).collect()))
        .collect();
    let path = bindings_path(eng);
    let encoded = match toml::to_string(&table) {
        Ok(encoded) => encoded,
        Err(err) => {
            tracing::error!("encoding rebindings: {err}");
            return;
        }
    };
    let fs = balaur_core::files::backend(eng);
    if let Some(parent) = path.parent() {
        if let Err(err) = fs.mkdir(parent) {
            tracing::error!("creating {}: {err}", parent.display());
            return;
        }
    }
    if let Err(err) = fs.write(&path, encoded.as_bytes()) {
        tracing::error!("writing {}: {err}", path.display());
    }
}

/// Warn once per action nobody declared, the way an unknown key does.
fn check_action(eng: &Engine, name: &str) {
    if eng.resource::<InputActions>().borrow().is_declared(name) {
        return;
    }
    thread_local! {
        static WARNED: std::cell::RefCell<std::collections::BTreeSet<String>> =
            const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
    }
    if WARNED.with_borrow_mut(|w| w.insert(name.to_string())) {
        tracing::warn!(
            action = name,
            "no such action in [input.actions]; it reads 0"
        );
    }
}

/// `input.action_*`, `input.actions`, `input.bind`.
pub(crate) fn install_actions(m: &mut dyn Bindings<Engine>) {
    use balaur_script::{BindingsExt as _, Value};

    m.describe(&[
        ("actions", &[], "", "Every action `[input.actions]` declares, so a rebinding screen can list them."),
        ("action_value", &[], "", "How far the action is pushed, -1 to 1; a key answers 0 or 1, a stick or `keys:A,D` the whole range."),
        ("action_pressed", &[], "", "Whether the action is held down now."),
        ("action_just_pressed", &[], "", "Whether the action went down this frame."),
        ("action_just_released", &[], "", "Whether the action came up this frame."),
        ("bindings", &[], "", "What the action is bound to now, whether from the project or from the player's own rebinding."),
        ("bind", &[], "", "Rebind the action to one binding or a list of them, replacing what it had and saving to the user data directory."),
        ("reset_bindings", &[], "", "Drop every saved rebinding and go back to what the project declared."),
        ("declare_actions", &[], "(actions: any)", "Declare the actions a project's `[input.actions]` would, from a table of name to binding list; for a host running a project other than its own, such as the editor."),
    ]);

    // Every declared action, so a rebinding screen can list them.
    m.function("actions", |eng: &Engine, ()| {
        let names = eng.resource::<InputActions>().borrow().names();
        Ok(Value::List(names.into_iter().map(Value::Str).collect()))
    });
    // -1..1. A digital binding reads 0 or 1; `keys:A,D` and an axis read the
    // whole range, so one action serves a key, a stick and a d-pad at once.
    m.function("action_value", |eng: &Engine, name: String| {
        check_action(eng, &name);
        let v = eng.resource::<InputActions>().borrow().value(&name);
        Ok(v)
    });
    m.function("action_pressed", |eng: &Engine, name: String| {
        check_action(eng, &name);
        let v = eng.resource::<InputActions>().borrow().is_pressed(&name);
        Ok(v)
    });
    m.function("action_just_pressed", |eng: &Engine, name: String| {
        check_action(eng, &name);
        let v = eng.resource::<InputActions>().borrow().just_pressed(&name);
        Ok(v)
    });
    m.function("action_just_released", |eng: &Engine, name: String| {
        check_action(eng, &name);
        let v = eng.resource::<InputActions>().borrow().just_released(&name);
        Ok(v)
    });
    // What the action is bound to now, project declaration or rebinding both.
    m.function("bindings", |eng: &Engine, name: String| {
        check_action(eng, &name);
        let list = eng.resource::<InputActions>().borrow().bindings(&name);
        Ok(Value::List(list.into_iter().map(Value::Str).collect()))
    });
    // `input.bind("jump", "gamepad:North")`, or a list for several. Replaces
    // what the action had and saves to the user data directory.
    m.function("declare_actions", |eng: &Engine, table: Value| {
        let Value::Map(entries) = table else {
            anyhow::bail!("declare_actions takes a table of name to bindings");
        };
        let mut declared = BTreeMap::new();
        for (name, value) in entries {
            let texts = match value {
                Value::Str(one) => vec![one],
                Value::List(many) => many
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::Str(s) => Some(s),
                        _ => None,
                    })
                    .collect(),
                _ => continue,
            };
            let mut parsed = Vec::with_capacity(texts.len());
            for text in &texts {
                match Binding::parse(text) {
                    Ok(binding) => parsed.push(binding),
                    Err(why) => tracing::warn!("action '{name}': {why}"),
                }
            }
            declared.insert(name, parsed);
        }
        eng.resource::<InputActions>()
            .borrow_mut()
            .declare(declared);
        Ok(())
    });
    install_rebinding(m);
}

/// The half a player drives rather than the project: `bind` and the reset
/// that undoes every one of them.
fn install_rebinding(m: &mut dyn Bindings<Engine>) {
    use balaur_script::{BindingsExt as _, Value};

    m.function("bind", |eng: &Engine, (name, bindings): (String, Value)| {
        let bindings = match bindings {
            Value::Str(one) => vec![one],
            Value::List(many) => many
                .into_iter()
                .map(|v| match v {
                    Value::Str(s) => Ok(s),
                    other => Err(anyhow::anyhow!(
                        "a binding is a string, got {}",
                        other.type_name()
                    )),
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            other => anyhow::bail!("a binding is a string or a list, got {}", other.type_name()),
        };
        let actions = eng.resource::<InputActions>();
        actions
            .borrow_mut()
            .rebind(&name, &bindings)
            .map_err(|why| anyhow::anyhow!("{why}"))?;
        save_rebindings(eng, &actions.borrow());
        Ok(())
    });
    // Back to what the project declared, and the saved file with it.
    m.function("reset_bindings", |eng: &Engine, ()| {
        let project = load(eng);
        let actions = eng.resource::<InputActions>();
        actions.borrow_mut().clear_overrides(&project);
        save_rebindings(eng, &actions.borrow());
        Ok(())
    });
}
